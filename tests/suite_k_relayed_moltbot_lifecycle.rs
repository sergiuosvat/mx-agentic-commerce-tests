use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use serde_json::{json, Value};
use std::fs;
use std::process::Stdio;
use tokio::time::{sleep, Duration};

mod common;
use common::{
    address_to_bech32, create_pem_file, fund_address_on_simulator, generate_blocks_on_simulator,
    generate_random_private_key, IdentityRegistryInteractor,
};

const RELAYER_PORT: u16 = 3003;
const RELAYER_URL: &str = "http://localhost:3003";
const FACILITATOR_PORT: u16 = 3005;
const FACILITATOR_URL: &str = "http://localhost:3005";

/// Suite K: Full Moltbot Lifecycle via Relayed Transactions
///
/// End-to-end test combining:
///   1. Agent registration via openclaw-relayer (unfunded bot)
///   2. x402 payment settlement via facilitator with Relayed V3
///
/// This tests the COMBINED flow: bot registers via relayer,
/// then a buyer pays via relayed settle.
#[tokio::test]
async fn test_relayed_moltbot_full_lifecycle() {
    let mut pm = ProcessManager::new();

    // 1. Infrastructure
    let port = pm.start_chain_simulator().unwrap(); // .expect("Failed to start Sim");
    let gateway_url = format!("http://localhost:{}", port);
    sleep(Duration::from_secs(2)).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);

    let chain_id = common::get_simulator_chain_id(&gateway_url).await;
    println!("Simulator ChainID: {}", chain_id);

    let admin = interactor.register_wallet(test_wallets::alice()).await;

    // Top up admin with 100,000 EGLD (chain sim initial balance is only ~10 EGLD)
    let admin_bech32 = address_to_bech32(&admin);
    fund_address_on_simulator(&admin_bech32, "100000000000000000000000", &gateway_url).await;
    println!("Admin topped up with 100,000 EGLD");

    // 2. Deploy & Setup
    let registry = IdentityRegistryInteractor::init(&mut interactor, admin.clone()).await;
    let registry_addr = address_to_bech32(registry.address());

    registry
        .issue_token(&mut interactor, "AgentNFT", "AGENTNFT")
        .await;
    generate_blocks_on_simulator(20, &gateway_url).await;
    sleep(Duration::from_millis(500)).await;

    // Temp directories
    let project_root = std::env::current_dir().unwrap();
    let temp_dir = project_root.join("tests").join("temp_suite_k");
    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir).unwrap();
    }
    fs::create_dir_all(&temp_dir).unwrap();

    // 3. Setup Relayer Wallets (shared by both services)
    let relayer_wallets_dir = temp_dir.join("relayer_wallets");
    fs::create_dir_all(&relayer_wallets_dir).unwrap();

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

    // Ensure cross-shard EGLD transfers settle (30 wallets across 3 shards)
    generate_blocks_on_simulator(30, &gateway_url).await;

    // 4. Start OpenClaw Relayer
    let relayer_env = vec![
        ("NETWORK_PROVIDER", gateway_url.as_str()),
        ("IDENTITY_REGISTRY_ADDRESS", registry_addr.as_str()),
        ("RELAYER_WALLETS_DIR", relayer_wallets_dir.to_str().unwrap()),
        ("PORT", "3003"),
        ("CHAIN_ID", chain_id.as_str()),
        ("IS_TEST_ENV", "true"),
        ("SKIP_SIMULATION", "false"),
    ];

    pm.start_node_service(
        "Relayer",
        "../x402_integration/multiversx-openclaw-relayer",
        "dist/index.js",
        relayer_env,
        RELAYER_PORT,
    )
    .expect("Failed to start Relayer");

    // 5. Start Facilitator
    let store_path = temp_dir.join("facilitator.db");
    let facilitator_env = vec![
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
    ];

    pm.start_node_service(
        "Facilitator",
        "../x402_integration/x402_facilitator",
        "dist/index.js",
        facilitator_env,
        FACILITATOR_PORT,
    )
    .expect("Failed to start Facilitator");
    sleep(Duration::from_secs(2)).await;

    // ────────────────────────────────────
    // PHASE A: Bot Registration via Relayer
    // ────────────────────────────────────
    println!("\n═══ PHASE A: Moltbot Registration via Relayer ═══");

    let bot_pk = generate_random_private_key();
    let bot_wallet = Wallet::from_private_key(&bot_pk).unwrap();
    let bot_addr = bot_wallet.to_address().to_bech32("erd").to_string();
    println!("Bot Address (UNFUNDED): {}", bot_addr);

    let bot_pem = temp_dir.join("bot.pem");
    create_pem_file(bot_pem.to_str().unwrap(), &bot_pk, &bot_addr);

    let reg_output = std::process::Command::new("npm")
        .arg("run")
        .arg("register")
        .current_dir("../moltbot-starter-kit")
        .env("MULTIVERSX_PRIVATE_KEY", bot_pem.to_str().unwrap())
        .env("MULTIVERSX_API_URL", &gateway_url)
        .env("IDENTITY_REGISTRY_ADDRESS", &registry_addr)
        .env("CHAIN_ID", &chain_id)
        .env("MULTIVERSX_CHAIN_ID", &chain_id)
        .env("MULTIVERSX_RELAYER_URL", RELAYER_URL)
        .env("FORCE_RELAYER", "true")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("Failed to run register");

    let reg_stdout = String::from_utf8_lossy(&reg_output.stdout);
    println!("Registration stdout: {}", reg_stdout);
    assert!(
        reg_output.status.success(),
        "Registration failed: {}",
        String::from_utf8_lossy(&reg_output.stderr)
    );
    assert!(
        reg_stdout.contains("Relayed Transaction Sent"),
        "Should use relay"
    );
    println!("✅ Phase A: Bot registered via Relayer");

    generate_blocks_on_simulator(30, &gateway_url).await;
    sleep(Duration::from_secs(1)).await;

    // ────────────────────────────────────
    // PHASE B: Payment via Facilitator Relayed V3
    // ────────────────────────────────────
    println!("\n═══ PHASE B: x402 Payment via Facilitator Relayed V3 ═══");

    // Create a funded buyer
    let buyer_pk = generate_random_private_key();
    let buyer_wallet = Wallet::from_private_key(&buyer_pk).unwrap();
    let buyer_addr = address_to_bech32(&buyer_wallet.to_address());
    let buyer_sc_addr = Address::from_slice(buyer_wallet.to_address().as_bytes());

    interactor
        .tx()
        .from(&admin)
        .to(&buyer_sc_addr)
        .egld(5_000_000_000_000_000_000u64)
        .run()
        .await;

    let buyer_pem = temp_dir.join("buyer.pem");
    create_pem_file(buyer_pem.to_str().unwrap(), &buyer_pk, &buyer_addr);
    let buyer_pem_abs = fs::canonicalize(&buyer_pem).expect("Failed to canonicalize");

    // Get relayer address for buyer's shard from facilitator
    let client = reqwest::Client::new();
    let relayer_res = client
        .get(format!(
            "{}/relayer/address/{}",
            FACILITATOR_URL, buyer_addr
        ))
        .send()
        .await
        .expect("Failed to get relayer address");

    let relayer_body: Value = relayer_res.json().await.unwrap();
    let relayer_bech32 = relayer_body["relayerAddress"]
        .as_str()
        .expect("No relayerAddress");
    println!("Relayer for buyer: {}", relayer_bech32);

    // Sign payment
    let buyer_nonce = interactor.get_account(&buyer_sc_addr).await.nonce;
    let payment = "1000000000000000000"; // 1 EGLD

    let sign_out = std::process::Command::new("npx")
        .arg("ts-node")
        .arg("scripts/sign_x402_relayed.ts")
        .arg(buyer_pem_abs.to_str().unwrap())
        .arg(&bot_addr)
        .arg(payment)
        .arg(buyer_nonce.to_string())
        .arg(&chain_id)
        .arg(relayer_bech32)
        .current_dir("../moltbot-starter-kit")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("Failed to sign");

    assert!(sign_out.status.success(), "Signing failed");
    let payload_str = String::from_utf8_lossy(&sign_out.stdout);
    let payload: Value =
        serde_json::from_str(payload_str.lines().last().unwrap()).expect("Invalid JSON");

    // Settle
    let settle_req = json!({
        "scheme": "exact",
        "payload": payload,
        "requirements": {
            "payTo": bot_addr,
            "amount": payment,
            "asset": "EGLD",
            "network": format!("multiversx:{}", chain_id),
        }
    });

    let res = client
        .post(format!("{}/settle", FACILITATOR_URL))
        .json(&settle_req)
        .send()
        .await
        .expect("Settle request failed");

    let status = res.status();
    let body = res.text().await.unwrap();
    println!("Settle ({}): {}", status, body);
    assert!(status.is_success(), "Settle failed: {}", body);
    println!("✅ Phase B: Payment settled via Relayed V3");

    // ────────────────────────────────────
    // VERIFICATION
    // ────────────────────────────────────
    generate_blocks_on_simulator(10, &gateway_url).await;
    sleep(Duration::from_secs(2)).await;

    let events_res = client
        .get(format!("{}/events?unread=true", FACILITATOR_URL))
        .send()
        .await
        .expect("Failed to get events");

    let events: Value = events_res.json().await.unwrap();
    let events_arr = events.as_array().expect("Events should be array");
    assert!(!events_arr.is_empty(), "Should have settlement events");
    println!("✅ Events found: {}", events_arr.len());

    // Cleanup
    let _ = fs::remove_dir_all(&temp_dir);
    println!("✅ Suite K Complete: Full Moltbot lifecycle via Relayed V3 passed.");
}
