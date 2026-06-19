use crate::common::{
    create_pem_file, fund_address_on_simulator, generate_random_private_key,
    EscrowInteractor, EscrowStatus, IdentityRegistryInteractor, ValidationRegistryInteractor,
};
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use tokio::time::{sleep, Duration};


/// S-001: Deploy escrow → deposit EGLD → verify on-chain state
#[tokio::test]
async fn test_escrow_deposit_egld() {
    let mut process_manager = ProcessManager::new();
    let port = process_manager
        .start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    sleep(Duration::from_secs(3)).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);

    // 1. Setup Wallets
    let owner_key = generate_random_private_key();
    let owner_wallet = Wallet::from_private_key(&owner_key).unwrap();
    let owner_address = owner_wallet.to_address();

    let receiver_key = generate_random_private_key();
    let receiver_wallet = Wallet::from_private_key(&receiver_key).unwrap();
    let receiver_address = receiver_wallet.to_address();

    let pem_path = "test_escrow_deposit.pem";
    create_pem_file(
        pem_path,
        &owner_key,
        &owner_address.to_bech32("erd").to_string(),
    );
    interactor.register_wallet(owner_wallet).await;
    interactor.register_wallet(receiver_wallet).await;

    fund_address_on_simulator(
        &owner_address.to_bech32("erd").to_string(),
        "100000000000000000000000",
        &gateway_url,
    )
    .await;

    // 2. Deploy Identity & Validation (required by escrow)
    let identity = IdentityRegistryInteractor::init(&mut interactor, owner_address.clone()).await;
    let validation = ValidationRegistryInteractor::init(
        &mut interactor,
        owner_address.clone(),
        identity.address(),
    )
    .await;

    // 3. Deploy Escrow
    let escrow = EscrowInteractor::deploy(
        &mut interactor,
        owner_address.clone(),
        validation.address(),
        identity.address(),
    )
    .await;

    // 4. Deposit EGLD
    let deposit_amount: u64 = 1_000_000_000_000_000_000; // 1 EGLD
    let deadline = 9_999_999_999u64; // Far future

    escrow
        .deposit_egld(
            &mut interactor,
            "job-001",
            &receiver_address,
            "poa-hash-001",
            deadline,
            deposit_amount,
        )
        .await;

    // 5. Verify on-chain state
    let data = escrow.get_escrow(&mut interactor, "job-001").await;
    assert_eq!(data.status, EscrowStatus::Active);
    assert_eq!(data.employer, ManagedAddress::from_address(&owner_address));
    assert_eq!(
        data.receiver,
        ManagedAddress::from_address(&receiver_address)
    );
    assert_eq!(data.deadline, deadline);

    println!("✅ S-001 PASSED: Escrow deposit EGLD verified on-chain");

    // Cleanup
    std::fs::remove_file(pem_path).unwrap_or(());
}
