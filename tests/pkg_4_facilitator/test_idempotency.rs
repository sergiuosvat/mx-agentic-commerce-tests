use multiversx_sc_snippets::imports::*;
use reqwest::Client;
use serde_json::json;
use std::process::Command;
use tokio::time::{sleep, Duration};

use crate::common::{
    address_to_bech32, generate_random_private_key, get_simulator_chain_id,
    start_facilitator, IdentityRegistryInteractor, TestEnv,
};

#[tokio::test]
async fn test_idempotency() {
    let env = TestEnv::chain_only().await;
    let mut pm = env.pm;
    let gateway_url = env.gateway_url.clone();
    let mut interactor = env.interactor;

    let chain_id = get_simulator_chain_id(&gateway_url).await;
    let facilitator_pk = generate_random_private_key();

    let owner = env.owner.clone();
    let identity = IdentityRegistryInteractor::init(&mut interactor, owner).await;
    let registry_address = address_to_bech32(identity.address());

    let facilitator_url = start_facilitator(
        &mut pm,
        &facilitator_pk,
        &registry_address,
        &gateway_url,
        &chain_id,
        &[("SKIP_SIMULATION", "false")],
    )
    .await;

    // 1. Setup Sender and Receiver
    let sender_pk_hex = generate_random_private_key();
    let sender_wallet = Wallet::from_private_key(&sender_pk_hex).unwrap();
    let sender_bech32 = sender_wallet.to_address().to_bech32("erd").to_string();

    let receiver_pk = generate_random_private_key();
    let receiver_wallet = Wallet::from_private_key(&receiver_pk).unwrap();
    let receiver_bech32 = receiver_wallet.to_address().to_bech32("erd").to_string();

    // Fund sender
    crate::common::fund_address_on_simulator(&sender_bech32, "1000000000000000000000", &gateway_url).await; // 1000 EGLD

    // 2. Sign Tx
    let value_str = "1000000000000000000"; // 1 EGLD

    let output = Command::new("npx")
        .arg("ts-node")
        .arg("scripts/sign_tx.ts")
        .arg("--sender-pk")
        .arg(&sender_pk_hex)
        .arg("--receiver")
        .arg(&receiver_bech32)
        .arg("--value")
        .arg(value_str)
        .arg("--nonce")
        .arg("0")
        .arg("--gas-limit")
        .arg("70000")
        .arg("--gas-price")
        .arg("1000000000")
        .arg("--chain-id")
        .arg(&chain_id)
        .current_dir("../moltbot-starter-kit")
        .output()
        .expect("Failed to run signing script");

    let signed_tx_json = String::from_utf8_lossy(&output.stdout);
    let payload_val: serde_json::Value =
        serde_json::from_str(&signed_tx_json).expect("Failed to parse signed tx JSON");

    // Fix payload fields if needed (same logic as before)
    let mut payload = payload_val.clone();
    if payload.get("data").is_none() || payload["data"].is_null() {
        payload["data"] = json!("");
    }
    if payload.get("options").is_none() {
        payload["options"] = json!(0);
    }

    let requirements = json!({
        "payTo": receiver_bech32,
        "amount": value_str,
        "asset": "EGLD",
        "network": format!("multiversx:{}", chain_id)
    });

    let body = json!({
        "scheme": "exact",
        "payload": payload,
        "requirements": requirements
    });

    let client = Client::new();

    // 3. First Settle - Should Succeed
    println!("Sending First Settle Request...");
    let resp1 = client
        .post(format!("{facilitator_url}/settle"))
        .json(&body)
        .send()
        .await
        .expect("Failed to send first request");

    assert!(resp1.status().is_success());
    let resp1_json: serde_json::Value = resp1.json().await.unwrap();
    assert_eq!(resp1_json["success"], true);

    // Wait for tx to be mined so nonce increments on chain
    crate::common::generate_blocks_on_simulator(5, &gateway_url).await;
    sleep(Duration::from_secs(3)).await;

    // 4. Second Settle (Replay) - Should Succeed Idempotently
    // The facilitator should detect duplicate settlement ID and return existing hash.
    println!("Sending Second Settle Request (Replay)...");
    let resp2 = client
        .post(format!("{facilitator_url}/settle"))
        .json(&body)
        .send()
        .await
        .expect("Failed to send second request");

    let resp2_json: serde_json::Value = resp2.json().await.unwrap();
    println!("Replay Response: {:?}", resp2_json);

    // Verify Idempotency
    assert_eq!(
        resp2_json["success"], true,
        "Idempotent request should return success"
    );
    assert_eq!(
        resp2_json["txHash"], resp1_json["txHash"],
        "Tx hash should match original"
    );

    // 5. Verify Receiver Balance (Should be +1 EGLD, not +2)
    // Wait for blocks if needed, but if it returned generic success, we assume no new tx broadcast.

    let account_url = format!("{}/address/{}", gateway_url, receiver_bech32);
    let balance_resp = client
        .get(&account_url)
        .send()
        .await
        .expect("Failed to get balance");
    let balance_json: serde_json::Value = balance_resp.json().await.unwrap();
    let balance_str = balance_json["data"]["account"]["balance"].as_str().unwrap();

    assert_eq!(
        balance_str, "1000000000000000000",
        "Balance should be 1 EGLD (no double spend)"
    );
}
