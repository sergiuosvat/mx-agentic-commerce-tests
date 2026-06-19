use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
mod common;
use common::{
    wait_for_simulator_ready,
    address_to_bech32, deploy_all_registries, generate_blocks_on_simulator, ServiceConfigInput, ValidationRegistryInteractor,
};

/// Suite Q: Validation Registry Extended Tests
///
/// Tests the following uncovered flows:
/// 1. init_job with EGLD payment (service_id match)
/// 2. Views: is_job_verified, get_job_data, get_validation_status, get_agent_validations
/// 3. Full validation_request + validation_response flow with view verification
///
/// Starts after epoch 1: generate 25 blocks after simulator start.
#[tokio::test]
async fn test_validation_extended_operations() {
    let mut pm = ProcessManager::new();

    // ── 1. Start Chain Simulator ──
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    // Generate 25 blocks to pass epoch 1
    generate_blocks_on_simulator(25, &gateway_url).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    let wallet_alice = interactor.register_wallet(test_wallets::alice()).await;

    // ── 2. Deploy all 3 registries ──
    let (identity, validation_addr, ..) =
        deploy_all_registries(&mut interactor, wallet_alice.clone()).await;

    let identity_bech32 = address_to_bech32(identity.address());
    let validation_bech32 = address_to_bech32(&validation_addr);
    println!("Identity: {}", identity_bech32);
    println!("Validation: {}", validation_bech32);

    // ── 3. Register an agent with a priced service ──
    let services = vec![ServiceConfigInput::<StaticApi> {
        service_id: 1,
        price: BigUint::from(100_000_000_000_000_000u64), // 0.1 EGLD
        token: EgldOrEsdtTokenIdentifier::egld(),
        nonce: 0,
    }];

    identity
        .register_agent_with_services(
            &mut interactor,
            "PricedBot",
            "https://pricedbot.example.com/manifest.json",
            vec![("type", b"validator".to_vec())],
            services,
        )
        .await;
    println!("Agent registered: PricedBot (nonce=1)");

    // ── 4. Create ValidationRegistryInteractor ──
    // We need to use raw calls since we already deployed
    let validation = ValidationRegistryInteractor {
        wallet_address: wallet_alice.clone(),
        contract_address: validation_addr.clone(),
    };

    // ── 5. init_job WITHOUT payment (basic, no service_id) ──
    validation
        .init_job(&mut interactor, "job-basic-001", 1)
        .await;
    println!("✅ Job initialized without payment: job-basic-001");

    // ── 6. init_job WITH EGLD payment (service_id=1) ──
    validation
        .init_job_with_payment(
            &mut interactor,
            "job-paid-001",
            1,
            1, // service_id
            "EGLD",
            100_000_000_000_000_000u64, // 0.1 EGLD
        )
        .await;
    println!("✅ Job initialized with 0.1 EGLD payment: job-paid-001");

    // ── 7. Query get_job_data view for job-paid-001 ──
    let client = reqwest::Client::new();
    let job_id_hex = hex::encode("job-paid-001".as_bytes());
    let body_job = serde_json::json!({
        "scAddress": validation_bech32,
        "funcName": "get_job_data",
        "args": [job_id_hex],
    });
    let resp_job: serde_json::Value = client
        .post(format!("{}/vm-values/query", gateway_url))
        .json(&body_job)
        .send()
        .await
        .expect("VM query failed")
        .json()
        .await
        .expect("VM query parse failed");
    let return_data_job = resp_job["data"]["data"]["returnData"]
        .as_array()
        .expect("No returnData");
    assert!(
        !return_data_job.is_empty(),
        "get_job_data should return data for job-paid-001"
    );
    println!("✅ get_job_data returned data for job-paid-001");

    // ── 8. Query is_job_verified — should be false (not yet verified) ──
    let body_verified = serde_json::json!({
        "scAddress": validation_bech32,
        "funcName": "is_job_verified",
        "args": [job_id_hex],
    });
    let resp_verified: serde_json::Value = client
        .post(format!("{}/vm-values/query", gateway_url))
        .json(&body_verified)
        .send()
        .await
        .expect("VM query failed")
        .json()
        .await
        .expect("VM query parse failed");
    let return_data_verified = resp_verified["data"]["data"]["returnData"]
        .as_array()
        .expect("No returnData");
    // is_job_verified returns false (empty or 0x00)
    let is_not_verified = return_data_verified.is_empty()
        || return_data_verified.iter().all(|v| {
            let s = v.as_str().unwrap_or("");
            s.is_empty()
        });
    assert!(is_not_verified, "Job should NOT be verified yet");
    println!("✅ is_job_verified correctly returns false before verification");

    // ── 9. Submit proof for job-paid-001 ──
    validation
        .submit_proof(&mut interactor, "job-paid-001", "sha256:proof123paid")
        .await;
    println!("✅ Proof submitted for job-paid-001");

    // ── 10. validation_request + validation_response ──
    validation
        .validation_request(
            &mut interactor,
            "job-paid-001",
            &wallet_alice,
            "https://validator.example.com/check",
            "req-hash-q001",
        )
        .await;
    println!("✅ Validation requested: req-hash-q001");

    validation
        .validation_response(
            &mut interactor,
            "req-hash-q001",
            1, // approved
            "https://validator.example.com/result",
            "resp-hash-q001",
            "quality-check",
        )
        .await;
    println!("✅ Validation approved: req-hash-q001");

    // ── 11. Query is_job_verified — should be true now ──
    let resp_verified2: serde_json::Value = client
        .post(format!("{}/vm-values/query", gateway_url))
        .json(&body_verified)
        .send()
        .await
        .expect("VM query failed")
        .json()
        .await
        .expect("VM query parse failed");
    let return_data_verified2 = resp_verified2["data"]["data"]["returnData"]
        .as_array()
        .expect("No returnData");
    // After verification, should return true (non-empty data, value = 0x01)
    let has_data = !return_data_verified2.is_empty()
        && return_data_verified2.iter().any(|v| {
            let s = v.as_str().unwrap_or("");
            !s.is_empty()
        });
    assert!(
        has_data,
        "Job should be verified after validation_response(approved)"
    );
    println!("✅ is_job_verified correctly returns true after verification");

    // ── 12. Query get_validation_status ──
    let req_hash_hex = hex::encode("req-hash-q001".as_bytes());
    let body_validation_status = serde_json::json!({
        "scAddress": validation_bech32,
        "funcName": "get_validation_status",
        "args": [req_hash_hex],
    });
    let resp_validation_status: serde_json::Value = client
        .post(format!("{}/vm-values/query", gateway_url))
        .json(&body_validation_status)
        .send()
        .await
        .expect("VM query failed")
        .json()
        .await
        .expect("VM query parse failed");
    let return_data_vs = resp_validation_status["data"]["data"]["returnData"]
        .as_array()
        .expect("No returnData");
    assert!(
        !return_data_vs.is_empty(),
        "get_validation_status should return data"
    );
    println!("✅ get_validation_status returned data for req-hash-q001");

    // ── 13. Query get_agent_validations ──
    let nonce_hex = hex::encode(1u64.to_be_bytes());
    let body_agent_validations = serde_json::json!({
        "scAddress": validation_bech32,
        "funcName": "get_agent_validations",
        "args": [nonce_hex],
    });
    let resp_agent_validations: serde_json::Value = client
        .post(format!("{}/vm-values/query", gateway_url))
        .json(&body_agent_validations)
        .send()
        .await
        .expect("VM query failed")
        .json()
        .await
        .expect("VM query parse failed");
    let return_data_av = resp_agent_validations["data"]["data"]["returnData"]
        .as_array()
        .expect("No returnData");
    assert!(
        !return_data_av.is_empty(),
        "get_agent_validations should return request hashes"
    );
    println!("✅ get_agent_validations returned data for agent nonce=1");

    println!("\n🎉 Suite Q: Validation Extended Operations — PASSED ✅");
    println!("  Tested: init_job with EGLD payment");
    println!(
        "  Tested: get_job_data, is_job_verified, get_validation_status, get_agent_validations"
    );
    println!("  Tested: Full validation_request → validation_response → job verified flow");
}
