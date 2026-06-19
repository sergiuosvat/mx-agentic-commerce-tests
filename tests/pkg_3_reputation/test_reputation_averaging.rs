use multiversx_sc::types::{BigUint, ManagedBuffer};
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;

use crate::common::{vm_query, wait_for_simulator_ready};

#[tokio::test]
async fn test_reputation_averaging() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    let owner = interactor.register_wallet(test_wallets::alice()).await;
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

    // 2. Complete 3 Jobs
    // Ratings: 80, 90, 60
    // Avg = floor((80 + 90 + 60) / 3) = 76

    let ratings = [80u64, 90u64, 60u64];
    let agent_nonce: u64 = 1;

    for (i, rating) in ratings.iter().enumerate() {
        let job_id = format!("job-avg-{}", i);
        let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(job_id.as_bytes());
        let zero_proof = ManagedBuffer::<StaticApi>::new_from_bytes(b"proof");

        // Init
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
        // Proof
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
        // Submit
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

    // 3. Verify Average Reputation Score
    let nonce_mb = ManagedBuffer::<StaticApi>::new_from_bytes(&agent_nonce.to_be_bytes());
    let score: BigUint<StaticApi> = vm_query(
        &mut interactor,
        &reputation_addr,
        "get_reputation_score",
        vec![nonce_mb.clone()],
    )
    .await;

    let score_val = score.to_u64().unwrap_or(0);
    println!("Final Reputation Score: {}", score_val);

    // Expected: (80 + 90 + 60) / 3 = 230 / 3 = 76.66 -> 76 (integer division)
    assert_eq!(score_val, 76, "Average score mismatch");

    // 4. Verify Total Jobs count
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
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    let owner = interactor.register_wallet(test_wallets::alice()).await;
    let employer = interactor.register_wallet(test_wallets::bob()).await;

    let (identity, validation_addr, reputation_addr) =
        crate::common::deploy_all_registries(&mut interactor, owner.clone()).await;

    identity
        .register_agent(&mut interactor, "MaxBot", "uri", vec![])
        .await;

    let agent_nonce: u64 = 1;
    let job_id = "job-max";
    let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(job_id.as_bytes());
    let proof_buf = ManagedBuffer::<StaticApi>::new_from_bytes(b"proof");

    // Complete job lifecycle
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

    // Submit max rating: 100
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

    // Verify score = 100
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
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    let owner = interactor.register_wallet(test_wallets::alice()).await;
    let employer = interactor.register_wallet(test_wallets::bob()).await;

    let (identity, validation_addr, reputation_addr) =
        crate::common::deploy_all_registries(&mut interactor, owner.clone()).await;

    identity
        .register_agent(&mut interactor, "MinBot", "uri", vec![])
        .await;

    let agent_nonce: u64 = 1;

    // Two jobs: first rating = 0, second rating = 100
    // Expected: after job-1 (rating=0): score = 0
    //           after job-2 (rating=100): score = (0*1 + 100) / 2 = 50
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

    // Verify score after 0 then 100 = average(0, 100) = 50
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
