use crate::common::{
    generate_blocks_on_simulator, generate_random_private_key, issue_fungible_esdt,
    EscrowInteractor, EscrowStatus, IdentityRegistryInteractor, TestEnv,
    ValidationRegistryInteractor,
};
use multiversx_sc_snippets::imports::*;

/// S-002: Deposit ESDT token → verify on-chain escrow state (mirrors mx-8004 `test_deposit_esdt`).
#[tokio::test]
async fn test_escrow_deposit_esdt() {
    let env = TestEnv::chain_only().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
    let gateway_url = env.gateway_url.clone();
    let owner_address = env.owner.clone();

    let receiver_key = generate_random_private_key();
    let receiver_wallet = Wallet::from_private_key(&receiver_key).unwrap();
    let receiver_address = receiver_wallet.to_address();
    interactor.register_wallet(receiver_wallet).await;

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

    // ESDT issuance is disabled at epoch 0 on the simulator
    generate_blocks_on_simulator(25, &gateway_url).await;

    let token_id = issue_fungible_esdt(
        &mut interactor,
        &owner_address,
        "EscrowToken",
        "ESCT",
        1_000_000,
        6,
        &gateway_url,
    )
    .await;
    generate_blocks_on_simulator(10, &gateway_url).await;

    let deposit_amount: u64 = 1_000;
    let deadline = 9_999_999_999u64;

    escrow
        .deposit_esdt(
            &mut interactor,
            "job-esdt-001",
            &receiver_address,
            "poa-esdt-hash",
            deadline,
            &token_id,
            deposit_amount,
        )
        .await;

    let data = escrow.get_escrow(&mut interactor, "job-esdt-001").await;
    assert_eq!(data.status, EscrowStatus::Active);
    assert_eq!(data.amount, BigUint::from(deposit_amount));
    assert_eq!(
        data.token_id,
        EgldOrEsdtTokenIdentifier::esdt(token_id.as_str())
    );
    assert_eq!(
        data.receiver,
        ManagedAddress::from_address(&receiver_address)
    );

    println!("✅ S-002 PASSED: Escrow ESDT deposit verified");
}
