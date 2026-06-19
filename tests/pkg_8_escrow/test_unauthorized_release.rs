use crate::common::{
    create_pem_file, fund_address_on_simulator, generate_random_private_key,
    EscrowInteractor, IdentityRegistryInteractor, ValidationRegistryInteractor,
};
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use tokio::time::{sleep, Duration};


/// S-005: Deposit → unauthorized release attempt → expect error
#[tokio::test]
async fn test_escrow_unauthorized_release() {
    let mut process_manager = ProcessManager::new();
    let port = process_manager
        .start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    sleep(Duration::from_secs(3)).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);

    // 1. Setup: owner deposits, attacker tries to release
    let owner_key = generate_random_private_key();
    let owner_wallet = Wallet::from_private_key(&owner_key).unwrap();
    let owner_address = owner_wallet.to_address();

    let attacker_key = generate_random_private_key();
    let attacker_wallet = Wallet::from_private_key(&attacker_key).unwrap();
    let attacker_address = attacker_wallet.to_address();

    let receiver_key = generate_random_private_key();
    let receiver_wallet = Wallet::from_private_key(&receiver_key).unwrap();
    let receiver_address = receiver_wallet.to_address();

    let pem_path = "test_escrow_unauth.pem";
    create_pem_file(
        pem_path,
        &owner_key,
        &owner_address.to_bech32("erd").to_string(),
    );
    interactor.register_wallet(owner_wallet).await;
    interactor.register_wallet(attacker_wallet).await;
    interactor.register_wallet(receiver_wallet).await;

    fund_address_on_simulator(
        &owner_address.to_bech32("erd").to_string(),
        "100000000000000000000000",
        &gateway_url,
    )
    .await;
    fund_address_on_simulator(
        &attacker_address.to_bech32("erd").to_string(),
        "10000000000000000000",
        &gateway_url,
    )
    .await;

    // 2. Deploy
    let identity = IdentityRegistryInteractor::init(&mut interactor, owner_address.clone()).await;
    let validation = ValidationRegistryInteractor::init(
        &mut interactor,
        owner_address.clone(),
        identity.address(),
    )
    .await;

    let escrow = EscrowInteractor::deploy(
        &mut interactor,
        owner_address.clone(),
        validation.address(),
        identity.address(),
    )
    .await;

    // 3. Owner deposits
    let job_id = "job-unauth-001";
    escrow
        .deposit_egld(
            &mut interactor,
            job_id,
            &receiver_address,
            "poa-unauth",
            9_999_999_999u64,
            1_000_000_000_000_000_000, // 1 EGLD
        )
        .await;

    // 4. Attacker tries to release (should fail — not the employer)
    // Create an attacker-owned escrow interactor to call release from attacker address
    let attacker_escrow = EscrowInteractor {
        wallet_address: attacker_address.clone(),
        contract_address: escrow.contract_address.clone(),
    };

    attacker_escrow
        .release_expect_err(&mut interactor, job_id, "Only the employer can call this")
        .await;

    // 5. Also test: release without job verification (should fail even for employer)
    escrow
        .release_expect_err(
            &mut interactor,
            job_id,
            "Escrow not found for this job", // validation registry hasn't init'd this job
        )
        .await;

    println!("✅ S-005 PASSED: Unauthorized release correctly rejected");

    std::fs::remove_file(pem_path).unwrap_or(());
}
