use crate::common::{
    create_pem_file, fund_address_on_simulator, generate_random_private_key,
    IdentityRegistryInteractor, ValidationRegistryInteractor,
};
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use tokio::time::{sleep, Duration};


#[tokio::test]
async fn test_job_lifecycle() {
    let mut process_manager = ProcessManager::new();
    let port = process_manager
        .start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);

    sleep(Duration::from_secs(3)).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);

    // 1. Setup Wallet
    let owner_private_key = generate_random_private_key();
    let owner_wallet = Wallet::from_private_key(&owner_private_key).unwrap();
    let owner_address = owner_wallet.to_address();

    let pem_path = "test_validation_lifecycle.pem";
    create_pem_file(
        pem_path,
        &owner_private_key,
        &owner_address.to_bech32("erd").to_string(),
    );
    interactor.register_wallet(owner_wallet).await;

    fund_address_on_simulator(
        &owner_address.to_bech32("erd").to_string(),
        "100000000000000000000000",
        &gateway_url,
    )
    .await;

    // 2. Deploy Identity Registry & Issue Token
    let identity_interactor =
        IdentityRegistryInteractor::init(&mut interactor, owner_address.clone()).await;
    identity_interactor
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;

    // 3. Register Agent (to be referenced in job)
    identity_interactor
        .register_agent(&mut interactor, "WorkerBot", "uri", vec![])
        .await;

    let agent_nonce = 1;

    // 4. Deploy Validation Registry
    let validation_interactor = ValidationRegistryInteractor::init(
        &mut interactor,
        owner_address.clone(),
        identity_interactor.address(),
    )
    .await;

    // 5. Init Job (No Payment)
    let job_id = "job-001";
    validation_interactor
        .init_job(&mut interactor, job_id, agent_nonce)
        .await;

    // 6. Submit Proof
    let proof_hash = "QmProofHash123";
    validation_interactor
        .submit_proof(&mut interactor, job_id, proof_hash)
        .await;

    // 7. Validation Request + Response (ERC-8004)
    validation_interactor
        .validation_request(
            &mut interactor,
            job_id,
            &owner_address,
            "https://val.uri",
            "req_hash_001",
        )
        .await;
    validation_interactor
        .validation_response(
            &mut interactor,
            "req_hash_001",
            80,
            "https://resp.uri",
            "resp_hash_001",
            "quality",
        )
        .await;

    // Cleanup
    std::fs::remove_file(pem_path).unwrap_or(());
}
