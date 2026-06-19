use multiversx_sc::types::{BigUint, ManagedBuffer};
use multiversx_sc_snippets::imports::*;

use crate::common::{TestEnv, vm_query};

#[tokio::test]
async fn test_submit_feedback() {
    let env = TestEnv::chain_only().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
    let owner = env.owner.clone();

    let employer = interactor.register_wallet(test_wallets::bob()).await;

    // 1. Deploy All Registries
    let (identity, validation_addr, reputation_addr) =
        crate::common::deploy_all_registries(&mut interactor, owner.clone()).await;

    // Register Agent (Nonce 1)
    identity
        .register_agent(
            &mut interactor,
            "WorkerBot",
            "https://workerbot.example.com/manifest.json",
            vec![],
        )
        .await;

    // 2. Init Job, Submit Proof, Verify Job
    let job_id = "job-feedback-1";
    let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(job_id.as_bytes());
    let agent_nonce: u64 = 1;

    // Init (Employer)
    interactor
        .tx()
        .from(&employer)
        .to(&validation_addr)
        .gas(20_000_000)
        .raw_call("init_job")
        .argument(&job_id_buf)
        .argument(&agent_nonce)
        .run()
        .await;

    // Proof (Agent/Owner)
    let proof = "proof-hash-1";
    let proof_buf = ManagedBuffer::<StaticApi>::new_from_bytes(proof.as_bytes());
    interactor
        .tx()
        .from(&owner)
        .to(&validation_addr)
        .gas(20_000_000)
        .raw_call("submit_proof")
        .argument(&job_id_buf)
        .argument(&proof_buf)
        .run()
        .await;

    // Employer can submit feedback directly

    // 4. Submit Feedback (Employer) -> Rating 80
    let rating: u64 = 80;

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

    println!("Feedback Submitted: {}", rating);

    // 5. Verify Reputation Score via VM Query
    // get_reputation_score(agent_nonce) -> BigUint
    let score: BigUint<StaticApi> = vm_query(
        &mut interactor,
        &reputation_addr,
        "get_reputation_score",
        vec![ManagedBuffer::<StaticApi>::new_from_bytes(
            &agent_nonce.to_be_bytes(),
        )],
    )
    .await;
    let score_val = score.to_u64().unwrap_or(0);
    println!("Reputation Score: {}", score_val);
    assert_eq!(score_val, 80, "Score should be 80 after single feedback");

    // 6. Verify Total Jobs
    let total_jobs: u64 = vm_query(
        &mut interactor,
        &reputation_addr,
        "get_total_jobs",
        vec![ManagedBuffer::<StaticApi>::new_from_bytes(
            &agent_nonce.to_be_bytes(),
        )],
    )
    .await;
    println!("Total Jobs: {}", total_jobs);
    assert_eq!(total_jobs, 1, "Should have 1 job");
}
