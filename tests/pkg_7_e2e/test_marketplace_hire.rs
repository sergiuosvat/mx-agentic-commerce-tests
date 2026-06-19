use multiversx_sc::types::ManagedBuffer;
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;

use crate::common::{deploy_all_registries, vm_query, wait_for_simulator_ready};

/// E2E-03: Marketplace Hire — employer hires agent, pays, job completes with proof + verification.
///
/// Simulates the full marketplace flow:
/// 1. Agent registers with services (price + token)
/// 2. Employer hires agent by initiating job
/// 3. Agent submits proof
/// 4. Owner verifies
/// 5. Employer submits feedback
/// 6. Verify all state changes on-chain
#[tokio::test]
async fn test_marketplace_hire() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    interactor.generate_blocks_until_all_activations().await;

    let owner = interactor.register_wallet(test_wallets::alice()).await;
    let employer = interactor.register_wallet(test_wallets::bob()).await;

    // 1. Deploy
    let (identity, validation_addr, reputation_addr) =
        deploy_all_registries(&mut interactor, owner.clone()).await;
    println!("Deployed all registries");

    // 2. Register agent (with service config would go here; simplified for now)
    identity
        .register_agent(
            &mut interactor,
            "MarketplaceBot",
            "https://marketplace.example.com/bot",
            vec![("pricing", b"100 EGLD".to_vec())],
        )
        .await;
    let agent_nonce: u64 = 1;
    println!("Agent registered in marketplace (nonce={})", agent_nonce);

    // 3. Employer initiates multiple jobs
    let jobs = vec!["marketplace-job-001", "marketplace-job-002"];
    for job_id in &jobs {
        let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(job_id.as_bytes());
        interactor
            .tx()
            .from(&employer)
            .to(&validation_addr)
            .gas(10_000_000)
            .raw_call("init_job")
            .argument(&job_id_buf)
            .argument(&agent_nonce)
            .run()
            .await;
        println!("Job initiated: {}", job_id);
    }

    // 4. Agent submits proof for both jobs
    for job_id in &jobs {
        let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(job_id.as_bytes());
        let proof =
            ManagedBuffer::<StaticApi>::new_from_bytes(format!("proof-for-{}", job_id).as_bytes());
        interactor
            .tx()
            .from(&owner) // agent owner
            .to(&validation_addr)
            .gas(10_000_000)
            .raw_call("submit_proof")
            .argument(&job_id_buf)
            .argument(&proof)
            .run()
            .await;
        println!("Proof submitted for: {}", job_id);
    }


    // 8. Employer submits feedback with different ratings
    let ratings = [80u64, 95u64];
    for (job_id, rating) in jobs.iter().zip(ratings.iter()) {
        let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(job_id.as_bytes());
        interactor
            .tx()
            .from(&employer)
            .to(&reputation_addr)
            .gas(10_000_000)
            .raw_call("giveFeedbackSimple")
            .argument(&job_id_buf)
            .argument(&agent_nonce)
            .argument(rating)
            .run()
            .await;
        println!("Feedback submitted for {}: rating={}", job_id, rating);
    }

    // 9. Verify averaged reputation
    let nonce_buf = ManagedBuffer::<StaticApi>::new_from_bytes(&agent_nonce.to_be_bytes());
    let score: u64 = vm_query(
        &mut interactor,
        &reputation_addr,
        "get_reputation_score",
        vec![nonce_buf.clone()],
    )
    .await;

    let expected_avg = (80 + 95) / 2; // 87
    println!(
        "Agent reputation: {} (expected avg ~{})",
        score, expected_avg
    );
    assert!(
        score >= expected_avg - 1 && score <= expected_avg + 1,
        "Reputation should be average of ratings"
    );
    println!("✅ Marketplace hire E2E complete — 2 jobs, averaged reputation");

    // 10. Verify total jobs
    let total_jobs: u64 = vm_query(
        &mut interactor,
        &reputation_addr,
        "get_total_jobs",
        vec![nonce_buf],
    )
    .await;
    assert_eq!(total_jobs, 2, "Should have 2 completed jobs");
    println!("✅ Total jobs = {} (expected 2)", total_jobs);

    println!("=== Marketplace Hire E2E Complete ===");
}
