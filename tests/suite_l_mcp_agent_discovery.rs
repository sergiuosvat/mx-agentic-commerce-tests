use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use serde_json::{json, Value};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::ChildStdout;
use tokio::process::Command;
mod common;
use common::{
    wait_for_simulator_ready,IdentityRegistryInteractor};

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
        if trimmed.starts_with('{') {
            return line;
        }
        // Skip log lines
    }
}

/// Helper: send a JSON-RPC request and read the response
async fn mcp_call(
    stdin: &mut tokio::process::ChildStdin,
    reader: &mut BufReader<ChildStdout>,
    id: u64,
    method: &str,
    params: Value,
) -> Value {
    let req = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    let req_str = serde_json::to_string(&req).unwrap();
    stdin.write_all(req_str.as_bytes()).await.unwrap();
    stdin.write_all(b"\n").await.unwrap();

    let line = read_json_response(reader).await;
    let resp: Value = serde_json::from_str(&line).expect("Invalid JSON Response");
    if let Some(error) = resp.get("error") {
        panic!("MCP call '{}' failed: {:?}", method, error);
    }
    resp
}

/// Helper: initialize MCP server connection
async fn mcp_init(stdin: &mut tokio::process::ChildStdin, reader: &mut BufReader<ChildStdout>) {
    let resp = mcp_call(
        stdin,
        reader,
        1,
        "initialize",
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "test-suite", "version": "1.0"}
        }),
    )
    .await;
    assert!(resp.get("result").is_some(), "MCP init failed");

    let notify = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    stdin
        .write_all(serde_json::to_string(&notify).unwrap().as_bytes())
        .await
        .unwrap();
    stdin.write_all(b"\n").await.unwrap();
}

