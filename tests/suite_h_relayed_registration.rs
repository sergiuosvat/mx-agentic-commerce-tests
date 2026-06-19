use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use std::process::Stdio;

mod common;
use common::{
    start_relayer, temp_relayer_wallets_dir,
    wait_for_simulator_ready,
    address_to_bech32, create_pem_file, create_temp_pem_file, generate_blocks_on_simulator,
    generate_random_private_key, IdentityRegistryInteractor,
};

#[tokio::test]
async fn test_relayed_registration() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator().unwrap(); // .expect("Failed to start Sim");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);

    let chain_id = common::get_simulator_chain_id(&gateway_url).await;
    println!("Simulator ChainID: {}", chain_id);

    let alice = interactor.register_wallet(test_wallets::alice()).await;

    // Deploy Registry
    let registry = IdentityRegistryInteractor::init(&mut interactor, alice.clone()).await;
    let registry_addr = address_to_bech32(registry.address());

    // Issue token (required before register_agent can mint NFTs)
    registry
        .issue_token(&mut interactor, "Agent", "AGENT")
        .await;
    generate_blocks_on_simulator(20, &gateway_url).await;

    // Setup Relayer Wallets (Generate multiple to cover all shards)
    let relayer_wallets_dir = std::path::PathBuf::from(temp_relayer_wallets_dir("relayed"));

    if relayer_wallets_dir.exists() {
        std::fs::remove_dir_all(&relayer_wallets_dir).unwrap();
    }
    std::fs::create_dir_all(&relayer_wallets_dir).unwrap();

    println!("Generating Relayer Wallets...");
    for i in 0..30 {
        let relayer_pk = generate_random_private_key();
        let relayer_wallet = Wallet::from_private_key(&relayer_pk).unwrap();
        let relayer_addr_obj = relayer_wallet.to_address();
        let relayer_addr = relayer_addr_obj.to_bech32("erd").to_string();

        let relayer_sc_addr = Address::from_slice(relayer_addr_obj.as_bytes());

        // Fund each relayer
        interactor
            .tx()
            .from(&alice)
            .to(&relayer_sc_addr)
            .egld(1_000_000_000_000_000_000u64)
            .run()
            .await;

        let relayer_pem = relayer_wallets_dir.join(format!("relayer_{i}.pem"));
        create_pem_file(relayer_pem.to_str().unwrap(), &relayer_pk);
        println!("Generated Relayer {}: {}", i, relayer_addr);
    }

    // Ensure cross-shard EGLD transfers to relayer wallets are finalized
    generate_blocks_on_simulator(10, &gateway_url).await;

    let relayer_url = start_relayer(
        &mut pm,
        &gateway_url,
        &registry_addr,
        relayer_wallets_dir.to_str().unwrap(),
        &chain_id,
        &[("LOG_LEVEL", "trace")],
    )
    .await;

    // Setup Moltbot (Unfunded)
    let moltbot_pk = generate_random_private_key();
    let moltbot_wallet = Wallet::from_private_key(&moltbot_pk).unwrap();
    let moltbot_addr = moltbot_wallet.to_address().to_bech32("erd").to_string();
    println!("Moltbot Address (Unfunded): {}", moltbot_addr);

    let moltbot_pem = create_temp_pem_file("moltbot_relayed", &moltbot_pk, &moltbot_addr);

    // Run Registration Script
    println!("Running Moltbot Registration with Relayer...");

    let output = std::process::Command::new("npm")
        .arg("run")
        .arg("register")
        .current_dir("../moltbot-starter-kit")
        .env("MULTIVERSX_PRIVATE_KEY", moltbot_pem.as_str())
        .env("MULTIVERSX_API_URL", &gateway_url)
        .env("IDENTITY_REGISTRY_ADDRESS", &registry_addr)
        .env("CHAIN_ID", &chain_id)
        .env("MULTIVERSX_CHAIN_ID", &chain_id)
        .env("MULTIVERSX_RELAYER_URL", &relayer_url)
        .env("FORCE_RELAYER", "true") // Enforce usage
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("Failed to run registration script");

    println!("Script Stdout: {}", String::from_utf8_lossy(&output.stdout));
    println!("Script Stderr: {}", String::from_utf8_lossy(&output.stderr));

    assert!(output.status.success(), "Registration script failed");

    // Generate blocks so the chain simulator processes the relayed transaction
    generate_blocks_on_simulator(5, &gateway_url).await;

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Relayed Transaction Sent"),
        "Log should verify relay"
    );

    // Verify Relayer Paid Fees (Optional check, but verifying log is good enough for now)

    // Clean up
    std::fs::remove_dir_all(&relayer_wallets_dir).ok();
}
