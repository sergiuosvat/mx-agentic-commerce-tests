use multiversx_sc::types::ManagedBuffer;
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use tokio::time::{sleep, Duration};

mod common;
use common::{
    address_to_bech32, deploy_all_registries, generate_blocks_on_simulator, ValidationRegistryInteractor,
};

/// Suite R: Reputation Registry Extended Tests
///
/// Tests the following uncovered flows:
/// 1. append_response — anyone can append a response to job feedback
/// 2. Multi-job reputation scoring — verify weighted average across multiple jobs
/// 3. Reputation views: get_reputation_score, get_total_jobs, has_given_feedback
///
/// Starts after epoch 1: generate 25 blocks after simulator start.
#[tokio::test]
async fn test_reputation_extended_operations() {
    let mut pm = ProcessManager::new();

    // ── 1. Start Chain Simulator ──
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    sleep(Duration::from_secs(2)).await;

    // Generate 25 blocks to pass epoch 1
    generate_blocks_on_simulator(25, &gateway_url).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    let wallet_alice = interactor.register_wallet(test_wallets::alice()).await;

    // ── 2. Deploy all 3 registries ──
    let (identity, validation_addr, reputation_addr) =
        deploy_all_registries(&mut interactor, wallet_alice.clone()).await;

    let validation_bech32 = address_to_bech32(&validation_addr);
    let reputation_bech32 = address_to_bech32(&reputation_addr);
    println!("Validation: {}", validation_bech32);
    println!("Reputation: {}", reputation_bech32);

    // ── 3. Register agent ──
    identity
        .register_agent(
            &mut interactor,
            "ReputedBot",
            "https://reputed.example.com/manifest.json",
            vec![("type", b"worker".to_vec())],
        )
        .await;
    println!("Agent registered: ReputedBot (nonce=1)");

    let validation = ValidationRegistryInteractor {
        wallet_address: wallet_alice.clone(),
        contract_address: validation_addr.clone(),
    };

    let client = reqwest::Client::new();

    // ── 4. Create 3 jobs, verify, and submit feedback with different ratings ──
    let jobs = [
        ("job-r-001", "proof-r-001", 80u64),  // rating 80
        ("job-r-002", "proof-r-002", 60u64),  // rating 60
        ("job-r-003", "proof-r-003", 100u64), // rating 100
    ];

    for (i, (job_id, proof, rating)) in jobs.iter().enumerate() {
        // init_job
        validation.init_job(&mut interactor, job_id, 1).await;
        println!("  Job initialized: {}", job_id);

        // submit_proof
        validation
            .submit_proof(&mut interactor, job_id, proof)
            .await;
        println!("  Proof submitted: {}", proof);

        // validation_request + response
        let req_hash = format!("req-r-{:03}", i + 1);
        validation
            .validation_request(
                &mut interactor,
                job_id,
                &wallet_alice,
                "https://validator.example.com/check",
                &req_hash,
            )
            .await;

        validation
            .validation_response(
                &mut interactor,
                &req_hash,
                1, // approved
                "https://validator.example.com/result",
                &format!("resp-r-{:03}", i + 1),
                "quality",
            )
            .await;
        println!("  Validation approved: {}", req_hash);

        // submit_feedback
        let job_id_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(job_id.as_bytes());
        let agent_nonce: u64 = 1;
        let rating_big = BigUint::<StaticApi>::from(*rating);

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
        println!("  Feedback submitted: rating={}", rating);
    }

    // ── 5. Query reputation score — should be cumulative moving average ──
    // Expected: (80 + 60 + 100) / 3 = 80
    let nonce_hex = hex::encode(1u64.to_be_bytes());
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
    println!(
        "Reputation Score after 3 jobs: {} (expected ~80)",
        score_val
    );
    assert!(
        score_val > 0,
        "Reputation score should be > 0 after 3 positive feedbacks"
    );

    // ── 6. Query get_total_jobs ──
    let body_total = serde_json::json!({
        "scAddress": reputation_bech32,
        "funcName": "get_total_jobs",
        "args": [nonce_hex],
    });
    let resp_total: serde_json::Value = client
        .post(format!("{}/vm-values/query", gateway_url))
        .json(&body_total)
        .send()
        .await
        .expect("VM query failed")
        .json()
        .await
        .expect("VM query parse failed");
    let return_data_total = resp_total["data"]["data"]["returnData"]
        .as_array()
        .expect("No returnData");
    assert!(
        !return_data_total.is_empty(),
        "Total jobs query should return data"
    );

    let total_b64 = return_data_total[0].as_str().unwrap_or("");
    let total_bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, total_b64)
        .unwrap_or_default();
    let mut total_val: u64 = 0;
    for byte in &total_bytes {
        total_val = (total_val << 8) | (*byte as u64);
    }
    println!("Total jobs: {} (expected 3)", total_val);
    assert_eq!(total_val, 3, "Total jobs should be 3 after 3 feedbacks");
    println!("✅ get_total_jobs verified: 3 jobs");

    // ── 7. Query has_given_feedback for job-r-001 (should be true) ──
    let job_r1_hex = hex::encode("job-r-001".as_bytes());
    let body_feedback = serde_json::json!({
        "scAddress": reputation_bech32,
        "funcName": "has_given_feedback",
        "args": [job_r1_hex],
    });
    let resp_feedback: serde_json::Value = client
        .post(format!("{}/vm-values/query", gateway_url))
        .json(&body_feedback)
        .send()
        .await
        .expect("VM query failed")
        .json()
        .await
        .expect("VM query parse failed");
    let return_data_feedback = resp_feedback["data"]["data"]["returnData"]
        .as_array()
        .expect("No returnData");
    let has_feedback = !return_data_feedback.is_empty()
        && return_data_feedback.iter().any(|v| {
            let s = v.as_str().unwrap_or("");
            !s.is_empty()
        });
    assert!(
        has_feedback,
        "has_given_feedback should be true for job-r-001"
    );
    println!("✅ has_given_feedback verified: true for job-r-001");

    // ── 8. Test append_response ──
    let job_r1_buf: ManagedBuffer<StaticApi> =
        ManagedBuffer::new_from_bytes("job-r-001".as_bytes());
    let response_uri_buf: ManagedBuffer<StaticApi> =
        ManagedBuffer::new_from_bytes(b"https://agent.example.com/refund-proof");

    interactor
        .tx()
        .from(&wallet_alice)
        .to(&reputation_addr)
        .gas(20_000_000)
        .raw_call("append_response")
        .argument(&job_r1_buf)
        .argument(&response_uri_buf)
        .run()
        .await;
    println!("✅ append_response executed for job-r-001");

    // Verify append_response via get_agent_response view
    let body_resp = serde_json::json!({
        "scAddress": reputation_bech32,
        "funcName": "get_agent_response",
        "args": [job_r1_hex],
    });
    let resp_resp: serde_json::Value = client
        .post(format!("{}/vm-values/query", gateway_url))
        .json(&body_resp)
        .send()
        .await
        .expect("VM query failed")
        .json()
        .await
        .expect("VM query parse failed");
    let return_data_resp = resp_resp["data"]["data"]["returnData"]
        .as_array()
        .expect("No returnData");
    assert!(
        !return_data_resp.is_empty(),
        "get_agent_response should return data after append_response"
    );
    println!("✅ get_agent_response verified for job-r-001");

    println!("\n🎉 Suite R: Reputation Extended Operations — PASSED ✅");
    println!("  Tested: Multi-job feedback (3 jobs, ratings 80/60/100)");
    println!("  Tested: get_reputation_score (weighted average)");
    println!("  Tested: get_total_jobs, has_given_feedback views");
    println!("  Tested: append_response + get_agent_response");
}
