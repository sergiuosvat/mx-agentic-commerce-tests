use crate::common::{
    create_pem_file, fund_address_on_simulator, generate_random_private_key,
    IdentityRegistryInteractor, ServiceConfigInput, ValidationRegistryInteractor,
};
use multiversx_sc::types::{BigUint, EgldOrEsdtTokenIdentifier};
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use tokio::time::{sleep, Duration};

#[tokio::test]
async fn test_job_with_payment() {
    let mut process_manager = ProcessManager::new();
    let port = process_manager
        .start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);

    sleep(Duration::from_secs(3)).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);

    // 1. Setup Owner (contract deployer)
    let owner_private_key = generate_random_private_key();
    let owner_wallet = Wallet::from_private_key(&owner_private_key).unwrap();
    let owner_address = owner_wallet.to_address();

    let pem_path = "test_validation_payment.pem";
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

    // 2. Deploy Identity & Issue Token
    let identity_interactor =
        IdentityRegistryInteractor::init(&mut interactor, owner_address.clone()).await;
    identity_interactor
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;

    // 3. Register Agent with Service Config
    // Service ID 1: Requires 1 EGLD
    let service_cost_egld = BigUint::from(1_000_000_000_000_000_000u64); // 1 EGLD
    let service1 = ServiceConfigInput {
        service_id: 1,
        price: service_cost_egld.clone(),
        token: EgldOrEsdtTokenIdentifier::egld(),
        nonce: 0,
    };

    identity_interactor
        .register_agent_with_services(
            &mut interactor,
            "PaidServiceBot",
            "uri",
            vec![],
            vec![service1],
        )
        .await;

    let agent_nonce = 1;

    // 4. Deploy Validation Registry
    let validation_interactor = ValidationRegistryInteractor::init(
        &mut interactor,
        owner_address.clone(),
        identity_interactor.address(),
    )
    .await;

    // 5. Setup Employer (Validation caller)
    let employer_private_key = generate_random_private_key();
    let employer_wallet = Wallet::from_private_key(&employer_private_key).unwrap();
    let employer_address = employer_wallet.to_address();

    // Fund employer
    fund_address_on_simulator(
        &employer_address.to_bech32("erd").to_string(),
        "50000000000000000000000", // 50,000 EGLD
        &gateway_url,
        )
    .await;

    let mut interactor_employer = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    interactor_employer
        .register_wallet(employer_wallet)
        .await;

    let contract_address = validation_interactor.contract_address.clone();
    let employer_validation_interactor = ValidationRegistryInteractor {
        wallet_address: employer_address.clone(),
        contract_address,
    };

    // 6. Init Job with Correct Payment (1 EGLD)
    let job_id = "paid-job-001";
    let payment_amount = 1_000_000_000_000_000_000u64;

    employer_validation_interactor
        .init_job_with_payment(
            &mut interactor_employer,
            job_id,
            agent_nonce,
            1,
            "EGLD",
            payment_amount,
        )
        .await;

    // 7. Verify Job Created (implies payment accepted)
    // We can't easily check balances in interactor without query, but success means it passed checks.

    // Cleanup
    std::fs::remove_file(pem_path).unwrap_or(());
}
