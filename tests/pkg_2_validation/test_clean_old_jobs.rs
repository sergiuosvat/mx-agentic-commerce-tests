use crate::common::{
    advance_simulator_days, IdentityRegistryInteractor,
    ValidationRegistryInteractor, ONE_DAY_ROUND_DURATION_MS, TestEnv,
};
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ChainSimulatorOptions;

/// Exercises `clean_old_jobs` on the chain simulator by starting with a 1-day
/// round duration, then generating four blocks (>3 day threshold).
#[tokio::test]
async fn test_clean_old_jobs() {
    let env = TestEnv::chain_only_with_options(ChainSimulatorOptions::with_round_duration_ms(
        ONE_DAY_ROUND_DURATION_MS,
    ))
    .await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
    let gateway_url = env.gateway_url.clone();
    let owner_address = env.owner.clone();

    let identity_interactor =
        IdentityRegistryInteractor::init(&mut interactor, owner_address.clone()).await;
    identity_interactor
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;

    let validation_interactor = ValidationRegistryInteractor::init(
        &mut interactor,
        owner_address.clone(),
        identity_interactor.address(),
    )
    .await;

    let old_job_id = "job-old";
    validation_interactor
        .init_job(&mut interactor, old_job_id, 1)
        .await;

    // Four blocks × 1 day/block = 4 simulated days (> 3 day cleanup threshold).
    advance_simulator_days(4, &gateway_url).await;

    let new_job_id = "job-new";
    validation_interactor
        .init_job(&mut interactor, new_job_id, 1)
        .await;

    validation_interactor
        .clean_old_jobs(&mut interactor, vec![old_job_id, new_job_id])
        .await;

    // Old job removed — re-init should succeed.
    validation_interactor
        .init_job(&mut interactor, old_job_id, 1)
        .await;

    // Recent job kept — duplicate init should fail.
    interactor
        .tx()
        .from(&owner_address)
        .to(validation_interactor.address())
        .gas(600_000_000)
        .raw_call("init_job")
        .argument(&ManagedBuffer::<StaticApi>::new_from_bytes(
            new_job_id.as_bytes(),
        ))
        .argument(&1u64)
        .returns(ExpectError(4, "Job already initialized"))
        .run()
        .await;
}

/// Jobs younger than 3 days must not be removed by `clean_old_jobs`.
#[tokio::test]
async fn test_clean_old_jobs_not_old_enough() {
    let env = TestEnv::chain_only_with_options(ChainSimulatorOptions::with_round_duration_ms(
        ONE_DAY_ROUND_DURATION_MS,
    ))
    .await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
    let gateway_url = env.gateway_url.clone();
    let owner = env.owner.clone();

    let identity =
        IdentityRegistryInteractor::init(&mut interactor, owner.clone()).await;
    identity
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;

    let validation = ValidationRegistryInteractor::init(
        &mut interactor,
        owner.clone(),
        identity.address(),
    )
    .await;

    let recent_job_id = "job-recent";
    validation
        .init_job(&mut interactor, recent_job_id, 1)
        .await;

    // Only 2 simulated days elapsed — below the 3-day threshold.
    advance_simulator_days(2, &gateway_url).await;

    validation
        .clean_old_jobs(&mut interactor, vec![recent_job_id])
        .await;

    // Job still present — duplicate init must fail.
    interactor
        .tx()
        .from(&owner)
        .to(validation.address())
        .gas(600_000_000)
        .raw_call("init_job")
        .argument(&ManagedBuffer::<StaticApi>::new_from_bytes(
            recent_job_id.as_bytes(),
        ))
        .argument(&1u64)
        .returns(ExpectError(4, "Job already initialized"))
        .run()
        .await;
}
