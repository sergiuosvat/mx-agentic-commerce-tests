use crate::common::{
    fund_address_on_simulator, generate_random_private_key, EscrowInteractor,
    IdentityRegistryInteractor, TestEnv, ValidationRegistryInteractor,
};
use multiversx_sc_snippets::imports::*;

/// S-005: Deposit → unauthorized release attempt → expect error
#[tokio::test]
async fn test_escrow_unauthorized_release() {
    let env = TestEnv::chain_only().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
    let gateway_url = env.gateway_url.clone();
    let owner_address = env.owner.clone();

    let attacker_key = generate_random_private_key();
    let attacker_wallet = Wallet::from_private_key(&attacker_key).unwrap();
    let attacker_address = attacker_wallet.to_address();

    let receiver_key = generate_random_private_key();
    let receiver_wallet = Wallet::from_private_key(&receiver_key).unwrap();
    let receiver_address = receiver_wallet.to_address();
    interactor.register_wallet(attacker_wallet).await;
    interactor.register_wallet(receiver_wallet).await;

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
}
