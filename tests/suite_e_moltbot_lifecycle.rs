use base64::Engine as _;
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use serde_json::{json, Value};
use std::process::Stdio;
use tokio::time::{sleep, Duration};

mod common;
use common::{
    wait_for_simulator_ready,
    address_to_bech32, create_temp_pem_file, generate_blocks_on_simulator, generate_random_private_key,
    IdentityRegistryInteractor,
};

/// Suite E: Moltbot direct registration (funded wallet, no relayer).
///
/// Tests:
///   1. Deploy identity registry + issue token
///   2. Fund moltbot wallet
///   3. Run `npm run register` with a funded wallet (direct broadcast path)
///   4. Verify on-chain that the agent is registered via `get_agent_id` vmQuery
#[tokio::test]
async fn test_moltbot_lifecycle() {
    let mut pm = ProcessManager::new();

    // 1. Start Chain Simulator
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    // 2. Setup Interactor & Admin
    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    let alice = interactor.register_wallet(test_wallets::alice()).await;

    let chain_id = common::get_simulator_chain_id(&gateway_url).await;
    println!("Simulator ChainID: {}", chain_id);

    // 3. Deploy Identity Registry + Issue Token
    let registry = IdentityRegistryInteractor::init(&mut interactor, alice.clone()).await;
    let registry_address = address_to_bech32(registry.address());
    println!("Registry Address: {}", registry_address);

    registry
        .issue_token(&mut interactor, "Agent", "AGENT")
        .await;
    generate_blocks_on_simulator(20, &gateway_url).await;
    sleep(Duration::from_millis(500)).await;

    // 4. Setup Moltbot Wallet (FUNDED — direct TX path)
    let moltbot_pk = generate_random_private_key();
    let moltbot_wallet_obj = Wallet::from_private_key(&moltbot_pk).unwrap();
    let moltbot_address = interactor.register_wallet(moltbot_wallet_obj).await;
    let moltbot_address_bech32 = address_to_bech32(&moltbot_address);

    println!("Funding Moltbot: {}", moltbot_address_bech32);
    interactor
        .tx()
        .from(&alice)
        .to(&moltbot_address)
        .egld(1_000_000_000_000_000_000u64)
        .run()
        .await;

    // Ensure cross-shard funding is settled
    generate_blocks_on_simulator(10, &gateway_url).await;

    let pem_path = create_temp_pem_file("moltbot_lifecycle", &moltbot_pk, &moltbot_address_bech32);
    println!("Created PEM at: {pem_path}");

    // 5. Run Registration Script (Direct Mode — wallet is funded)
    println!("\n═══ Running Moltbot Registration (Direct TX) ═══");
    let output = std::process::Command::new("npm")
        .arg("run")
        .arg("register")
        .current_dir("../moltbot-starter-kit")
        .env("MULTIVERSX_PRIVATE_KEY", pem_path.as_str())
        .env("MULTIVERSX_API_URL", &gateway_url)
        .env("IDENTITY_REGISTRY_ADDRESS", &registry_address)
        .env("MULTIVERSX_CHAIN_ID", &chain_id)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("Failed to run register script");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!("Script Stdout: {}", stdout);
    if !stderr.is_empty() {
        println!("Script Stderr: {}", stderr);
    }

    assert!(
        output.status.success(),
        "Registration script failed:\n{}",
        stderr
    );
    assert!(
        stdout.contains("Transaction Sent"),
        "Should broadcast directly (not via relayer). stdout: {}",
        stdout
    );
    println!("✅ register_agent broadcast SUCCESS (direct)");

    // Generate blocks to process the registration transaction
    generate_blocks_on_simulator(10, &gateway_url).await;

    // 6. Verify On-Chain via vmQuery
    println!("\n═══ On-Chain Verification ═══");
    let client = reqwest::Client::new();

    // get_agent_id takes 0 args, returns variadic<multi<u64, Address>> — all agent mappings
    let vm_query = json!({
        "scAddress": registry_address,
        "funcName": "get_agent_id",
        "args": []
    });

    let vm_res = client
        .post(format!("{}/vm-values/query", gateway_url))
        .json(&vm_query)
        .send()
        .await
        .expect("VM query failed");

    let vm_body: Value = vm_res.json().await.unwrap();
    println!(
        "get_agent_id response: {}",
        serde_json::to_string_pretty(&vm_body["data"]["data"]).unwrap_or_default()
    );

    let return_code = vm_body["data"]["data"]["returnCode"]
        .as_str()
        .unwrap_or("unknown");
    assert_eq!(return_code, "ok", "get_agent_id query failed: {}", vm_body);

    let return_data = vm_body["data"]["data"]["returnData"]
        .as_array()
        .expect("returnData not found");

    let agent_nonce_b64 = return_data[0].as_str().unwrap_or("");
    assert!(
        !agent_nonce_b64.is_empty(),
        "Agent should have a non-zero ID after registration. returnData: {:?}",
        return_data
    );

    let nonce_bytes = base64::engine::general_purpose::STANDARD
        .decode(agent_nonce_b64)
        .unwrap();
    let mut agent_nonce_val = 0u64;
    for b in &nonce_bytes {
        agent_nonce_val = (agent_nonce_val << 8) | (*b as u64);
    }
    println!(
        "✅ Agent registered on-chain with NFT nonce: {}",
        agent_nonce_val
    );

    // 7. Cleanup
    println!("✅ Suite E Complete: Moltbot direct registration PASSED.");
}
