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
const FACILITATOR_URL: &str = "http://localhost:3005";

/// Suite J: Facilitator settle endpoint via Relayed V3
///
/// Tests the fixed `sendRelayedV3` in settler.ts — ensures the facilitator
/// correctly broadcasts relayed transactions with pre-broadcast simulation.
///
/// Flow:
///   1. Fund wallets + relayer wallets FIRST (cross-shard settlement)
///   2. Deploy identity-registry, register Bob as agent
///   3. Start facilitator with relayer wallets
///   4. Alice signs x402 payment with relayer address (sign_x402_relayed.ts)
///   5. POST /settle — facilitator runs sendRelayedV3
///   6. Verify on-chain: Bob received funds + events emitted
#[tokio::test]
async fn test_relayed_facilitator_settle() {
    let mut pm = ProcessManager::new();

    // 1. Start Chain Simulator
    let port = pm.start_chain_simulator().unwrap(); // .expect("Failed to start Sim");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);

    let chain_id = common::get_simulator_chain_id(&gateway_url).await;
    println!("Simulator ChainID: {}", chain_id);

    let admin = interactor.register_wallet(test_wallets::alice()).await;

    // Top up admin with 100,000 EGLD (chain sim initial balance is only ~10 EGLD)
    let admin_bech32 = address_to_bech32(&admin);
    fund_address_on_simulator(&admin_bech32, "100000000000000000000000", &gateway_url).await; // 100,000 EGLD
    println!("Admin topped up with 100,000 EGLD");

    // ────────────────────────────────────────────
    // 2. FUND ALL WALLETS FIRST (before registry deployment)
    // ────────────────────────────────────────────

    // Setup Bob (Seller)
    let bob_pk = generate_random_private_key();
    let bob_wallet = Wallet::from_private_key(&bob_pk).unwrap();
    let bob_addr = address_to_bech32(&bob_wallet.to_address());
    let bob_sc_addr = Address::from_slice(bob_wallet.to_address().as_bytes());

    interactor
        .tx()
        .from(&admin)
        .to(&bob_sc_addr)
        .egld(1_000_000_000_000_000_000u64)
        .run()
        .await;

    // Setup Alice (Buyer)
    let alice_pk = generate_random_private_key();
    let alice_wallet = Wallet::from_private_key(&alice_pk).unwrap();
    let alice_addr = address_to_bech32(&alice_wallet.to_address());
    let alice_sc_addr = Address::from_slice(alice_wallet.to_address().as_bytes());

    interactor
        .tx()
        .from(&admin)
        .to(&alice_sc_addr)
        .egld(5_000_000_000_000_000_000u64)
        .run()
        .await; // 5 EGLD

    // Setup temp dir and relayer wallets
    let project_root = std::env::current_dir().unwrap();
    let temp_dir = project_root.join("tests").join("temp_suite_j");
    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir).unwrap();
    }
    fs::create_dir_all(&temp_dir).unwrap();

    let alice_pem = temp_dir.join("alice.pem");
    create_pem_file(alice_pem.to_str().unwrap(), &alice_pk, &alice_addr);
    let alice_pem_abs = fs::canonicalize(&alice_pem).expect("Failed to canonicalize");

    // Fund 30 relayer wallets (1 EGLD each)
    let relayer_wallets_dir = temp_dir.join("relayer_wallets");
    fs::create_dir_all(&relayer_wallets_dir).unwrap();

    println!("Generating 30 Relayer Wallets...");
    for i in 0..30 {
        let relayer_pk = generate_random_private_key();
        let relayer_wallet = Wallet::from_private_key(&relayer_pk).unwrap();
        let relayer_addr_obj = relayer_wallet.to_address();
        let relayer_addr = relayer_addr_obj.to_bech32("erd").to_string();
        let relayer_sc_addr = Address::from_slice(relayer_addr_obj.as_bytes());

        interactor
            .tx()
            .from(&admin)
            .to(&relayer_sc_addr)
            .egld(1_000_000_000_000_000_000u64)
            .run()
            .await;

        let relayer_pem = relayer_wallets_dir.join(format!("relayer_{}.pem", i));
        create_pem_file(relayer_pem.to_str().unwrap(), &relayer_pk, &relayer_addr);
    }
    println!("All relayer wallets funded.");

    // Ensure cross-shard EGLD transfers settle
    generate_blocks_on_simulator(30, &gateway_url).await;
    sleep(Duration::from_millis(500)).await;

    // ────────────────────────────────────────────
    // 3. DEPLOY REGISTRY + REGISTER BOB
    //    (borrows interactor mutably — do after wallet funding)
    // ────────────────────────────────────────────
    let registry_addr;
    {
        let registry = IdentityRegistryInteractor::init(&mut interactor, admin.clone()).await;
        registry_addr = address_to_bech32(registry.address());
        println!("Registry: {}", registry_addr);

        registry
            .issue_token(&mut interactor, "AgentNFT", "AGENTNFT")
            .await;
        generate_blocks_on_simulator(20, &gateway_url).await;
        sleep(Duration::from_secs(1)).await;

        registry
            .register_agent(
                &mut interactor,
                "BobAgent",
                "https://bob.example.com",
                vec![],
            )
            .await;
    }
    // registry dropped — interactor borrow released

    // Final block generation to ensure ALL cross-shard EGLD transfers are settled
    generate_blocks_on_simulator(30, &gateway_url).await;
    sleep(Duration::from_millis(500)).await;

    // ────────────────────────────────────────────
    // 4. START FACILITATOR WITH RELAYER WALLETS
    // ────────────────────────────────────────────
    let store_path = temp_dir.join("facilitator.db");
    let env = vec![
        ("PORT", "3005"),
        ("NETWORK_PROVIDER", gateway_url.as_str()),
        ("MULTIVERSX_API_URL", gateway_url.as_str()),
        ("MX_PROXY_URL", gateway_url.as_str()),
        ("REGISTRY_ADDRESS", registry_addr.as_str()),
        ("CHAIN_ID", chain_id.as_str()),
        ("RELAYER_WALLETS_DIR", relayer_wallets_dir.to_str().unwrap()),
        ("STORAGE_TYPE", "json"),
        ("STORE_PATH", store_path.to_str().unwrap()),
        ("SKIP_SIMULATION", "false"),
        ("LOG_LEVEL", "debug"),
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

    // Get relayer address for Alice's shard
    let client = reqwest::Client::new();
    let relayer_addr_res = client
        .get(format!(
            "{}/relayer/address/{}",
            FACILITATOR_URL, alice_addr
        ))
        .send()
        .await
        .expect("Failed to get relayer address from facilitator");

    let relayer_addr_body: Value = relayer_addr_res.json().await.unwrap();
    let relayer_address_bech32 = relayer_addr_body["relayerAddress"]
        .as_str()
        .expect("relayerAddress not in response");
    println!("Relayer for Alice shard: {}", relayer_address_bech32);

    // Verify relayer has funds
    let (_, data) = bech32::decode(relayer_address_bech32).expect("Invalid bech32");
    let relayer_sc_addr_chk = Address::from_slice(&data);
    let relayer_acc = interactor.get_account(&relayer_sc_addr_chk).await;
    let bal_u128 = relayer_acc.balance.parse::<u128>().unwrap_or(0);
    assert!(
        bal_u128 > 0,
        "Relayer has 0 balance! Address: {}",
        relayer_address_bech32
    );
    println!("Relayer balance: {} wei ✓", bal_u128);

    // ────────────────────────────────────────────
    // 5. SIGN X402 PAYMENT WITH RELAYER
    // ────────────────────────────────────────────
    let alice_nonce = interactor.get_account(&alice_sc_addr).await.nonce;
    let payment_value = "1000000000000000000"; // 1 EGLD

    let sign_status = std::process::Command::new("npx")
        .arg("ts-node")
        .arg("scripts/sign_x402_relayed.ts")
        .arg(alice_pem_abs.to_str().unwrap())
        .arg(&bob_addr)
        .arg(payment_value)
        .arg(alice_nonce.to_string())
        .arg(&chain_id)
        .arg(relayer_address_bech32)
        .arg("init_job@1234")
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
    let payload_json: Value =
        serde_json::from_str(payload_str).expect("Invalid JSON from sign_x402_relayed.ts");
    println!("Signed Payload: {}", payload_json);

    // Verify the payload has relayer field
    assert!(
        payload_json["relayer"].is_string(),
        "Payload must include relayer"
    );
    assert_eq!(payload_json["version"], 2, "Payload version must be 2");

    // ────────────────────────────────────────────
    // 6. CALL /settle WITH THE RELAYED PAYLOAD
    // ────────────────────────────────────────────
    let settle_req = json!({
        "scheme": "exact",
        "payload": payload_json,
        "requirements": {
            "payTo": bob_addr,
            "amount": payment_value,
            "asset": "EGLD",
            "network": format!("multiversx:{}", chain_id),
        }
    });

    println!("Sending /settle request...");
    let res = client
        .post(format!("{}/settle", FACILITATOR_URL))
        .json(&settle_req)
        .send()
        .await
        .expect("Failed to send settle");

    let status = res.status();
    let body = res.text().await.unwrap();
    println!("Settle Response ({}): {}", status, body);
    assert!(status.is_success(), "Settle failed: {}", body);
    assert!(body.contains("txHash"), "Should contain txHash");
    println!("✅ Relayed Settle: SUCCESS");

    // ────────────────────────────────────────────
    // 7. VERIFY EVENTS
    // ────────────────────────────────────────────
    generate_blocks_on_simulator(10, &gateway_url).await;
    wait_for_simulator_ready(&gateway_url).await;

    let events_res = client
        .get(format!("{}/events?unread=true", FACILITATOR_URL))
        .send()
        .await
        .expect("Failed to poll events");

    let events: Value = events_res.json().await.unwrap();
    let events_arr = events.as_array().expect("Events should be array");
    println!("Events: {:?}", events_arr);

    assert!(!events_arr.is_empty(), "Should have events");

    let found = events_arr
        .iter()
        .any(|e| e["meta"]["sender"].as_str() == Some(&alice_addr));
    assert!(found, "Should find event from Alice");

    // Cleanup
    let _ = fs::remove_dir_all(&temp_dir);
    println!("✅ Suite J Complete: Facilitator Relayed Settle passed.");
}
