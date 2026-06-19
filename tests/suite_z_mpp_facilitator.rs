use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use serde_json::{json, Value};
use std::fs;
use std::process::Stdio;
use tokio::time::{sleep, Duration};

mod common;
use common::{
    wait_for_simulator_ready,
    address_to_bech32, create_pem_file, fund_address_on_simulator, generate_blocks_on_simulator,
    generate_random_private_key,
};

const MPP_PORT: u16 = 3006;
const MPP_URL: &str = "http://localhost:3006";

/// Suite Z: MPP Facilitator Relayed V3 tests
///
/// Ensures the mpp-facilitator-mvx correctly broadcasts relayed transactions 
/// via the `/submit_relayed_v3` endpoint.
///
/// Flow:
///   1. Start Chain Simulator
///   2. Fund wallets
///   3. Start mpp-facilitator-mvx
///   4. Sign x402 payment
///   5. POST /submit_relayed_v3
///   6. Verify on-chain payment
#[tokio::test]
async fn test_relayed_mpp_facilitator() {
    let mut pm = ProcessManager::new();

    // 1. Start Chain Simulator
    let port = pm.start_chain_simulator().unwrap(); // .expect("Failed to start Sim");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);

    let chain_id = common::get_simulator_chain_id(&gateway_url).await;
    println!("Simulator ChainID: {}", chain_id);

    let admin = interactor.register_wallet(test_wallets::alice()).await;
    let admin_bech32 = address_to_bech32(&admin);
    fund_address_on_simulator(&admin_bech32, "100000000000000000000000", &gateway_url).await; 

    // 2. Setup Alice's wallet (which is used as Relayer by mpp-facilitator-mvx defaults)
    let project_root = std::env::current_dir().unwrap();
    let alice_pem_path = project_root.join("alice.pem");
    let alice_wallet = Wallet::from_pem_file(alice_pem_path.to_str().unwrap()).unwrap();
    let relayer_sc_addr = Address::from_slice(alice_wallet.to_address().as_bytes());
    let relayer_bech32 = address_to_bech32(&relayer_sc_addr);

    // Fund relayer
    interactor
        .tx()
        .from(&admin)
        .to(&relayer_sc_addr)
        .egld(1_000_000_000_000_000_000u64)
        .run()
        .await;

    // ────────────────────────────────────────────
    // 3. Setup Wallets (Sender and Receiver)
    // ────────────────────────────────────────────
    
    // Both Sender and Relayer must be in the same shard for Relayed V3!
    let relayer_pk_last_byte = alice_wallet.to_address().as_bytes()[31];
    let (bob_pk, bob_wallet) = loop {
        let bob_pk = generate_random_private_key();
        let bob_wallet = Wallet::from_private_key(&bob_pk).unwrap();
        if bob_wallet.to_address().as_bytes()[31] == relayer_pk_last_byte {
            break (bob_pk, bob_wallet);
        }
    };
    let bob_addr = address_to_bech32(&bob_wallet.to_address());
    let bob_sc_addr = Address::from_slice(bob_wallet.to_address().as_bytes());
    
    fund_address_on_simulator(&bob_addr, "10000000000000000000", &gateway_url).await; // 10 EGLD

    // Receiver (can be anywhere, let's use Charlie)
    let charlie_pk = generate_random_private_key();
    let charlie_wallet = Wallet::from_private_key(&charlie_pk).unwrap();
    let charlie_addr = address_to_bech32(&charlie_wallet.to_address());
    let charlie_sc_addr = Address::from_slice(charlie_wallet.to_address().as_bytes());

    generate_blocks_on_simulator(5, &gateway_url).await;
    sleep(Duration::from_millis(500)).await;

    let temp_dir = project_root.join("tests").join("temp_suite_z");
    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir).unwrap();
    }
    fs::create_dir_all(&temp_dir).unwrap();

    let bob_pem = temp_dir.join("bob.pem");
    create_pem_file(bob_pem.to_str().unwrap(), &bob_pk, &bob_addr);
    let bob_pem_abs = fs::canonicalize(&bob_pem).expect("Failed to canonicalize");

    // 3. Start MPP Facilitator
    let env = vec![
        ("PORT", "3006"),
        ("NETWORK_PROVIDER", gateway_url.as_str()),
        ("MPP_SECRET_KEY", "test-secret-key-12345678901234567890123456789012"),
    ];

    pm.start_node_service(
        "MppFacilitator",
        "../mpp-facilitator-mvx",
        "dist/main.js",
        env,
        MPP_PORT,
    )
    .expect("Failed to start MPP Facilitator");
    sleep(Duration::from_secs(3)).await;

    // Verify relayer address via endpoint
    let client = reqwest::Client::new();
    let relayer_addr_res = client
        .get(format!("{}/relayer_address", MPP_URL))
        .send()
        .await
        .expect("Failed to get relayer address");

    let relayer_addr_body: Value = relayer_addr_res.json().await.unwrap();
    let returned_relayer_addr = relayer_addr_body["address"].as_str().unwrap();
    assert_eq!(returned_relayer_addr, relayer_bech32);
    
    // Check initial charlie balance
    let charlie_acc_initial = interactor.get_account(&charlie_sc_addr).await;
    let charlie_bal_initial_u128 = charlie_acc_initial.balance.parse::<u128>().unwrap_or(0);

    // 4. Sign X402 Relayed with Bob's PEM
    let bob_nonce = interactor.get_account(&bob_sc_addr).await.nonce;
    let payment_value = "10000000000000000"; // 0.01 EGLD

    let sign_status = std::process::Command::new("npx")
        .arg("ts-node")
        .arg("scripts/sign_x402_relayed.ts")
        .arg(bob_pem_abs.to_str().unwrap())
        .arg(&charlie_addr)
        .arg(payment_value)
        .arg(bob_nonce.to_string())
        .arg(&chain_id)
        .arg(&relayer_bech32)
        .current_dir("../moltbot-starter-kit")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("Failed to run sign_x402_relayed.ts");

    if !sign_status.status.success() {
        let stderr = String::from_utf8_lossy(&sign_status.stderr);
        panic!("Signing failed: {}", stderr);
    }

    let sign_stdout = String::from_utf8_lossy(&sign_status.stdout);
    let payload_str = sign_stdout.lines().last().unwrap();
    let payload_json: Value = serde_json::from_str(payload_str).expect("Invalid JSON");
    
    // 5. Submit to MPP Facilitator
    // Generating blocks to advance epoch because Relayed V3 transactions might be disabled on Epoch 0 in chain simulator
    println!("Advancing epoch to enable Relayed V3 on chain simulator...");
    common::generate_blocks_on_simulator(50, &gateway_url).await;
    wait_for_simulator_ready(&gateway_url).await;

    // ────────────────────────────────────────────
    // 5. CALL /submit_relayed_v3 WITH THE RELAYED PAYLOAD
    // ────────────────────────────────────────────
    let submit_req = json!(payload_json);
    let res = client
        .post(format!("{}/submit_relayed_v3", MPP_URL))
        .json(&submit_req)
        .send()
        .await
        .expect("Failed to submit relayed v3");

    let status = res.status();
    let body = res.text().await.unwrap();
    println!("Submit Response ({}): {}", status, body);
    assert!(status.is_success(), "Submit failed: {}", body);
    assert!(body.contains("txHash"), "Should contain txHash");
    
    // Wait for block to produce
    generate_blocks_on_simulator(5, &gateway_url).await;
    wait_for_simulator_ready(&gateway_url).await;

    // 6. Verify Charlie received EGLD
    let charlie_acc_final = interactor.get_account(&charlie_sc_addr).await;
    let charlie_bal_final_u128 = charlie_acc_final.balance.parse::<u128>().unwrap_or(0);
    
    assert_eq!(
        charlie_bal_final_u128,
        charlie_bal_initial_u128 + 10000000000000000,
        "Charlie did not receive the funds"
    );

    // Cleanup
    let _ = fs::remove_dir_all(&temp_dir);
    println!("✅ MPP Facilitator Relayed V3 passed!");
}
