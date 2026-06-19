use mx_agentic_commerce_tests::ProcessManager;
use multiversx_sc_snippets::imports::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use serde_json::{json, Value};
use std::process::Stdio;
use tokio::process::ChildStdout;

mod common;
use common::wait_for_simulator_ready;

async fn read_json_response(reader: &mut BufReader<ChildStdout>) -> String {
    let mut line = String::new();
    loop {
        line.clear();
        let bytes = reader.read_line(&mut line).await.expect("Failed to read line");
        if bytes == 0 {
            panic!("Unexpected EOF from MCP Server");
        }
        let trimmed = line.trim();
        if trimmed.starts_with("{") {
            return line;
        }
        println!("Ignored Log: {}", trimmed);
    }
}

#[tokio::test]
async fn test_mcp_features() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator().unwrap(); // .expect("Failed to start Sim");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    let chain_id = common::get_simulator_chain_id(&gateway_url).await;
    println!("Simulator ChainID: {}", chain_id);

    println!("Starting MCP Server...");
    let mut child = Command::new("node")
        .arg("dist/index.js")
        .arg("mcp")
        .current_dir("../multiversx-mcp-server")
        .env("MULTIVERSX_API_URL", &gateway_url)
        .env("MULTIVERSX_CHAIN_ID", &chain_id)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("Failed to spawn MCP server");

    let stdin = child.stdin.as_mut().expect("Failed to open stdin");
    let stdout = child.stdout.take().expect("Failed to open stdout");
    let mut reader = BufReader::new(stdout);

    // 1. Initialize
    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "test-suite", "version": "1.0"}
        }
    });
    
    let init_str = serde_json::to_string(&init_req).unwrap();
    stdin.write_all(init_str.as_bytes()).await.unwrap();
    stdin.write_all(b"\n").await.unwrap();
    
    // Read response
    let line = read_json_response(&mut reader).await;
    println!("MCP Init Resp: {}", line);
    let resp: Value = serde_json::from_str(&line).expect("Invalid JSON Response");
    if let Some(error) = resp.get("error") {
        panic!("MCP Init Failed: {:?}", error);
    }
    assert!(resp.get("result").is_some());
    
    // 2. Initialized Notification
    let notify = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    stdin.write_all(serde_json::to_string(&notify).unwrap().as_bytes()).await.unwrap();
    stdin.write_all(b"\n").await.unwrap();

    // 3. List Tools
    let list_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });
    stdin.write_all(serde_json::to_string(&list_req).unwrap().as_bytes()).await.unwrap();
    stdin.write_all(b"\n").await.unwrap();
    
    let line = read_json_response(&mut reader).await;
    println!("Tools List: {}", line);
    let tools_resp: Value = serde_json::from_str(&line).expect("Invalid JSON");
    if let Some(error) = tools_resp.get("error") {
        panic!("Tools List Failed: {:?}", error);
    }
    let tools = tools_resp["result"]["tools"].as_array().expect("No tools found");
    assert!(!tools.is_empty(), "No tools returned");
    
    // Verify specific tools exist
    let tool_names: Vec<&str> = tools.iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    println!("Available Tools: {:?}", tool_names);
    assert!(tool_names.contains(&"get-balance"));
    assert!(tool_names.contains(&"send-egld"));

    // 4. Call get-balance (Alice)
    let alice_addr = "erd1qyu5wthldzr8wx5c9ucg8kjagg0jfs53s8nr3zpz3hypefsdd8ssycr6th"; 
    
    let call_req = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "get-balance",
            "arguments": {
                "address": alice_addr
            }
        }
    });
    stdin.write_all(serde_json::to_string(&call_req).unwrap().as_bytes()).await.unwrap();
    stdin.write_all(b"\n").await.unwrap();

    let line = read_json_response(&mut reader).await;
    println!("Get Balance Resp: {}", line);
    let call_resp: Value = serde_json::from_str(&line).expect("Invalid JSON");
    if let Some(error) = call_resp.get("error") {
        panic!("Tool Call Failed: {:?}", error);
    }
    
    let content = call_resp["result"]["content"][0]["text"].as_str().unwrap();
    println!("Balance Content: {}", content);
    assert!(content.to_lowercase().contains("balance"), "Response should contain 'balance'");
    assert!(content.contains("EGLD"), "Response should contain 'EGLD'");

    // Kill child
    child.kill().await.expect("Failed to kill");
}
