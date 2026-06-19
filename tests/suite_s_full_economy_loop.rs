use multiversx_sc::types::ManagedBuffer;
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
mod common;
use common::{
    wait_for_simulator_ready,
    address_to_bech32, deploy_all_registries, generate_blocks_on_simulator,
    IdentityRegistryInteractor, ServiceConfigInput, ValidationRegistryInteractor,
};

/// Suite S: Full Agent Economy Loop
///
/// End-to-end test of the complete agent economy:
/// 1. Register agent with service pricing
/// 2. Buyer init_job WITH payment (matching service config)
/// 3. Agent submits proof
/// 4. Validator requests + approves validation
/// 5. Buyer submits feedback
/// 6. Query reputation score
///
/// This is the flow that was completely untested end-to-end WITH payment.
///
/// Starts after epoch 1: generate 25 blocks after simulator start.
#[tokio::test]
async fn test_full_agent_economy_loop() {
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
    let wallet_bob = interactor.register_wallet(test_wallets::bob()).await;
    let alice_bech32 = address_to_bech32(&wallet_alice);
    let bob_bech32 = address_to_bech32(&wallet_bob);

    println!("Alice (Buyer/Employer): {}", alice_bech32);
    println!("Bob (Agent/Worker): {}", bob_bech32);

    // ── 2. Deploy all 3 registries ──
    let (identity, validation_addr, reputation_addr) =
        deploy_all_registries(&mut interactor, wallet_alice.clone()).await;

    let identity_bech32 = address_to_bech32(identity.address());
    let validation_bech32 = address_to_bech32(&validation_addr);
    let reputation_bech32 = address_to_bech32(&reputation_addr);
    println!("Identity: {}", identity_bech32);
    println!("Validation: {}", validation_bech32);
    println!("Reputation: {}", reputation_bech32);

    // ── 3. Bob registers as an agent with a priced service ──
    // Transfer the agent NFT to Bob by registering from Bob's wallet
    // First, we need identity contract to be callable by Bob
    // Bob registers himself
    let bob_identity = IdentityRegistryInteractor {
        wallet_address: wallet_bob.clone(),
        contract_address: identity.address().clone(),
    };

    let services = vec![ServiceConfigInput::<StaticApi> {
        service_id: 1,
        price: BigUint::from(500_000_000_000_000_000u64), // 0.5 EGLD
        token: EgldOrEsdtTokenIdentifier::egld(),
        nonce: 0,
    }];

    bob_identity
        .register_agent_with_services(
            &mut interactor,
            "WorkerBot",
            "https://workerbot.example.com/manifest.json",
            vec![
                ("type", b"worker".to_vec()),
                ("specialty", b"data-analysis".to_vec()),
            ],
            services,
        )
        .await;
    println!("\n✅ Phase 1: Bob registered as agent (nonce=1) with price=0.5 EGLD");

    // ── 4. Alice (Buyer) discovers Bob's service config via view ──
    let client = reqwest::Client::new();
    let nonce_hex = hex::encode(1u64.to_be_bytes());
    let service_id_hex = hex::encode(1u32.to_be_bytes());

    let body_svc = serde_json::json!({
        "scAddress": identity_bech32,
        "funcName": "get_agent_service_config",
        "args": [nonce_hex, service_id_hex],
    });
    let resp_svc: serde_json::Value = client
        .post(format!("{}/vm-values/query", gateway_url))
        .json(&body_svc)
        .send()
        .await
        .expect("VM query failed")
        .json()
        .await
        .expect("VM query parse failed");
    let return_data_svc = resp_svc["data"]["data"]["returnData"]
        .as_array()
        .expect("No returnData");
    assert!(
        !return_data_svc.is_empty(),
        "Alice should be able to query Bob's service config"
    );
    println!("✅ Phase 2: Alice discovered Bob's service pricing via get_agent_service_config");

    // ── 5. Alice creates a job WITH payment ──
    let validation = ValidationRegistryInteractor {
        wallet_address: wallet_alice.clone(),
        contract_address: validation_addr.clone(),
    };

    validation
        .init_job_with_payment(
            &mut interactor,
            "economy-job-001",
            1, // agent_nonce (Bob)
            1, // service_id
            "EGLD",
            500_000_000_000_000_000u64, // 0.5 EGLD
        )
        .await;
    println!("✅ Phase 3: Alice init_job with 0.5 EGLD payment for Bob's service");

    // Verify job exists via view
    let job_id_hex = hex::encode("economy-job-001".as_bytes());
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
    assert!(
        !resp_job["data"]["data"]["returnData"]
            .as_array()
            .expect("No returnData")
            .is_empty(),
        "Job should be visible via get_job_data"
    );
    println!("  Verified: job visible on-chain via get_job_data");

    // ── 6. Bob (Agent) submits proof of work ──
    // Bob calls submit_proof (he's the agent owner registered in the identity contract)
    let proof_hash = "sha256:economy-proof-abc123";
    let job_id_buf: ManagedBuffer<StaticApi> =
        ManagedBuffer::new_from_bytes("economy-job-001".as_bytes());
    let proof_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(proof_hash.as_bytes());

    interactor
        .tx()
        .from(&wallet_bob)
        .to(&validation_addr)
        .gas(20_000_000)
        .raw_call("submit_proof")
        .argument(&job_id_buf)
        .argument(&proof_buf)
        .run()
        .await;
    println!("✅ Phase 4: Bob submitted proof: {}", proof_hash);

    // ── 7. Validation flow ──
    // Bob (agent owner) requests validation, naming Alice as validator
    let bob_validation = ValidationRegistryInteractor {
        wallet_address: wallet_bob.clone(),
        contract_address: validation_addr.clone(),
    };

    bob_validation
        .validation_request(
            &mut interactor,
            "economy-job-001",
            &wallet_alice, // Alice is the validator
            "https://oracle.example.com/verify",
            "economy-req-001",
        )
        .await;
    println!("✅ Phase 5a: Validation requested (by Bob, validator=Alice)");

    // Alice (validator) approves the validation
    validation
        .validation_response(
            &mut interactor,
            "economy-req-001",
            1, // approved
            "https://oracle.example.com/result",
            "economy-resp-001",
            "quality-verified",
        )
        .await;
    println!("✅ Phase 5b: Validation approved (by Alice)");

    // Verify job is now verified
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
    let is_verified = !return_data_verified.is_empty()
        && return_data_verified.iter().any(|v| {
            let s = v.as_str().unwrap_or("");
            !s.is_empty()
        });
    assert!(is_verified, "Job should be verified after validation");
    println!("  Verified: is_job_verified = true");

    // ── 8. Alice (Employer) submits feedback ──
    let rating: u64 = 90;
    let rating_big = BigUint::<StaticApi>::from(rating);
    let agent_nonce: u64 = 1;

    interactor
        .tx()
        .from(&wallet_alice)
        .to(&reputation_addr)
        .gas(20_000_000)
        .raw_call("giveFeedbackSimple")
        .argument(&job_id_buf)
        .argument(&agent_nonce)
        .argument(&rating_big)
        .run()
        .await;
    println!("✅ Phase 6: Alice submitted feedback: rating={}", rating);

    // ── 9. Query reputation score ──
    let body_score = serde_json::json!({
        "scAddress": reputation_bech32,
        "funcName": "get_reputation_score",
        "args": [nonce_hex],
    });
    let resp_score: serde_json::Value = client
        .post(format!("{}/vm-values/query", gateway_url))
        .json(&body_score)
        .send()
        .await
        .expect("VM query failed")
        .json()
        .await
        .expect("VM query parse failed");
    let return_data_score = resp_score["data"]["data"]["returnData"]
        .as_array()
        .expect("No returnData");
    assert!(
        !return_data_score.is_empty(),
        "Score query should return data"
    );

    let score_b64 = return_data_score[0].as_str().unwrap_or("");
    let score_bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, score_b64)
        .unwrap_or_default();
    let mut score_val: u64 = 0;
    for byte in &score_bytes {
        score_val = (score_val << 8) | (*byte as u64);
    }
    println!("✅ Phase 7: Reputation Score = {} (expected 90)", score_val);
    assert!(
        score_val > 0,
        "Reputation score should be > 0 after positive feedback"
    );
    assert_eq!(
        score_val, 90,
        "Reputation score should be 90 after single job with rating 90"
    );

    // ── 10. Bob appends a response to the feedback ──
    let response_uri_buf: ManagedBuffer<StaticApi> =
        ManagedBuffer::new_from_bytes(b"https://workerbot.example.com/thank-you");

    interactor
        .tx()
        .from(&wallet_bob)
        .to(&reputation_addr)
        .gas(20_000_000)
        .raw_call("append_response")
        .argument(&job_id_buf)
        .argument(&response_uri_buf)
        .run()
        .await;
    println!("✅ Phase 8: Bob appended response to feedback");

    println!("\n🎉 Suite S: Full Agent Economy Loop — PASSED ✅");
    println!("  Flow: Register → Price → Discover → Pay → Prove → Validate → Feedback → Score");
    println!("  Participants: Alice (Buyer), Bob (Agent)");
    println!("  Payment: 0.5 EGLD for service_id=1");
    println!("  Final Score: {}", score_val);
}
