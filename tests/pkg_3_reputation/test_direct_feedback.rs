// ERC-8004: Feedback is submitted directly.
// Feedback is now submitted directly by the employer without authorization.
// This test validates that the employer can submit feedback without

use multiversx_sc::types::ManagedBuffer;
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use crate::common::wait_for_simulator_ready;


#[tokio::test]
async fn test_feedback_without_authorization() {
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

    identity
        .register_agent(
            &mut interactor,
            "WorkerBot",
            "https://workerbot.example.com/manifest.json",
            vec![],
        )
        .await;

    // 2. Init Job (Employer)
    let job_id = "job-auth-test";
    let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(job_id.as_bytes());
    let agent_nonce: u64 = 1;
    let agent_nonce_buf = ManagedBuffer::<StaticApi>::new_from_bytes(&agent_nonce.to_be_bytes());

    interactor
        .tx()
        .from(&employer)
        .to(&validation_addr)
        .gas(20_000_000)
        .raw_call("init_job")
        .argument(&job_id_buf)
        .argument(&agent_nonce_buf)
        .run()
        .await;

    // 3. Submit Proof (Agent/Owner)
    let proof_buf = ManagedBuffer::<StaticApi>::new_from_bytes(b"proof-hash-1");
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

    // 4. Submit Feedback directly (Employer)
    let rating: u64 = 85;
    interactor
        .tx()
        .from(&employer)
        .to(&reputation_addr)
        .gas(10_000_000)
        .raw_call("giveFeedbackSimple")
        .argument(&job_id_buf)
        .argument(&agent_nonce_buf)
        .argument(&rating)
        .run()
        .await;

    println!("Feedback submitted without authorization — ERC-8004 compliant");
}
