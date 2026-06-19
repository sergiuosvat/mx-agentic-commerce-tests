use multiversx_sc_snippets::imports::*;
use reqwest::Client;
use serde_json::json;
use std::process::Command;

use crate::common::{
    address_to_bech32, generate_random_private_key, get_simulator_chain_id,
    start_facilitator, IdentityRegistryInteractor, TestEnv,
};

#[tokio::test]
async fn test_rejection_cases() {
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

    // 2. Create Valid Signed Tx
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
    let original_payload: serde_json::Value =
        serde_json::from_str(&signed_tx_json).expect("Failed to parse signed tx JSON");

    let client = Client::new();

    // --- Case 1: Invalid Signature ---
    println!("Testing Invalid Signature...");
    let mut payload = original_payload.clone();
    let sig = payload["signature"].as_str().unwrap();
    // Tamper signature
    let tampered_sig = format!("{}a", &sig[0..sig.len() - 1]);
    payload["signature"] = json!(tampered_sig);

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

    let resp = client
        .post(format!("{facilitator_url}/verify"))
        .json(&body)
        .send()
        .await
        .expect("Failed to send request");

    let resp_json: serde_json::Value = resp.json().await.unwrap();
    println!("Invalid Sig Response: {:?}", resp_json);

    let is_invalid = if let Some(err) = resp_json.get("error") {
        !err.is_null()
    } else if let Some(valid) = resp_json.get("isValid") {
        !valid.as_bool().unwrap_or(true)
    } else {
        false
    };
    assert!(is_invalid, "Should be invalid due to signature");

    // --- Case 2: Amount Mismatch (Payload < Requirement) ---
    println!("Testing Amount Mismatch...");
    // Re-sign a tx with LOWER amount
    let output_low = Command::new("npx")
        .arg("ts-node")
        .arg("scripts/sign_tx.ts")
        .arg("--sender-pk")
        .arg(&sender_pk_hex)
        .arg("--receiver")
        .arg(&receiver_bech32)
        .arg("--value")
        .arg("500000000000000000") // 0.5 EGLD
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

    let signed_tx_low_json = String::from_utf8_lossy(&output_low.stdout);
    let payload_low: serde_json::Value = serde_json::from_str(&signed_tx_low_json).unwrap();

    let requirements_high = json!({
        "payTo": receiver_bech32,
        "amount": value_str, // Require 1 EGLD
        "asset": "EGLD",
        "network": format!("multiversx:{}", chain_id)
    });

    let body_low = json!({
        "scheme": "exact",
        "payload": payload_low,
        "requirements": requirements_high
    });

    let resp_low = client
        .post(format!("{facilitator_url}/verify"))
        .json(&body_low)
        .send()
        .await
        .unwrap();

    let resp_low_json: serde_json::Value = resp_low.json().await.unwrap();
    println!("Amount Mismatch Response: {:?}", resp_low_json);

    let is_invalid_low = if let Some(err) = resp_low_json.get("error") {
        !err.is_null()
    } else if let Some(valid) = resp_low_json.get("isValid") {
        !valid.as_bool().unwrap_or(true)
    } else {
        false
    };
    assert!(is_invalid_low, "Should be invalid due to amount too low");

    // --- Case 3: Receiver Mismatch ---
    println!("Testing Receiver Mismatch...");
    // Use original valid payload (sends to receiver_bech32)
    // But requirement asks to pay SOMEONE_ELSE
    let someone_else = "erd1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq6gq4hu";

    let requirements_diff_receiver = json!({
        "payTo": someone_else,
        "amount": value_str,
        "asset": "EGLD",
        "network": format!("multiversx:{}", chain_id)
    });

    let body_diff = json!({
        "scheme": "exact",
        "payload": original_payload,
        "requirements": requirements_diff_receiver
    });

    let resp_diff = client
        .post(format!("{facilitator_url}/verify"))
        .json(&body_diff)
        .send()
        .await
        .unwrap();

    let resp_diff_json: serde_json::Value = resp_diff.json().await.unwrap();
    println!("Receiver Mismatch Response: {:?}", resp_diff_json);

    let is_invalid_diff = if let Some(err) = resp_diff_json.get("error") {
        !err.is_null()
    } else if let Some(valid) = resp_diff_json.get("isValid") {
        !valid.as_bool().unwrap_or(true)
    } else {
        false
    };
    assert!(
        is_invalid_diff,
        "Should be invalid due to receiver mismatch"
    );
}
