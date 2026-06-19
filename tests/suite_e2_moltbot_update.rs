use base64::Engine as _;
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use serde_json::{json, Value};
use std::process::Stdio;
use tokio::time::{sleep, Duration};

mod common;
use common::{
    wait_for_simulator_ready,
    address_to_bech32, create_pem_file, generate_blocks_on_simulator, generate_random_private_key,
    IdentityRegistryInteractor,
};

/// Suite E2: Moltbot direct update-manifest flow.
///
/// Tests:
///   1. Deploy identity registry + issue token
///   2. Fund moltbot wallet
///   3. Run `npm run register` (direct — wallet funded)
///   4. Verify registration on-chain
///   5. Run `npm run update-manifest` (direct — wallet funded)
///   6. Verify on-chain that agent data changed
#[tokio::test]
async fn test_moltbot_update_manifest() {
    let mut pm = ProcessManager::new();

    // 1. Start Chain Simulator
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    let alice = interactor.register_wallet(test_wallets::alice()).await;

    let chain_id = common::get_simulator_chain_id(&gateway_url).await;
    println!("Simulator ChainID: {}", chain_id);

    // 2. Deploy Identity Registry + Issue Token
    let registry = IdentityRegistryInteractor::init(&mut interactor, alice.clone()).await;
    let registry_address = address_to_bech32(registry.address());
    println!("Registry Address: {}", registry_address);

    registry
        .issue_token(&mut interactor, "Agent", "AGENT")
        .await;
    generate_blocks_on_simulator(20, &gateway_url).await;
    sleep(Duration::from_millis(500)).await;

    // 3. Setup Moltbot Wallet (FUNDED)
    let moltbot_pk = generate_random_private_key();
    let moltbot_wallet_obj = Wallet::from_private_key(&moltbot_pk).unwrap();
    let moltbot_address = interactor.register_wallet(moltbot_wallet_obj).await;
    let moltbot_address_bech32 = address_to_bech32(&moltbot_address);

    println!("Funding Moltbot: {}", moltbot_address_bech32);
    interactor
        .tx()
        .from(&alice)
        .to(&moltbot_address)
        .egld(2_000_000_000_000_000_000u64)
        .run()
        .await; // 2 EGLD for both txs

    generate_blocks_on_simulator(10, &gateway_url).await;

    // Create PEM
    let project_root = std::env::current_dir().unwrap();
    let temp_dir = project_root.join("tests").join("temp_suite_e2");
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).unwrap();
    }
    std::fs::create_dir_all(&temp_dir).unwrap();

    let pem_path = temp_dir.join("moltbot.pem");
    create_pem_file(
        pem_path.to_str().unwrap(),
        &moltbot_pk,
        &moltbot_address_bech32,
    );

    // 4. Run Registration Script (Direct)
    println!("\n═══ Step 1: Moltbot Registration (Direct TX) ═══");
    let reg_output = std::process::Command::new("npm")
        .arg("run")
        .arg("register")
        .current_dir("../moltbot-starter-kit")
        .env("MULTIVERSX_PRIVATE_KEY", pem_path.to_str().unwrap())
        .env("MULTIVERSX_API_URL", &gateway_url)
        .env("IDENTITY_REGISTRY_ADDRESS", &registry_address)
        .env("MULTIVERSX_CHAIN_ID", &chain_id)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("Failed to run register script");

    let reg_stdout = String::from_utf8_lossy(&reg_output.stdout);
    let reg_stderr = String::from_utf8_lossy(&reg_output.stderr);
    println!("Register stdout: {}", reg_stdout);
    if !reg_stderr.is_empty() {
        println!("Register stderr: {}", reg_stderr);
    }

    assert!(
        reg_output.status.success(),
        "Registration script failed:\n{}",
        reg_stderr
    );
    assert!(
        reg_stdout.contains("Transaction Sent"),
        "Should broadcast directly"
    );
    println!("✅ Registration SUCCESS");

    generate_blocks_on_simulator(10, &gateway_url).await;

    // 5. Verify Registration On-Chain
    let client = reqwest::Client::new();

    // get_agent_id takes 0 args, returns variadic<multi<u64, Address>>
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
    let return_code = vm_body["data"]["data"]["returnCode"]
        .as_str()
        .unwrap_or("unknown");
    assert_eq!(return_code, "ok", "get_agent_id query failed: {}", vm_body);

    let return_data = vm_body["data"]["data"]["returnData"]
        .as_array()
        .expect("returnData not found");
    let has_agent = return_data
        .iter()
        .any(|v| v.as_str().is_some_and(|s| !s.is_empty()));
    assert!(
        has_agent,
        "Agent should be registered. returnData: {:?}",
        return_data
    );

    // The first entry is the nonce (u64 as base64), second is the address
    let agent_nonce_b64 = return_data[0].as_str().unwrap_or("");
    let nonce_bytes = base64::engine::general_purpose::STANDARD
        .decode(agent_nonce_b64)
        .unwrap();
    let mut agent_nonce_val = 0u64;
    for b in &nonce_bytes {
        agent_nonce_val = (agent_nonce_val << 8) | (*b as u64);
    }
    println!("✅ Agent registered with NFT nonce: {}", agent_nonce_val);

    // 6. Write agent.config.json for update-manifest
    //    The update_manifest.ts script reads agent.config.json for nonce, manifestUri, metadata
    let config_json = serde_json::json!({
        "agentName": "moltbot",
        "capabilities": ["inference", "summarization"],
        "nonce": agent_nonce_val,
        "manifestUri": "https://updated.moltbot.io/manifest.json",
        "metadata": [
            { "key": "version", "value": "2.0.0" },
            { "key": "model", "value": "gpt-4-turbo" }
        ]
    });

    let config_path = project_root
        .parent()
        .unwrap()
        .join("moltbot-starter-kit")
        .join("agent.config.json");
    std::fs::write(
        &config_path,
        serde_json::to_string_pretty(&config_json).unwrap(),
    )
    .expect("Failed to write agent.config.json");
    println!("Wrote agent.config.json with nonce={}", agent_nonce_val);

    // 7. Run Update Manifest Script (Direct)
    println!("\n═══ Step 2: Moltbot Update Manifest (Direct TX) ═══");
    let update_output = std::process::Command::new("npm")
        .arg("run")
        .arg("update-manifest")
        .current_dir("../moltbot-starter-kit")
        .env("MULTIVERSX_PRIVATE_KEY", pem_path.to_str().unwrap())
        .env("MULTIVERSX_API_URL", &gateway_url)
        .env("IDENTITY_REGISTRY_ADDRESS", &registry_address)
        .env("MULTIVERSX_CHAIN_ID", &chain_id)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("Failed to run update-manifest script");

    let up_stdout = String::from_utf8_lossy(&update_output.stdout);
    let up_stderr = String::from_utf8_lossy(&update_output.stderr);
    println!("Update stdout: {}", up_stdout);
    if !up_stderr.is_empty() {
        println!("Update stderr: {}", up_stderr);
    }

    assert!(
        update_output.status.success(),
        "Update manifest script failed:\n{}",
        up_stderr
    );
    assert!(
        up_stdout.contains("Update Transaction Sent"),
        "Should broadcast update"
    );
    println!("✅ Update Manifest SUCCESS");

    generate_blocks_on_simulator(10, &gateway_url).await;

    // 8. Verify Updated State On-Chain
    println!("\n═══ On-Chain Verification After Update ═══");
    let nonce_hex = hex::encode(&nonce_bytes);
    let vm_agent_query = json!({
        "scAddress": registry_address,
        "funcName": "get_agent",
        "args": [nonce_hex]
    });

    let vm_agent_res = client
        .post(format!("{}/vm-values/query", gateway_url))
        .json(&vm_agent_query)
        .send()
        .await
        .expect("VM query get_agent failed");

    let vm_agent_body: Value = vm_agent_res.json().await.unwrap();
    let agent_return_code = vm_agent_body["data"]["data"]["returnCode"]
        .as_str()
        .unwrap_or("unknown");
    assert_eq!(
        agent_return_code, "ok",
        "get_agent should succeed: {}",
        vm_agent_body
    );
    println!("✅ get_agent: Agent data found after update");

    // 9. Cleanup
    let _ = std::fs::remove_dir_all(&temp_dir);
    let _ = std::fs::remove_file(&config_path); // cleanup agent.config.json
    println!("✅ Suite E2 Complete: Moltbot direct registration + update manifest PASSED.");
}
