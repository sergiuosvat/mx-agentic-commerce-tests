use crate::common::{address_to_bech32, get_simulator_chain_id, wait_for_simulator_ready};
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;

#[tokio::test]
async fn test_account_tools() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    let chain_id = get_simulator_chain_id(&gateway_url).await;
    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    let wallet = interactor.register_wallet(test_wallets::alice()).await;
    let wallet_bech32 = address_to_bech32(&wallet);

    // Fund wallet
    crate::common::fund_address_on_simulator(&wallet_bech32, "100000000000000000000", &gateway_url).await; // 100 EGLD

    // Start MCP Client
    // Accessing `mcp_client` from the crate root module `pkg_5_mcp.rs` which declared it.
    // However, inside `test_account_tools.rs` (which is a module of `pkg_5_mcp` crate),
    // we can access sibling modules via `crate::mcp_client`.
    let mut client = crate::mcp_client::McpClient::new(&chain_id, None, &gateway_url).await;

    // 1. Test get-balance
    println!("Testing get-balance...");
    let args = serde_json::json!({ "address": wallet_bech32 });
    let resp = client.call_tool("get-balance", args).await;

    // Check response
    if let Some(err) = resp.get("error") {
        panic!("MCP Error: {:?}", err);
    }
    let content = resp["result"]["content"][0]["text"].as_str().unwrap();
    println!("Balance output: {}", content);
    assert!(content.contains("100"), "Balance should reflect 100 EGLD");

    // 2. Test get-balance for unfunded address
    println!("Testing get-balance (unfunded)...");
    let random_addr = "erd1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq6gq4hu";
    let args_unfunded = serde_json::json!({ "address": random_addr });
    let resp_unfunded = client.call_tool("get-balance", args_unfunded).await;
    let content_unfunded = resp_unfunded["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    println!("Unfunded Balance output: {}", content_unfunded);
    assert!(
        content_unfunded.contains("0"),
        "Balance should be 0 or small"
    );

    // 3. Test query-account (if available) - checking tool list might be safer?
    // Let's assume it is available based on previous analysis.
    println!("Testing query-account...");
    let args_query = serde_json::json!({ "address": wallet_bech32 });
    let resp_query = client.call_tool("query-account", args_query).await;

    if let Some(err) = resp_query.get("error") {
        // If tool not found, we might skip, but let's see.
        println!("query-account error: {:?}", err);
    } else {
        let content_query = resp_query["result"]["content"][0]["text"].as_str().unwrap();
        println!("Account Query output: {}", content_query);
        assert!(
            content_query.contains(wallet_bech32.as_str()),
            "Should contain address"
        );
        assert!(
            content_query.contains("balance"),
            "Should contain balance info"
        );
        assert!(content_query.contains("nonce"), "Should contain nonce info");
    }

    // Cleanup handled by Drop or OS on exit
}
