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
    generate_random_private_key, IdentityRegistryInteractor,
};

const FACILITATOR_PORT: u16 = 3005;
const SIM_URL: &str = "http://localhost:8085";
const FACILITATOR_URL: &str = "http://localhost:3005";

#[tokio::test]
async fn test_multi_agent_payment_delegation() {
    let mut pm = ProcessManager::new();

    let port = pm.start_chain_simulator()
        .expect("Failed to start Sim");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    // 2. Setup Interactor & Wallets
    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);

    let chain_id = common::get_simulator_chain_id(&gateway_url).await;
    println!("Simulator ChainID: {}", chain_id);

    let admin = interactor.register_wallet(test_wallets::alice()).await;

    // Top up admin with 100,000 EGLD (chain sim initial balance is only ~10 EGLD)
    let admin_bech32 = address_to_bech32(&admin);
    fund_address_on_simulator(&admin_bech32, "100000000000000000000000", &gateway_url).await;
    println!("Admin topped up with 100,000 EGLD");

    let alice_pk = generate_random_private_key();
    let alice_wallet = Wallet::from_private_key(&alice_pk).unwrap();
    let alice_addr = address_to_bech32(&alice_wallet.to_address());
    let alice_sc_addr = Address::from_slice(alice_wallet.to_address().as_bytes());

    let bob_pk = generate_random_private_key();
    let bob_wallet = Wallet::from_private_key(&bob_pk).unwrap();
    let bob_addr = address_to_bech32(&bob_wallet.to_address());
    let bob_sc_addr = Address::from_slice(bob_wallet.to_address().as_bytes());

    println!("Alice (Buyer): {}", alice_addr);
    println!("Bob (Seller): {}", bob_addr);

    // Fund Alice & Bob using Interactor
    interactor
        .tx()
        .from(&admin)
        .to(&bob_sc_addr)
        .egld(1_000_000_000_000_000_000u64)
        .run()
        .await; // 1 EGLD
    interactor
        .tx()
        .from(&admin)
        .to(&alice_sc_addr)
        .egld(5_000_000_000_000_000_000u64)
        .run()
        .await; // 5 EGLD

    // Wait for funding
    // Funding is sync in Interactor.run().await so we can proceed.
    generate_blocks_on_simulator(10, &gateway_url).await;
    sleep(Duration::from_millis(500)).await;

    // Save PEM for signing scripts
    let project_root = std::env::current_dir().unwrap();
    let temp_dir = project_root.join("tests").join("temp_multi_agent");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).unwrap();
    }
    std::fs::create_dir_all(&temp_dir).unwrap();

    let alice_pem = temp_dir.join("alice.pem");
    create_pem_file(alice_pem.to_str().unwrap(), &alice_pk, &alice_addr);
    let alice_pem_abs = fs::canonicalize(&alice_pem).expect("Failed to canonicalize alice pem");

    // 3. Deploy Registry & Register Bob
    let registry = IdentityRegistryInteractor::init(&mut interactor, admin.clone()).await;
    let registry_addr = address_to_bech32(registry.address());

    let sim_url = SIM_URL;

    // 4. Start Facilitator
    let env = vec![
        ("PORT", "3005"),
        ("NETWORK_PROVIDER", sim_url), // CRITICAL FIX: Point to Simulator
        ("MULTIVERSX_API_URL", sim_url),
        ("MX_PROXY_URL", sim_url),
        (
            "PRIVATE_KEY",
            "e253a571ca153dc2aee845819f74bcc9773b0586edead15a94d728462b34ef8c",
        ), // Random
        ("REGISTRY_ADDRESS", &registry_addr),
        ("CHAIN_ID", &chain_id), // Use dynamic ChainID
        (
            "MNEMONIC",
            "moral volcano peasant pass circle pen over picture flat shop clap goat",
        ), // Dummy
        ("STORE_PATH", "tests/temp_multi_agent/facilitator.db"),
        ("STORAGE_TYPE", "json"),
        ("SKIP_SIMULATION", "false"),
    ];

    pm.start_node_service(
        "Facilitator",
        "../x402_integration/x402_facilitator",
        "dist/index.js",
        env,
        FACILITATOR_PORT,
    )
    .expect("Failed to start Facilitator");
    wait_for_simulator_ready(&gateway_url).await;

    // 5. Execute Payment Flow (Alice -> Bob)
    let payment_value = "1000000000000000000"; // 1 EGLD

    // Get Alice's Nonce
    let account = interactor.get_account(&alice_sc_addr).await;
    let nonce = account.nonce;
    println!("Alice Nonce: {}", nonce);

    // Sign X402 Payment
    println!("Signing X402 Payload...");
    let status = std::process::Command::new("npx")
        .arg("ts-node")
        .arg("scripts/sign_x402.ts")
        .arg(alice_pem_abs.to_str().unwrap())
        .arg(&bob_addr)
        .arg(payment_value)
        .arg(nonce.to_string())
        .arg(&chain_id) // Dynamic ChainID
        .arg("init_job@1234") // Data
        .current_dir("../moltbot-starter-kit")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .expect("Failed to run signing script");

    if !status.status.success() {
        panic!("Signing X402 failed");
    }

    let payload_json: Value =
        serde_json::from_slice(&status.stdout).expect("Invalid JSON from signer");
    println!("Payload: {}", payload_json);

    // 6. Call /settle
    let client = reqwest::Client::new();
    let settle_req = json!({
        "scheme": "exact",
        "payload": payload_json,
        "requirements": {
            "payTo": bob_addr,
            "amount": payment_value,
            "asset": "EGLD",
            "network": "D"
        }
    });

    println!("Sending Settle Request...");
    let res = client
        .post(format!("{}/settle", FACILITATOR_URL))
        .json(&settle_req)
        .send()
        .await
        .expect("Failed to send settle request");

    let status = res.status();
    let body = res.text().await.unwrap();
    println!("Settle Resp ({}) : {}", status, body);

    assert!(status.is_success(), "Settle failed");
    assert!(body.contains("txHash"), "Response should contain txHash");

    // 7. Verify Event
    println!("Polling Events...");
    sleep(Duration::from_secs(5)).await; // Wait for processing

    let events_res = client
        .get(format!("{}/events?unread=true", FACILITATOR_URL))
        .send()
        .await
        .expect("Failed to poll events");

    let events: Value = events_res.json().await.unwrap();
    let events_arr = events.as_array().expect("Events should be array");

    println!("Events Found: {:?}", events_arr);

    // Find our payment
    let found = events_arr.iter().any(|e| {
        let meta = e["meta"].as_object().unwrap();
        meta["sender"].as_str().unwrap() == alice_addr
    });

    assert!(!events_arr.is_empty(), "Should have events");
    assert!(found, "Should find event from Alice");

    let event = &events_arr[0];
    assert_eq!(event["amount"], payment_value);

    // Clean up
    let _ = std::fs::remove_dir_all(&temp_dir);
}
