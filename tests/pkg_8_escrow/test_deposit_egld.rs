use crate::common::{
    generate_random_private_key, EscrowInteractor, EscrowStatus, IdentityRegistryInteractor,
    TestEnv, ValidationRegistryInteractor,
};
use multiversx_sc_snippets::imports::*;

/// S-001: Deploy escrow → deposit EGLD → verify on-chain state
#[tokio::test]
async fn test_escrow_deposit_egld() {
    let env = TestEnv::chain_only().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
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

    let deposit_amount: u64 = 1_000_000_000_000_000_000;
    let deadline = 9_999_999_999u64;

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

    let data = escrow.get_escrow(&mut interactor, "job-001").await;
    assert_eq!(data.status, EscrowStatus::Active);
    assert_eq!(data.employer, ManagedAddress::from_address(&owner_address));
    assert_eq!(
        data.receiver,
        ManagedAddress::from_address(&receiver_address)
    );
    assert_eq!(data.deadline, deadline);
}