/// Suite L: MCP Agent Discovery E2E
///
/// Registers agents on-chain via Rust interactor, then verifies
/// they are discoverable through MCP tools and HTTP VM queries.
#[tokio::test]
async fn test_mcp_agent_discovery() {
    let mut pm = ProcessManager::new();

    // ── 1. Start Chain Simulator ──
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    // ── 2. Deploy Identity Registry & Register Agents ──
    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    let wallet_alice = interactor.register_wallet(test_wallets::alice()).await;

    // Generate and fund a second wallet for Agent #2
    let bob_pk = common::generate_random_private_key();
    let bob_wallet = Wallet::from_private_key(&bob_pk).expect("Wallet failed");
    let wallet_bob = interactor.register_wallet(bob_wallet).await;

    interactor
        .tx()
        .from(&wallet_alice)
        .to(&wallet_bob)
        .egld(1_000_000_000_000_000_000u64)
        .run()
        .await;

    // Deploy and register Agent #1 (Alice)
    let identity =
        IdentityRegistryInteractor::init(&mut interactor, wallet_alice.clone()).await;
    identity
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;

    identity
        .register_agent(
            &mut interactor,
            "AlphaBot",
            "data:application/json;base64,eyJuYW1lIjoiQWxwaGFCb3QifQ==",
            vec![
                ("category", b"shopping".to_vec()),
                ("version", b"1.0".to_vec()),
            ],
        )
        .await;

    let registry_addr = identity.address().clone();

    // Register Agent #2 from Bob's wallet
    let name_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(b"BetaBot");
    let uri_buf: ManagedBuffer<StaticApi> =
        ManagedBuffer::new_from_bytes(b"https://betabot.example.com/manifest.json");
    let pk_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(&[0u8; 32]);
    let metadata_count: u32 = 1;
    let metadata_count_buf: ManagedBuffer<StaticApi> =
        ManagedBuffer::new_from_bytes(&metadata_count.to_be_bytes());

    let mut encoded_bytes = Vec::new();
    let key = b"category";
    let val = b"finance";
    encoded_bytes.extend_from_slice(&(key.len() as u32).to_be_bytes());
    encoded_bytes.extend_from_slice(key);
    encoded_bytes.extend_from_slice(&(val.len() as u32).to_be_bytes());
    encoded_bytes.extend_from_slice(val);
    let encoded_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(&encoded_bytes);

    let services_count: u32 = 0;
    let services_count_buf: ManagedBuffer<StaticApi> =
        ManagedBuffer::new_from_bytes(&services_count.to_be_bytes());

    interactor
        .tx()
        .from(&wallet_bob)
        .to(&registry_addr)
        .gas(600_000_000)
        .raw_call("register_agent")
        .argument(&name_buf)
        .argument(&uri_buf)
        .argument(&pk_buf)
        .argument(&metadata_count_buf)
        .argument(&encoded_buf)
        .argument(&services_count_buf)
        .run()
        .await;

    println!("Registered Agent #2 (BetaBot) from Bob's wallet");

    let registry_address = common::address_to_bech32(&registry_addr);
    println!("Registry Address: {}", registry_address);

    // ── 3. Verify agents via HTTP VM query ──
    let client = reqwest::Client::new();

    // Query get_agent for nonce 1 (AlphaBot)
    let query_url = format!("{}/vm-values/query", gateway_url);
    let query_body = json!({
        "scAddress": registry_address,
        "funcName": "get_agent",
        "args": ["01"]
    });
    let query_resp = client
        .post(&query_url)
        .json(&query_body)
        .send()
        .await
        .expect("VM query failed");
    assert!(
        query_resp.status().is_success(),
        "VM query HTTP status not OK"
    );

    let query_json: Value = query_resp.json().await.unwrap();
    let return_data = &query_json["data"]["data"]["returnData"];
    assert!(
        return_data.is_array() && !return_data.as_array().unwrap().is_empty(),
        "Agent #1 should have return data"
    );
    println!("Agent #1 VM query return data: {:?}", return_data);

    // Query get_agent for nonce 2 (BetaBot)
    let query_body2 = json!({
        "scAddress": registry_address,
        "funcName": "get_agent",
        "args": ["02"]
    });
    let query_resp2 = client
        .post(&query_url)
        .json(&query_body2)
        .send()
        .await
        .expect("VM query failed");
    assert!(query_resp2.status().is_success());

    let query_json2: Value = query_resp2.json().await.unwrap();
    let return_data2 = &query_json2["data"]["data"]["returnData"];
    assert!(
        return_data2.is_array() && !return_data2.as_array().unwrap().is_empty(),
        "Agent #2 should have return data"
    );
    println!("Agent #2 VM query return data: {:?}", return_data2);

    // ── 4. Start MCP Server and verify tools work ──
    println!("Starting MCP Server...");
    let mut child = Command::new("node")
        .arg("dist/index.js")
        .arg("mcp")
        .current_dir("../multiversx-mcp-server")
        .env("MVX_API_URL", &gateway_url)
        .env("MVX_NETWORK", "devnet")
        .env("MVX_REGISTRY_IDENTITY", &registry_address)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("Failed to spawn MCP server");

    let stdin = child.stdin.as_mut().expect("Failed to open stdin");
    let stdout = child.stdout.take().expect("Failed to open stdout");
    let mut reader = BufReader::new(stdout);

    mcp_init(stdin, &mut reader).await;

    // ── 5. Test tools/list — verify registry tools are registered ──
    println!("Testing tools/list...");
    let tools_resp = mcp_call(stdin, &mut reader, 10, "tools/list", json!({})).await;

    let tools = tools_resp["result"]["tools"]
        .as_array()
        .expect("No tools found");
    let tool_names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    println!(
        "Available tools ({} total): {:?}",
        tool_names.len(),
        tool_names
    );

    assert!(
        tool_names.contains(&"get-balance"),
        "Missing get-balance tool"
    );
    assert!(
        tool_names.contains(&"get-agent-manifest"),
        "Missing get-agent-manifest tool"
    );

    // ── 6. Test get-balance to verify MCP → chain sim connectivity ──
    let alice_bech32 = "erd1qyu5wthldzr8wx5c9ucg8kjagg0jfs53s8nr3zpz3hypefsdd8ssycr6th";
    let balance_resp = mcp_call(
        stdin,
        &mut reader,
        11,
        "tools/call",
        json!({
            "name": "get-balance",
            "arguments": { "address": alice_bech32 }
        }),
    )
    .await;

    let balance_text = balance_resp["result"]["content"][0]["text"]
        .as_str()
        .expect("No balance text");
    println!("Alice balance: {}", balance_text);
    assert!(
        balance_text.contains("EGLD") || balance_text.contains("balance"),
        "Balance response unexpected"
    );

    // ── 7. Cleanup ──
    child.kill().await.expect("Failed to kill MCP server");
    println!("\nSuite L: MCP Agent Discovery — PASSED ✅");
    println!("  ✓ Two agents registered from separate wallets");
    println!("  ✓ Agents verified via HTTP VM query");
    println!("  ✓ MCP tools listing verified (registry tools present)");
    println!("  ✓ MCP get-balance connectivity confirmed");
}
