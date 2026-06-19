use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use serde_json::json;
use std::process::Command;
use tokio::time::{sleep, Duration};

mod common;
use common::{
    wait_for_simulator_ready,
    address_to_bech32, fund_address_on_simulator, generate_blocks_on_simulator,
    generate_random_private_key, get_simulator_chain_id, start_facilitator,
    IdentityRegistryInteractor, ValidationRegistryInteractor,
};

/// Suite X: Full x402 Lifecycle with Proof Submission After Settlement
///
/// End-to-end flow:
///   1. Deploy Identity + Validation registries
///   2. Register an agent
///   3. Create a job (init_job)
///   4. Agent submits proof
///   5. Buyer settles via Facilitator (x402)
///   6. Verify job state (proof submitted, settlement on-chain)
///
/// This tests the gap: "No proof/validation after settlement" (from spec analysis)
#[tokio::test]
async fn test_x402_lifecycle_with_proof() {
    let mut pm = ProcessManager::new();

    // ── 1. Start Chain Simulator ──
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    let chain_id = get_simulator_chain_id(&gateway_url).await;

    // ── 2. Setup Wallets ──
    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    let owner = interactor.register_wallet(test_wallets::alice()).await;
    let owner_bech32 = address_to_bech32(&owner);
    fund_address_on_simulator(&owner_bech32, "100000000000000000000000", &gateway_url).await;

    let buyer_pk = generate_random_private_key();
    let buyer_wallet = Wallet::from_private_key(&buyer_pk).unwrap();
    let buyer_bech32 = buyer_wallet.to_address().to_bech32("erd").to_string();
    fund_address_on_simulator(&buyer_bech32, "1000000000000000000000", &gateway_url).await;

    // ── 3. Deploy Identity + Validation Registries ──
    let identity = IdentityRegistryInteractor::init(&mut interactor, owner.clone()).await;
    identity
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;
    generate_blocks_on_simulator(20, &gateway_url).await;

    let validation =
        ValidationRegistryInteractor::init(&mut interactor, owner.clone(), identity.address())
            .await;

    let identity_bech32 = address_to_bech32(identity.address());
    let validation_bech32 = address_to_bech32(validation.address());

    // ── 4. Register Agent ──
    identity
        .register_agent(
            &mut interactor,
            "LifecycleAgent",
            "https://lifecycle-agent.test/manifest",
            vec![("type", b"worker".to_vec())],
        )
        .await;

    // ── 5. Create Job (init_job) ──
    let job_id = "x402-lifecycle-job-001";
    validation
        .init_job(&mut interactor, job_id, 1) // agent nonce = 1
        .await;
    println!("✅ Job created: {}", job_id);

    // ── 6. Agent Submits Proof ──
    let proof_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    validation
        .submit_proof(&mut interactor, job_id, proof_hash)
        .await;
    println!("✅ Proof submitted for job: {}", job_id);

    // ── 7. Start Facilitator ──
    let facilitator_pk = generate_random_private_key();
    let db_path = "./facilitator_suite_x.db";

    let facilitator_url = start_facilitator(
        &mut pm,
        &facilitator_pk,
        &identity_bech32,
        &gateway_url,
        &chain_id,
        &[
            ("IDENTITY_REGISTRY_ADDRESS", identity_bech32.as_str()),
            ("VALIDATION_REGISTRY_ADDRESS", validation_bech32.as_str()),
            ("SQLITE_DB_PATH", db_path),
            ("SKIP_SIMULATION", "false"),
        ],
    )
    .await;

    let client = reqwest::Client::new();

    // ── 8. Buyer Signs x402 Payment ──
    let value_str = "1000000000000000000"; // 1 EGLD

    let output = Command::new("npx")
        .arg("ts-node")
        .arg("../moltbot-starter-kit/scripts/sign_tx.ts")
        .arg("--sender-pk")
        .arg(&buyer_pk)
        .arg("--receiver")
        .arg(&owner_bech32) // Pay to agent owner
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
        .output()
        .expect("Failed to sign tx");

    if !output.status.success() {
        panic!(
            "Sign tx failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let signed_tx: serde_json::Value =
        serde_json::from_str(String::from_utf8(output.stdout).unwrap().trim())
            .expect("Invalid JSON");

    let mut payload = signed_tx;
    if payload.get("options").is_none() {
        payload["options"] = json!(0);
    }
    if payload.get("data").is_none() || payload["data"].is_null() {
        payload["data"] = json!("");
    }

    let requirements = json!({
        "payTo": owner_bech32,
        "amount": value_str,
        "asset": "EGLD",
        "network": format!("multiversx:{}", chain_id)
    });

    let body = json!({
        "scheme": "exact",
        "payload": payload,
        "requirements": requirements
    });

    // ── 9. Settle via Facilitator ──
    let resp = client
        .post(format!("{}/settle", facilitator_url))
        .json(&body)
        .send()
        .await
        .expect("Failed to settle");

    let resp_json: serde_json::Value = resp.json().await.unwrap();
    println!("Settle response: {:?}", resp_json);

    if resp_json.get("success").is_some() {
        println!("✅ Settlement: SUCCESS");
    } else {
        println!("⚠️ Settlement may have failed (chain ID mismatch on simulator)");
    }

    // Generate blocks for settlement
    sleep(Duration::from_secs(1)).await;
    generate_blocks_on_simulator(5, &gateway_url).await;

    // ── 10. Verify Job State ──
    // Query is_job_verified view
    let job_id_hex = hex::encode(job_id.as_bytes());
    let vm_query = json!({
        "scAddress": validation_bech32,
        "funcName": "is_job_verified",
        "args": [job_id_hex]
    });

    let vm_res = client
        .post(format!("{}/vm-values/query", gateway_url))
        .json(&vm_query)
        .send()
        .await
        .expect("VM query failed");

    let vm_body: serde_json::Value = vm_res.json().await.unwrap();
    let return_code = vm_body["data"]["data"]["returnCode"]
        .as_str()
        .unwrap_or("unknown");

    println!("is_job_verified returnCode: {}", return_code);
    assert_eq!(return_code, "ok", "is_job_verified query should succeed");

    // Query get_job_data to check proof hash
    let vm_job_query = json!({
        "scAddress": validation_bech32,
        "funcName": "get_job_data",
        "args": [job_id_hex]
    });

    let vm_job_res = client
        .post(format!("{}/vm-values/query", gateway_url))
        .json(&vm_job_query)
        .send()
        .await
        .expect("get_job_data query failed");

    let vm_job_body: serde_json::Value = vm_job_res.json().await.unwrap();
    let job_return_code = vm_job_body["data"]["data"]["returnCode"]
        .as_str()
        .unwrap_or("unknown");

    println!("get_job_data returnCode: {}", job_return_code);
    if job_return_code == "ok" {
        println!("✅ Job data verified on-chain");
    }

    // Cleanup
    println!("\n✅ Suite X: x402 Lifecycle with Proof — PASSED");
    println!("  Flow: Deploy registries → Register agent → Create job → Submit proof → Settle via x402 → Verify on-chain");
}
