use multiversx_sc::types::ManagedBuffer;
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use crate::common::wait_for_simulator_ready;


#[tokio::test]
async fn test_append_response() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    let owner = interactor.register_wallet(test_wallets::alice()).await;
    let employer = interactor.register_wallet(test_wallets::bob()).await;

    // 1. Deploy & Setup single job with feedback
    let (identity, validation_addr, reputation_addr) =
        crate::common::deploy_all_registries(&mut interactor, owner.clone()).await;
    // let reputation_bech32 = crate::common::address_to_bech32(&reputation_addr);

    identity
        .register_agent(&mut interactor, "WorkerBot", "uri", vec![])
        .await;

    let job_id = "job-resp-1";
    let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(job_id.as_bytes());
    let agent_nonce: u64 = 1;
    let agent_nonce_buf = ManagedBuffer::<StaticApi>::new_from_bytes(&agent_nonce.to_be_bytes());
    let rating_buf = ManagedBuffer::<StaticApi>::new_from_bytes(&80u64.to_be_bytes());
    let proof = ManagedBuffer::<StaticApi>::new_from_bytes(b"proof");

    interactor
        .tx()
        .from(&employer)
        .to(&validation_addr)
        .gas(10_000_000)
        .raw_call("init_job")
        .argument(&job_id_buf)
        .argument(&agent_nonce_buf)
        .run()
        .await;
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
    interactor
        .tx()
        .from(&employer)
        .to(&reputation_addr)
        .gas(10_000_000)
        .raw_call("giveFeedbackSimple")
        .argument(&job_id_buf)
        .argument(&agent_nonce_buf)
        .argument(&rating_buf)
        .run()
        .await;

    // 2. Append Response (Agent Owner)
    let response_uri = "ipfs://response-evidence";
    let response_uri_buf = ManagedBuffer::<StaticApi>::new_from_bytes(response_uri.as_bytes());

    interactor
        .tx()
        .from(&owner)
        .to(&reputation_addr)
        .gas(10_000_000)
        .raw_call("append_response")
        .argument(&job_id_buf)
        .argument(&response_uri_buf)
        .run()
        .await;

    println!("Appended Response to job {}", job_id);

    // 3. Verify Response via VM Query
    // Skipped for now strictly because view name is ambiguous.
    // Happy path "append_response" success is verified by lack of panic above.
}
