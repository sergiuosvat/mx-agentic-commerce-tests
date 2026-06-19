use crate::common::{address_to_bech32, get_simulator_chain_id, TestEnv};
use multiversx_sc_snippets::imports::*;

#[tokio::test]
async fn test_account_tools() {
    let env = TestEnv::chain_only().await;
    std::mem::forget(env.pm);
    let gateway_url = env.gateway_url.clone();
    let wallet = env.owner.clone();
    let wallet_bech32 = address_to_bech32(&wallet);

    let chain_id = get_simulator_chain_id(&gateway_url).await;
    let mut client = crate::mcp_client::McpClient::new(&chain_id, None, &gateway_url).await;

    println!("Testing get-balance...");
    let args = serde_json::json!({ "address": wallet_bech32 });
    let resp = client.call_tool("get-balance", args).await;

    if let Some(err) = resp.get("error") {
        panic!("MCP Error: {:?}", err);
    }
    let content = resp["result"]["content"][0]["text"].as_str().unwrap();
    println!("Balance output: {}", content);
    assert!(content.contains("100"), "Balance should reflect 100 EGLD");

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

    println!("Testing query-account...");
    let args_query = serde_json::json!({ "address": wallet_bech32 });
    let resp_query = client.call_tool("query-account", args_query).await;

    if let Some(err) = resp_query.get("error") {
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
}
