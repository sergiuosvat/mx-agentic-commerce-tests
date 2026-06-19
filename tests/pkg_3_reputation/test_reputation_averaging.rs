use multiversx_sc::types::{BigUint, ManagedBuffer};
use multiversx_sc_snippets::imports::*;

use crate::common::{deploy_all_registries, vm_query, TestEnv};

#[tokio::test]
async fn test_reputation_averaging() {
    let env = TestEnv::chain_only().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
    let owner = env.owner.clone();
    let employer = interactor.register_wallet(test_wallets::bob()).await;

    let (identity, validation_addr, reputation_addr) =
        deploy_all_registries(&mut interactor, owner.clone()).await;

    identity
        .register_agent(
            &mut interactor,
            "WorkerBot",
            "https://workerbot.example.com/manifest.json",
            vec![],
        )
        .await;

    let ratings = [80u64, 90u64, 60u64];
    let agent_nonce: u64 = 1;

    for (i, rating) in ratings.iter().enumerate() {
        let job_id = format!("job-avg-{}", i);
        let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(job_id.as_bytes());
        let zero_proof = ManagedBuffer::<StaticApi>::new_from_bytes(b"proof");

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
        interactor
            .tx()
            .from(&owner)
            .to(&validation_addr)
            .gas(10_000_000)
            .raw_call("submit_proof")
            .argument(&job_id_buf)
            .argument(&zero_proof)
            .run()
            .await;
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
    }

    let nonce_mb = ManagedBuffer::<StaticApi>::new_from_bytes(&agent_nonce.to_be_bytes());
    let score: BigUint<StaticApi> = vm_query(
        &mut interactor,
        &reputation_addr,
        "get_reputation_score",
        vec![nonce_mb.clone()],
    )
    .await;

    let score_val = score.to_u64().unwrap_or(0);
    assert_eq!(score_val, 76, "Average score mismatch");

    let total_jobs: u64 = vm_query(
        &mut interactor,
        &reputation_addr,
        "get_total_jobs",
        vec![nonce_mb],
    )
    .await;
    assert_eq!(total_jobs, 3, "Total jobs mismatch");
}

#[tokio::test]
async fn test_max_rating() {
    let env = TestEnv::chain_only().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
    let owner = env.owner.clone();
    let employer = interactor.register_wallet(test_wallets::bob()).await;

    let (identity, validation_addr, reputation_addr) =
        deploy_all_registries(&mut interactor, owner.clone()).await;

    identity
        .register_agent(&mut interactor, "MaxBot", "uri", vec![])
        .await;

    let agent_nonce: u64 = 1;
    let job_id = "job-max";
    let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(job_id.as_bytes());
    let proof_buf = ManagedBuffer::<StaticApi>::new_from_bytes(b"proof");

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
    interactor
        .tx()
        .from(&owner)
        .to(&validation_addr)
        .gas(10_000_000)
        .raw_call("submit_proof")
        .argument(&job_id_buf)
        .argument(&proof_buf)
        .run()
        .await;
    interactor
        .tx()
        .from(&employer)
        .to(&reputation_addr)
        .gas(10_000_000)
        .raw_call("giveFeedbackSimple")
        .argument(&job_id_buf)
        .argument(&agent_nonce)
        .argument(&100u64)
        .run()
        .await;

    let nonce_mb = ManagedBuffer::<StaticApi>::new_from_bytes(&agent_nonce.to_be_bytes());
    let score: BigUint<StaticApi> = vm_query(
        &mut interactor,
        &reputation_addr,
        "get_reputation_score",
        vec![nonce_mb],
    )
    .await;
    assert_eq!(
        score.to_u64().unwrap_or(0),
        100,
        "Max rating should give score 100"
    );
}

#[tokio::test]
async fn test_min_rating() {
    let env = TestEnv::chain_only().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
    let owner = env.owner.clone();
    let employer = interactor.register_wallet(test_wallets::bob()).await;

    let (identity, validation_addr, reputation_addr) =
        deploy_all_registries(&mut interactor, owner.clone()).await;

    identity
        .register_agent(&mut interactor, "MinBot", "uri", vec![])
        .await;

    let agent_nonce: u64 = 1;
    let ratings_and_jobs = [(0u64, "job-min-0"), (100u64, "job-min-1")];

    for (rating, job_id) in &ratings_and_jobs {
        let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(job_id.as_bytes());
        let proof_buf = ManagedBuffer::<StaticApi>::new_from_bytes(b"proof");

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
        interactor
            .tx()
            .from(&owner)
            .to(&validation_addr)
            .gas(10_000_000)
            .raw_call("submit_proof")
            .argument(&job_id_buf)
            .argument(&proof_buf)
            .run()
            .await;
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
    }

    let nonce_mb = ManagedBuffer::<StaticApi>::new_from_bytes(&agent_nonce.to_be_bytes());
    let score: BigUint<StaticApi> = vm_query(
        &mut interactor,
        &reputation_addr,
        "get_reputation_score",
        vec![nonce_mb.clone()],
    )
    .await;
    assert_eq!(
        score.to_u64().unwrap_or(0),
        50,
        "Average of 0 and 100 should be 50"
    );

    let total_jobs: u64 = vm_query(
        &mut interactor,
        &reputation_addr,
        "get_total_jobs",
        vec![nonce_mb],
    )
    .await;
    assert_eq!(total_jobs, 2, "Total jobs should be 2");
}
