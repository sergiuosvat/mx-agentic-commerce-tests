use crate::common::{
    address_to_bech32, fund_address_on_simulator, generate_blocks_on_simulator,
    generate_random_private_key, get_simulator_chain_id, issue_fungible_esdt,
    start_facilitator, IdentityRegistryInteractor, TestEnv, wait_for_simulator_ready,
};
use multiversx_sc_snippets::imports::*;
use std::process::Command;
use tokio::time::{sleep, Duration};

const ESDT_DB_PATH: &str = "./facilitator_esdt.db";

#[tokio::test]
async fn test_settle_esdt() {
    let env = TestEnv::chain_only().await;
    let mut pm = env.pm;
    let gateway_url = env.gateway_url.clone();
    let mut interactor = env.interactor;

    let sender_pk = generate_random_private_key();
    let sender_wallet = Wallet::from_private_key(&sender_pk).unwrap();
    let sender_address = sender_wallet.to_address().to_bech32("erd").to_string();
    let sender_sc_address = interactor.register_wallet(sender_wallet).await;

    let receiver_pk = generate_random_private_key();
    let receiver_wallet = Wallet::from_private_key(&receiver_pk).unwrap();
    let receiver_address = receiver_wallet.to_address().to_bech32("erd").to_string();

    // 1. Fund Sender (needs EGLD for issuance fees + gas)
    println!("Funding Sender: {}", sender_address);
    fund_address_on_simulator(&sender_address, "500000000000000000000", &gateway_url).await; // 500 EGLD

    // Advance past epoch 0 — ESDT system SC is disabled at epoch 0
    // RoundsPerEpoch = 20, so 25 blocks guarantees epoch >= 1
    generate_blocks_on_simulator(25, &gateway_url).await;

    // 2. Issue ESDT
    let token_id = issue_fungible_esdt(
        &mut interactor,
        &sender_sc_address,
        "FacilitatorToken",
        "FACT",
        1_000_000_000u128,
        6,
        &gateway_url,
    )
    .await;
    println!("Issued Token: {}", token_id);

    // 3. Start Facilitator
    let chain_id = get_simulator_chain_id(&gateway_url).await;
    let facilitator_pk = generate_random_private_key();
    let identity = IdentityRegistryInteractor::init(&mut interactor, sender_sc_address.clone()).await;
    let registry_address = address_to_bech32(identity.address());

    let facilitator_url = start_facilitator(
        &mut pm,
        &facilitator_pk,
        &registry_address,
        &gateway_url,
        &chain_id,
        &[
            ("SQLITE_DB_PATH", ESDT_DB_PATH),
            ("SKIP_SIMULATION", "false"),
        ],
    )
    .await;

    let client = reqwest::Client::new();

    // 4. Sign ESDT Transaction

    let esdt_amount = "1000000"; // 1.000000 USDC

    // Use the updated sign_tx.ts with --token and --amount
    let output = Command::new("npx")
        .arg("ts-node")
        .arg("../moltbot-starter-kit/scripts/sign_tx.ts")
        .arg("--sender-pk")
        .arg(&sender_pk)
        .arg("--receiver")
        .arg(&receiver_address)
        .arg("--value")
        .arg("0") // EGLD value is 0 for ESDT transfer
        .arg("--token")
        .arg(&token_id)
        .arg("--amount")
        .arg(esdt_amount)
        .arg("--nonce")
        .arg("1") // Nonce 1 (0 was issuance)
        .arg("--gas-limit")
        .arg("500000") // ESDT transfer needs more gas
        .arg("--gas-price")
        .arg("1000000000")
        .arg("--chain-id")
        .arg(&chain_id)
        .output()
        .expect("Failed to sign transaction");

    if !output.status.success() {
        eprintln!("Sign Tx Error: {}", String::from_utf8_lossy(&output.stderr));
        panic!("Sign Tx failed");
    }

    let json_str = String::from_utf8(output.stdout).unwrap();
    let signed_tx: serde_json::Value = serde_json::from_str(json_str.trim()).unwrap();
    println!("Signed Tx Payload: {}", signed_tx);

    // Prepare x402 envelope (same structure as test_settle_egld)
    let mut payload = signed_tx.clone();
    if payload.get("data").is_none() || payload["data"].is_null() {
        payload["data"] = serde_json::json!("");
    }
    if payload.get("options").is_none() {
        payload["options"] = serde_json::json!(0);
    }

    let requirements = serde_json::json!({
        "payTo": receiver_address,
        "amount": esdt_amount,
        "asset": token_id,
        "network": format!("multiversx:{}", chain_id)
    });

    let body = serde_json::json!({
        "scheme": "exact",
        "payload": payload,
        "requirements": requirements
    });

    // 5. Send /settle Request (matching test_settle_egld pattern — skip /verify)
    let resp = client
        .post(format!("{}/settle", facilitator_url))
        .json(&body)
        .send()
        .await
        .expect("Failed to send request");

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        panic!("Request failed: status={}, body={}", status, text);
    }

    let resp_json: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(resp_json["success"], true);

    // 6. Generate Blocks & Verify
    // Wait for facilitator to broadcast
    wait_for_simulator_ready(&gateway_url).await;
    generate_blocks_on_simulator(5, &gateway_url).await;
    sleep(Duration::from_secs(5)).await;

    // Check Receiver ESDT Balance
    let account_url = format!(
        "{}/address/{}/esdt/{}", gateway_url, receiver_address, token_id
    );
    let balance_resp = client
        .get(&account_url)
        .send()
        .await
        .expect("Failed to get balance");

    if !balance_resp.status().is_success() {
        panic!("Receiver has no token balance (404 likely)");
    }

    let balance_json: serde_json::Value = balance_resp.json().await.unwrap();
    let balance = balance_json["data"]["tokenData"]["balance"]
        .as_str()
        .unwrap();

    assert_eq!(balance, esdt_amount, "Receiver ESDT balance incorrect");
}
