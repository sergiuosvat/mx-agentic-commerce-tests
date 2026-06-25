use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use serde_json::json;
use std::process::Stdio;
use tokio::time::{sleep, Duration};

mod common;
use common::{
    address_to_bech32, create_pem_file, create_temp_pem_file, fund_address_on_simulator,
    generate_blocks_on_simulator, generate_random_private_key, get_simulator_chain_id,
    start_relayer, temp_relayer_wallets_dir, wait_for_simulator_ready, IdentityRegistryInteractor,
};

/// Suite V3: Relayer quota enforcement (HTTP 429) — mirrors `multiversx-openclaw-relayer` Server.test.
#[tokio::test]
async fn test_relayer_quota_exceeded_returns_429() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator().expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{port}");
    wait_for_simulator_ready(&gateway_url).await;
    generate_blocks_on_simulator(5, &gateway_url).await;

    let chain_id = get_simulator_chain_id(&gateway_url).await;
    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);

    let admin = interactor.register_wallet(test_wallets::alice()).await;
    let admin_bech32 = address_to_bech32(&admin);
    fund_address_on_simulator(&admin_bech32, "100000000000000000000", &gateway_url).await;
    generate_blocks_on_simulator(5, &gateway_url).await;

    let registry = IdentityRegistryInteractor::init(&mut interactor, admin.clone()).await;
    let registry_bech32 = address_to_bech32(registry.address());
    registry
        .issue_token(&mut interactor, "AgentNFT", "AGENTNFT")
        .await;
    generate_blocks_on_simulator(25, &gateway_url).await;
    sleep(Duration::from_secs(1)).await;

    let relayer_wallets_dir = std::path::PathBuf::from(temp_relayer_wallets_dir("relayer_v3"));
    std::fs::remove_dir_all(&relayer_wallets_dir).ok();
    std::fs::create_dir_all(&relayer_wallets_dir).unwrap();

    for i in 0..5 {
        let relayer_pk = generate_random_private_key();
        let relayer_wallet = Wallet::from_private_key(&relayer_pk).unwrap();
        let relayer_addr = Address::from_slice(relayer_wallet.to_address().as_bytes());
        interactor
            .tx()
            .from(&admin)
            .to(&relayer_addr)
            .egld(1_000_000_000_000_000_000u64)
            .run()
            .await;
        create_pem_file(
            relayer_wallets_dir
                .join(format!("relayer_{i}.pem"))
                .to_str()
                .unwrap(),
            &relayer_pk,
        );
    }
    generate_blocks_on_simulator(10, &gateway_url).await;

    let agent_pk = generate_random_private_key();
    let agent_wallet = Wallet::from_private_key(&agent_pk).unwrap();
    let agent_bech32 = agent_wallet.to_address().to_bech32("erd").to_string();
    let agent_pem = create_temp_pem_file("quota_agent", &agent_pk, &agent_bech32);

    let db_path = std::env::temp_dir()
        .join(format!("relayer-quota-{}.db", std::process::id()))
        .to_string_lossy()
        .into_owned();

    let relayer_url = start_relayer(
        &mut pm,
        &gateway_url,
        &registry_bech32,
        relayer_wallets_dir.to_str().unwrap(),
        &chain_id,
        &[
            ("QUOTA_LIMIT", "1"),
            ("DB_PATH", db_path.as_str()),
            ("SKIP_SIMULATION", "true"),
            ("IS_TEST_ENV", "true"),
        ],
    )
    .await;

    let reg_output = std::process::Command::new("npm")
        .args(["run", "register"])
        .current_dir("../moltbot-starter-kit")
        .env("MULTIVERSX_PRIVATE_KEY", agent_pem.as_str())
        .env("MULTIVERSX_API_URL", &gateway_url)
        .env("IDENTITY_REGISTRY_ADDRESS", &registry_bech32)
        .env("CHAIN_ID", &chain_id)
        .env("MULTIVERSX_CHAIN_ID", &chain_id)
        .env("MULTIVERSX_RELAYER_URL", &relayer_url)
        .env("FORCE_RELAYER", "true")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("register script");

    assert!(
        reg_output.status.success(),
        "First relayed registration should succeed:\n{}",
        String::from_utf8_lossy(&reg_output.stderr)
    );
    generate_blocks_on_simulator(10, &gateway_url).await;

    let reg_output_2 = std::process::Command::new("npm")
        .args(["run", "register"])
        .current_dir("../moltbot-starter-kit")
        .env("MULTIVERSX_PRIVATE_KEY", agent_pem.as_str())
        .env("MULTIVERSX_API_URL", &gateway_url)
        .env("IDENTITY_REGISTRY_ADDRESS", &registry_bech32)
        .env("CHAIN_ID", &chain_id)
        .env("MULTIVERSX_CHAIN_ID", &chain_id)
        .env("MULTIVERSX_RELAYER_URL", &relayer_url)
        .env("FORCE_RELAYER", "true")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("second register script");

    let stderr = String::from_utf8_lossy(&reg_output_2.stderr);
    let stdout = String::from_utf8_lossy(&reg_output_2.stdout);
    let combined = format!("{stdout}{stderr}").to_lowercase();

    assert!(
        !reg_output_2.status.success()
            || combined.contains("quota")
            || combined.contains("429"),
        "Second relay should fail when quota exceeded. stdout: {stdout} stderr: {stderr}"
    );

    let client = reqwest::Client::new();
    let relay_res = client
        .post(format!("{relayer_url}/relay"))
        .json(&json!({
            "transaction": {
                "nonce": 99,
                "value": "0",
                "receiver": registry_bech32,
                "sender": agent_bech32,
                "gasPrice": 1000000000,
                "gasLimit": 50000000,
                "chainID": chain_id,
                "version": 1
            }
        }))
        .send()
        .await
        .expect("relay request");

    assert_eq!(
        relay_res.status(),
        429,
        "HTTP relay after quota exhaustion should return 429"
    );

    println!("✅ Suite V3: Relayer quota enforcement verified (HTTP 429)");
}
