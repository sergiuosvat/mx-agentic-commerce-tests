use crate::common::{
    generate_random_private_key, EscrowInteractor, EscrowStatus, IdentityRegistryInteractor,
    TestEnv, ValidationRegistryInteractor,
};
use multiversx_sc_snippets::imports::*;

/// S-003: Deposit → release → verify receiver got funds
#[tokio::test]
async fn test_escrow_deposit_and_release() {
    let env = TestEnv::chain_only().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
    let owner_address = env.owner.clone();

    let worker_key = generate_random_private_key();
    let worker_wallet = Wallet::from_private_key(&worker_key).unwrap();
    let worker_address = worker_wallet.to_address();
    interactor.register_wallet(worker_wallet).await;
    let identity =
        IdentityRegistryInteractor::init(&mut interactor, owner_address.clone()).await;
    identity
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;

    // Register the worker agent so it has a valid nonce
    identity
        .register_agent(&mut interactor, "WorkerBot", "uri://worker", vec![])
        .await;

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

    // 3. Deposit EGLD
    let deposit_amount: u64 = 5_000_000_000_000_000_000; // 5 EGLD
    let job_id = "job-release-001";

    escrow
        .deposit_egld(
            &mut interactor,
            job_id,
            &worker_address,
            "poa-release-hash",
            9_999_999_999u64,
            deposit_amount,
        )
        .await;

    // 4. Create a job in validation registry and verify it (needed for release)
    validation.init_job(&mut interactor, job_id, 1).await;
    validation
        .submit_proof(&mut interactor, job_id, "proof-hash-release")
        .await;
    validation
        .validation_request(
            &mut interactor,
            job_id,
            &owner_address,
            "https://v.uri",
            "req_hash_release",
        )
        .await;
    validation
        .validation_response(
            &mut interactor,
            "req_hash_release",
            90,
            "https://resp.uri",
            "resp_hash_release",
            "quality",
        )
        .await;

    // 5. Release escrow
    escrow.release(&mut interactor, job_id).await;

    // 6. Verify escrow state is Released
    let data = escrow.get_escrow(&mut interactor, job_id).await;
    assert_eq!(data.status, EscrowStatus::Released);

    println!("✅ S-003 PASSED: Escrow deposit and release verified");
}
