use multiversx_sc::types::ManagedBuffer;
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;

use crate::common::{deploy_all_registries, wait_for_simulator_ready};

#[tokio::test]
async fn test_full_agent_lifecycle() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    let owner = interactor.register_wallet(test_wallets::alice()).await;
    let employer = interactor.register_wallet(test_wallets::bob()).await;

    // 1. Deploy Infrastructure
    let (identity, validation_addr, reputation_addr) =
        deploy_all_registries(&mut interactor, owner.clone()).await;

    println!("Deployed all registries");

    // 2. Register Agent
    identity
        .register_agent(
            &mut interactor,
            "FullLifecycleBot",
            "https://bot.io",
            vec![],
        )
        .await;

    let agent_nonce: u64 = 1;

    // 3. Init Job (Validation)
    let job_id = "job-e2e-01";
    let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(job_id.as_bytes());

    // Employer inits job
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

    println!("Job Initialized");

    // 4. Submit Proof (Validation) by Agent Owner
    let proof = ManagedBuffer::<StaticApi>::new_from_bytes(b"e2e-proof");
    interactor
        .tx()
        .from(&owner)
        .to(&validation_addr)
        .gas(10_000_000)
        .raw_call("submit_proof")
        .argument(&job_id_buf)
        .argument(&proof)
        .run()
        .await;

    println!("Proof Submitted");


    // 7. Submit Feedback (Reputation) by Employer
    let rating: u64 = 95;

    interactor
        .tx()
        .from(&employer)
        .to(&reputation_addr)
        .gas(10_000_000)
        .raw_call("giveFeedbackSimple")
        .argument(&job_id_buf)
        .argument(&agent_nonce)
        .argument(&rating)
        .run()
        .await;

    println!("Feedback Submitted");

    // 8. Verify Score
    let nonce_mb = ManagedBuffer::<StaticApi>::new_from_bytes(&agent_nonce.to_be_bytes());
    let score: u64 = crate::common::vm_query(
        &mut interactor,
        &reputation_addr,
        "get_reputation_score",
        vec![nonce_mb],
    )
    .await;

    println!("Reputation Score: {}", score);
    assert!(score > 0, "Score should be positive");
    assert_eq!(score, 95, "Score should be 95 (first feedback)"); // 95 is what we submitted

    println!("Full E2E Lifecycle Complete");
}
