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
    }
}

async fn mcp_call(
    stdin: &mut tokio::process::ChildStdin,
    reader: &mut BufReader<ChildStdout>,
    id: u64,
    method: &str,
    params: Value,
) -> Value {
    let req = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    stdin
        .write_all(serde_json::to_string(&req).unwrap().as_bytes())
        .await
        .unwrap();
    stdin.write_all(b"\n").await.unwrap();
    let line = read_json_response(reader).await;
    let resp: Value = serde_json::from_str(&line).expect("Invalid JSON Response");
    if let Some(error) = resp.get("error") {
        panic!("MCP call '{}' failed: {:?}", method, error);
    }
    resp
}

async fn mcp_init(stdin: &mut tokio::process::ChildStdin, reader: &mut BufReader<ChildStdout>) {
    let resp = mcp_call(
        stdin,
        reader,
        1,
        "initialize",
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "test-suite-m", "version": "1.0"}
        }),
    )
    .await;
    assert!(resp.get("result").is_some());
    let notify = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    stdin
        .write_all(serde_json::to_string(&notify).unwrap().as_bytes())
        .await
        .unwrap();
    stdin.write_all(b"\n").await.unwrap();
}

/// Suite M: Agent-to-Agent Discovery via MCP
///
/// Tests the full agent discovery lifecycle:
/// 1. Deploy Identity Registry
/// 2. Register Agent A and Agent B via Rust interactor
/// 3. Start MCP Server
/// 4. Agent A discovers Agent B via get-agent-manifest
/// 5. Verify both agents' manifests
#[tokio::test]
async fn test_agent_to_agent_discovery() {
    let mut pm = ProcessManager::new();

    // ── 1. Start Chain Simulator ──
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    let chain_id = common::get_simulator_chain_id(&gateway_url).await;
    println!("Simulator ChainID: {}", chain_id);

    // ── 2. Setup wallets ──
    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    let wallet_alice = interactor.register_wallet(test_wallets::alice()).await;
    let wallet_bob = interactor.register_wallet(test_wallets::bob()).await;

    // ── 3. Deploy Identity Registry & Register Agents ──
    let identity =
        IdentityRegistryInteractor::init(&mut interactor, wallet_alice.clone()).await;
    identity
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;

    let registry_address = common::address_to_bech32(identity.address());
    println!("Registry Address: {}", registry_address);

    // Register Agent A (MoltBot) from Alice — nonce=1
    identity
        .register_agent(
            &mut interactor,
            "MoltBot",
            "https://moltbot.example.com/manifest.json",
            vec![
                ("category", b"assistant".to_vec()),
                ("version", b"1.0".to_vec()),
            ],
        )
        .await;
    println!("Agent A (MoltBot) registered as nonce=1");

    let registry_addr = identity.address().clone();

    // Register Agent B (ServiceBot) from Bob's wallet — nonce=2
    // Must use a different wallet since contract enforces 1 agent per address
    let name_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(b"ServiceBot");
    let uri_buf: ManagedBuffer<StaticApi> =
        ManagedBuffer::new_from_bytes(b"https://servicebot.example.com/arf.json");
    let pk_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(&[0u8; 32]);

    let metadata_count: u32 = 2;
    let metadata_count_buf: ManagedBuffer<StaticApi> =
        ManagedBuffer::new_from_bytes(&metadata_count.to_be_bytes());

    // Encode metadata entries (category=computation, version=2.0)
    let mut enc1 = Vec::new();
    enc1.extend_from_slice(&(8u32).to_be_bytes());
    enc1.extend_from_slice(b"category");
    enc1.extend_from_slice(&(11u32).to_be_bytes());
    enc1.extend_from_slice(b"computation");
    let enc1_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(&enc1);

    let mut enc2 = Vec::new();
    enc2.extend_from_slice(&(7u32).to_be_bytes());
    enc2.extend_from_slice(b"version");
    enc2.extend_from_slice(&(3u32).to_be_bytes());
    enc2.extend_from_slice(b"2.0");
    let enc2_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(&enc2);

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
        .argument(&enc1_buf)
        .argument(&enc2_buf)
        .argument(&services_count_buf)
        .run()
        .await;
    println!("Agent B (ServiceBot) registered as nonce=2");

    // ── 4. Start MCP Server ──
    println!("Starting MCP Server...");
    let mut mcp_child = Command::new("node")
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

    let mcp_stdin = mcp_child.stdin.as_mut().expect("stdin");
    let mcp_stdout = mcp_child.stdout.take().expect("stdout");
    let mut mcp_reader = BufReader::new(mcp_stdout);

    mcp_init(mcp_stdin, &mut mcp_reader).await;

    // ── 5. Agent A discovers Agent B via MCP ──
    println!("Agent A querying Agent B's manifest via MCP...");
    let resp = mcp_call(
        mcp_stdin,
        &mut mcp_reader,
        10,
        "tools/call",
        json!({
            "name": "get-agent-manifest",
            "arguments": { "agentNonce": 2 }
        }),
    )
    .await;

    let manifest_text = resp["result"]["content"][0]["text"]
        .as_str()
        .expect("No manifest text");
    println!("Agent B Manifest: {}", manifest_text);
    let manifest: Value = serde_json::from_str(manifest_text).unwrap();
    assert_eq!(manifest["name"].as_str().unwrap(), "ServiceBot");

    // ── 6. Also verify Agent A's manifest ──
    println!("Querying Agent A's manifest via MCP...");
    let resp = mcp_call(
        mcp_stdin,
        &mut mcp_reader,
        11,
        "tools/call",
        json!({
            "name": "get-agent-manifest",
            "arguments": { "agentNonce": 1 }
        }),
    )
    .await;

    let agent_a_text = resp["result"]["content"][0]["text"]
        .as_str()
        .expect("No Agent A manifest text");
    println!("Agent A Manifest: {}", agent_a_text);
    let agent_a: Value = serde_json::from_str(agent_a_text).unwrap();
    assert_eq!(agent_a["name"].as_str().unwrap(), "MoltBot");

    // ── 7. Cross-discovery: verify both agents visible ──
    println!("Verifying agent cross-discovery...");

    // Query balance to ensure basic MCP connectivity
    let alice_addr = "erd1qyu5wthldzr8wx5c9ucg8kjagg0jfs53s8nr3zpz3hypefsdd8ssycr6th";
    let resp = mcp_call(
        mcp_stdin,
        &mut mcp_reader,
        12,
        "tools/call",
        json!({
            "name": "get-balance",
            "arguments": { "address": alice_addr }
        }),
    )
    .await;
    let balance_text = resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("no text");
    println!("Alice balance: {}", balance_text);

    // ── Cleanup ──
    mcp_child.kill().await.expect("Failed to kill MCP");

    println!("\nSuite M: Agent-to-Agent Discovery — PASSED ✅");
    println!("  Agent A (MoltBot) → MCP discovery → Agent B (ServiceBot)");
    println!("  Both manifests verified via get-agent-manifest");
}
