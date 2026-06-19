use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use serde_json::json;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdout, Command};

use crate::common::{address_to_bech32, get_simulator_chain_id, wait_for_simulator_ready};

async fn read_json_response(reader: &mut BufReader<ChildStdout>) -> String {
    let mut line = String::new();
    loop {
        line.clear();
        let bytes = reader
            .read_line(&mut line)
            .await
            .expect("Failed to read line");
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
async fn test_balance_tools() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    let wallet = interactor.register_wallet(test_wallets::alice()).await;
    let wallet_bech32 = address_to_bech32(&wallet);

    // Fund wallet
    crate::common::fund_address_on_simulator(&wallet_bech32, "100000000000000000000", &gateway_url).await; // 100 EGLD

    let chain_id = get_simulator_chain_id(&gateway_url).await;

    // Start MCP Server process
    // Path relative to mx-agentic-commerce-tests root
    let mcp_path = "dist/index.js";
    let working_dir = "../multiversx-mcp-server";

    let mut child = Command::new("node")
        .arg(mcp_path)
        .arg("mcp")
        .current_dir(working_dir)
        .env("MULTIVERSX_API_URL", &gateway_url)
        .env("MULTIVERSX_CHAIN_ID", &chain_id)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit()) // See logs in test output
        .spawn()
        .expect("Failed to start MCP server");

    let stdin = child.stdin.as_mut().expect("Failed to open stdin");
    let stdout = child.stdout.take().expect("Failed to open stdout");
    let mut reader = BufReader::new(stdout);

    // 1. Initialize MCP
    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test-client", "version": "1.0.0" }
        }
    });

    let req_str = init_req.to_string();
    stdin.write_all(req_str.as_bytes()).await.unwrap();
    stdin.write_all(b"\n").await.unwrap();

    // Read initialize response
    let line = read_json_response(&mut reader).await;
    println!("MCP Init Response: {}", line);
    assert!(line.contains("serverInfo"), "Failed to initialize MCP");

    // Send initialized notification
    let initialized_notif = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    stdin
        .write_all(initialized_notif.to_string().as_bytes())
        .await
        .unwrap();
    stdin.write_all(b"\n").await.unwrap();

    // 2. Call get-balance
    let call_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "get-balance",
            "arguments": {
                "address": wallet_bech32
            }
        }
    });

    stdin
        .write_all(call_req.to_string().as_bytes())
        .await
        .unwrap();
    stdin.write_all(b"\n").await.unwrap();

    let line = read_json_response(&mut reader).await;
    println!("MCP Call Response: {}", line);

    // Parse response
    let resp: serde_json::Value = serde_json::from_str(&line).expect("Invalid JSON");
    // Ensure not error
    if let Some(err) = resp.get("error") {
        panic!("MCP returned error: {:?}", err);
    }

    let content = resp["result"]["content"][0]["text"].as_str().unwrap();
    println!("Balance output: {}", content);

    // Should be approx 100 EGLD (minus gas if any?, no, query is free?)
    // Actually we funded 100 EGLD
    assert!(content.contains("100"), "Balance should reflect 100 EGLD");

    // Cleanup
    child.kill().await.expect("Failed to kill MCP server");
}
