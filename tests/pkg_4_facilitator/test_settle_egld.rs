use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use reqwest::Client;
use serde_json::json;
use std::process::Command;
use tokio::time::{sleep, Duration};

use crate::common::{
    generate_random_private_key, get_simulator_chain_id,
};

#[tokio::test]
async fn test_settle_egld() {
    let mut pm = ProcessManager::new();
    let sim_port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", sim_port);

    // Setup Facilitator
    let chain_id = get_simulator_chain_id(&gateway_url).await;
    println!("Simulator Chain ID: {}", chain_id);
    let facilitator_pk = generate_random_private_key();
    let port = 3045; // Avoid conflict with potential stale processes

    let env_vars = vec![
        ("PORT", "3045"),
        ("PRIVATE_KEY", facilitator_pk.as_str()),
        (
            "REGISTRY_ADDRESS",
            "erd1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq6gq4hu",
        ),
        ("NETWORK_PROVIDER", gateway_url.as_str()),
        ("GATEWAY_URL", gateway_url.as_str()),
        ("CHAIN_ID", chain_id.as_str()),
        ("SKIP_SIMULATION", "false"),
    ];

    pm.start_node_service(
        "Facilitator",
        "../x402_integration/x402_facilitator",
        "dist/index.js",
        env_vars,
        port,
    )
    .expect("Failed to start facilitator");

    sleep(Duration::from_secs(4)).await;

    // 1. Setup Sender (User) and Receiver
    let sender_pk_hex = generate_random_private_key();
    let sender_wallet = Wallet::from_private_key(&sender_pk_hex).unwrap();
    let sender_bech32 = sender_wallet.to_address().to_bech32("erd").to_string();

    let receiver_pk = generate_random_private_key();
    let receiver_wallet = Wallet::from_private_key(&receiver_pk).unwrap();
    let receiver_bech32 = receiver_wallet.to_address().to_bech32("erd").to_string();

    // Fund sender
    crate::common::fund_address_on_simulator(&sender_bech32, "1000000000000000000000", &gateway_url).await; // 1000 EGLD

    // Verify sender exists
    let client = Client::new();
    let acc_resp = client
        .get(format!("{}/address/{}", gateway_url, sender_bech32))
        .send()
        .await
        .expect("Failed to get sender")
        .json::<serde_json::Value>()
        .await
        .expect("Failed to parse sender JSON");
    println!("Sender State: {:?}", acc_resp);

    // 2. Sign Transaction using external script
    let value = "1000000000000000000"; // 1 EGLD
    let nonce = "0";
    let gas_limit = "70000"; // Increased gas limit
    let gas_price = "1000000000";

    let output = Command::new("npx")
        .arg("ts-node")
        .arg("scripts/sign_tx.ts")
        .arg("--sender-pk")
        .arg(&sender_pk_hex)
        .arg("--receiver")
        .arg(&receiver_bech32)
        .arg("--value")
        .arg(value)
        .arg("--nonce")
        .arg(nonce)
        .arg("--gas-limit")
        .arg(gas_limit)
        .arg("--gas-price")
        .arg(gas_price)
        .arg("--chain-id")
        .arg(&chain_id)
        .current_dir("../moltbot-starter-kit")
        .output()
        .expect("Failed to run signing script");

    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        panic!("signing script failed: stderr={}", stderr);
    }
    println!("Sign Script Stderr: {}", stderr);

    let signed_tx_json = String::from_utf8_lossy(&output.stdout);
    let signed_tx: serde_json::Value =
        serde_json::from_str(&signed_tx_json).expect("Failed to parse signed tx JSON");

    println!("Signed Tx: {}", signed_tx_json);

    // 3. Prepare Payload
    // Use the output from sign_tx.ts directly, ensuring data and options are correct
    let mut payload = signed_tx.clone();

    // Ensure data is "" if null or missing
    if payload.get("data").is_none() || payload["data"].is_null() {
        payload["data"] = json!("");
    }

    // Ensure options is 0 if missing
    if payload.get("options").is_none() {
        payload["options"] = json!(0);
    }

    let requirements = json!({
        "payTo": receiver_bech32,
        "amount": value,
        "asset": "EGLD",
        "network": format!("multiversx:{}", chain_id)
    });

    let body = json!({
        "scheme": "exact",
        "payload": payload,
        "requirements": requirements
    });

    // 4. Send /settle Request
    let client = Client::new();
    let resp = client
        .post(format!("http://localhost:{}/settle", port))
        .json(&body)
        .send()
        .await
        .expect("Failed to send request");

    // 5. Verify Response
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        panic!("Request failed: status={}, body={}", status, text);
    }

    let resp_json: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    // println!("Settle Response: {:?}", resp_json);

    assert_eq!(resp_json["success"], true);

    // 6. Verify On-Chain
    // 6. Verify On-Chain
    crate::common::generate_blocks_on_simulator(5, &gateway_url).await;
    sleep(Duration::from_secs(5)).await;

    // Check receiver balance via HTTP API
    let balance_resp = client
        .get(format!("{}/address/{}", gateway_url, receiver_bech32))
        .send()
        .await
        .expect("Failed to get balance")
        .json::<serde_json::Value>()
        .await
        .expect("Failed to parse balance JSON");

    let balance_str = balance_resp["data"]["account"]["balance"]
        .as_str()
        .unwrap_or("0");

    println!("Receiver Balance: {}", balance_str);
    assert_eq!(balance_str, "1000000000000000000");
}
