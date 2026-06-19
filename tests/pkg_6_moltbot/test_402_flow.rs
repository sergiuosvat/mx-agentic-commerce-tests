use multiversx_sc::types::ManagedBuffer;
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;

use crate::common::{deploy_all_registries, vm_query, wait_for_simulator_ready};

/// Test the full "proof & reputation" flow: init_job → submit_proof → feedback (ERC-8004).
#[tokio::test]
async fn test_proof_and_reputation_flow() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    interactor.generate_blocks_until_all_activations().await;

    let owner = interactor.register_wallet(test_wallets::alice()).await;
    let employer = interactor.register_wallet(test_wallets::bob()).await;

    // 1. Deploy all registries
    let (identity, validation_addr, reputation_addr) =
        deploy_all_registries(&mut interactor, owner.clone()).await;
    println!("Registries deployed");

    // 2. Register agent (owner)
    identity
        .register_agent(
            &mut interactor,
            "ProofTestBot",
            "https://example.com/proofbot",
            vec![],
        )
        .await;
    let agent_nonce: u64 = 1;
    println!("Agent registered (nonce={})", agent_nonce);

    // 3. Init job by employer
    let job_id = "job-moltbot-402";
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
    println!("Job initialized: {}", job_id);

    // 4. Submit proof by agent owner
    let proof = ManagedBuffer::<StaticApi>::new_from_bytes(b"moltbot-proof-hash-001");
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
    println!("Proof submitted");


    let rating: u64 = 85;
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
    println!("Feedback submitted (rating={})", rating);

    // 8. Verify reputation
    let nonce_buf = ManagedBuffer::<StaticApi>::new_from_bytes(&agent_nonce.to_be_bytes());
    let score: u64 = vm_query(
        &mut interactor,
        &reputation_addr,
        "get_reputation_score",
        vec![nonce_buf],
    )
    .await;

    assert!(score > 0, "Reputation should be positive after feedback");
    assert_eq!(score, rating, "First feedback = exact score");
    println!("✅ Reputation score = {} (expected {})", score, rating);

    println!("=== Proof & Reputation Flow Complete ===");
}
