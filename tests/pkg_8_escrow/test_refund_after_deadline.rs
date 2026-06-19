use crate::common::{
    generate_random_private_key, EscrowInteractor, EscrowStatus, IdentityRegistryInteractor,
    TestEnv, ValidationRegistryInteractor,
};
use multiversx_sc_snippets::imports::*;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{sleep, Duration};

/// S-004: Deposit → deadline passes → refund → verify depositor refunded
#[tokio::test]
async fn test_escrow_refund_after_deadline() {
    let env = TestEnv::chain_only().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
    let gateway_url = env.gateway_url.clone();
    let owner_address = env.owner.clone();

    let receiver_key = generate_random_private_key();
    let receiver_wallet = Wallet::from_private_key(&receiver_key).unwrap();
    let receiver_address = receiver_wallet.to_address();
    interactor.register_wallet(receiver_wallet).await;

    // 2. Deploy contracts
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

    // 3. Use a deadline far enough in the future from "now" that it will be
    //    ahead of the on-chain timestamp after all deploys (~50 rounds * 6s = ~300s).
    //    Then we'll generate 200+ blocks (~1200s) to push past it.
    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    // Deadline = now + 600s (10 min). By this point the chain is at ~now+300s.
    // After deposit, we'll generate 200 blocks → chain advances to ~now+1500s > deadline.
    let near_future_deadline = now_unix + 600;

    let job_id = "job-refund-001";
    let deposit_amount: u64 = 2_000_000_000_000_000_000; // 2 EGLD

    escrow
        .deposit_egld(
            &mut interactor,
            job_id,
            &receiver_address,
            "poa-refund-hash",
            near_future_deadline,
            deposit_amount,
        )
        .await;

    println!(
        "Deposited successfully with deadline = {}",
        near_future_deadline
    );

    // 4. Try refund BEFORE deadline (should fail — chain is still before deadline)
    escrow
        .refund_expect_err(
            &mut interactor,
            &owner_address,
            job_id,
            "Deadline has not passed yet",
        )
        .await;

    println!("Pre-deadline refund correctly rejected");

    // 5. Generate 200 blocks to advance on-chain timestamp past the deadline.
    //    Each block advances ~6 seconds → 200 * 6 = 1200 seconds advancement.
    //    Chain will be at ~now + 300 + 1200 = now + 1500 >> deadline (now + 600).
    let client = reqwest::Client::new();
    client
        .post(format!("{}/simulator/generate-blocks/200", gateway_url))
        .send()
        .await
        .ok();
    sleep(Duration::from_millis(1000)).await;

    println!("Generated 200 blocks to advance past deadline");

    // 6. Now refund should succeed (deadline passed)
    escrow.refund(&mut interactor, &owner_address, job_id).await;

    // 7. Verify escrow state is Refunded
    let data = escrow.get_escrow(&mut interactor, job_id).await;
    assert_eq!(data.status, EscrowStatus::Refunded);

    println!("✅ S-004 PASSED: Escrow refund after deadline verified");
}
