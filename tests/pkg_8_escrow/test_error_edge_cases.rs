use multiversx_sc::types::ManagedBuffer;
use multiversx_sc_snippets::imports::*;

use crate::common::{TestEnv, EscrowInteractor, EscrowStatus};

/// S-007: Edge case error testing
/// - Duplicate deposit on same job → "Escrow already exists"
/// - Release without init_job in validation → "Escrow not found" (cross-contract check re-uses same error)
/// - Release after init_job but before verification → "Job must be verified before release"
/// - Escrow remains Active despite all error attempts
#[tokio::test]
async fn test_escrow_error_edge_cases() {
    let env = TestEnv::chain_only().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
    let owner = env.owner.clone();

    let receiver = interactor.register_wallet(test_wallets::bob()).await;

    // Deploy dependencies + escrow
    let (identity, validation_addr, ..) =
        crate::common::deploy_all_registries(&mut interactor, owner.clone()).await;

    identity
        .register_agent(&mut interactor, "TestAgent", "https://test.ai", vec![])
        .await;
    let agent_nonce: u64 = 1;

    let escrow = EscrowInteractor::deploy(
        &mut interactor,
        owner.clone(),
        &validation_addr,
        identity.address(),
    )
    .await;

    // 1. Deposit 1 EGLD for job "job-edge-001"
    let job_id = "job-edge-001";
    let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(job_id.as_bytes());
    let receiver_addr: ManagedAddress<StaticApi> = ManagedAddress::from_address(&receiver);
    let poa_buf = ManagedBuffer::<StaticApi>::new_from_bytes(b"poa-edge");
    let deadline: u64 = 9_999_999_999;

    interactor
        .tx()
        .from(&owner)
        .to(escrow.address())
        .gas(600_000_000)
        .egld(1_000_000_000_000_000_000u64)
        .raw_call("deposit")
        .argument(&job_id_buf)
        .argument(&receiver_addr)
        .argument(&poa_buf)
        .argument(&deadline)
        .run()
        .await;

    let data = escrow.get_escrow(&mut interactor, job_id).await;
    assert_eq!(data.status, EscrowStatus::Active);
    println!("✓ Deposit stored and Active");

    // 2. Duplicate deposit on same job must fail
    interactor
        .tx()
        .from(&owner)
        .to(escrow.address())
        .gas(600_000_000)
        .egld(1_000_000_000_000_000_000u64)
        .raw_call("deposit")
        .argument(&job_id_buf)
        .argument(&receiver_addr)
        .argument(&poa_buf)
        .argument(&deadline)
        .returns(ExpectError(4, "Escrow already exists for this job"))
        .run()
        .await;
    println!("✓ Duplicate deposit correctly rejected");

    // 3. Release without init_job → cross-contract job check fails with "Escrow not found"
    //    (the escrow release function re-uses ERR_ESCROW_NOT_FOUND for both escrow and job checks)
    interactor
        .tx()
        .from(&owner)
        .to(escrow.address())
        .gas(600_000_000)
        .raw_call("release")
        .argument(&job_id_buf)
        .returns(ExpectError(4, "Escrow not found for this job"))
        .run()
        .await;
    println!("✓ Release without init_job correctly rejected");

    // 4. Init job in validation (but DON'T verify it)
    interactor
        .tx()
        .from(&owner)
        .to(&validation_addr)
        .gas(10_000_000)
        .raw_call("init_job")
        .argument(&job_id_buf)
        .argument(&agent_nonce)
        .run()
        .await;
    println!("✓ Job initialized (no verification)");

    // 5. Release with unverified job → "Job must be verified before release"
    interactor
        .tx()
        .from(&owner)
        .to(escrow.address())
        .gas(600_000_000)
        .raw_call("release")
        .argument(&job_id_buf)
        .returns(ExpectError(4, "Job must be verified before release"))
        .run()
        .await;
    println!("✓ Unverified release correctly rejected");

    // 6. Escrow still Active
    let data_final = escrow.get_escrow(&mut interactor, job_id).await;
    assert_eq!(data_final.status, EscrowStatus::Active);
    println!("✓ Escrow remains Active after all error attempts");

    println!("✅ S-007 PASSED: Edge case errors handled correctly");
}
