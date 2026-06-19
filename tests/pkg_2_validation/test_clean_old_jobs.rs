use crate::common::{
    advance_simulator_days, create_temp_pem_file, fund_address_on_simulator,
    generate_random_private_key, wait_for_simulator_ready, IdentityRegistryInteractor,
    ValidationRegistryInteractor, ONE_DAY_ROUND_DURATION_MS,
};
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::{ChainSimulatorOptions, ProcessManager};

/// Exercises `clean_old_jobs` on the chain simulator by starting with a 1-day
/// round duration, then generating four blocks (>3 day threshold).
#[tokio::test]
async fn test_clean_old_jobs() {
    let mut pm = ProcessManager::new();
    let port = pm
        .start_chain_simulator_with_options(ChainSimulatorOptions::with_round_duration_ms(
            ONE_DAY_ROUND_DURATION_MS,
        ))
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    let mut interactor = Interactor::new(&gateway_url)
        .await
        .use_chain_simulator(true);

    let owner_private_key = generate_random_private_key();
    let owner_wallet = Wallet::from_private_key(&owner_private_key).unwrap();
    let owner_address = owner_wallet.to_address();
    let owner_bech32 = owner_address.to_bech32("erd").to_string();
    let _pem_path = create_temp_pem_file("owner_clean", &owner_private_key, &owner_bech32);

    interactor.register_wallet(owner_wallet).await;
    fund_address_on_simulator(&owner_bech32, "100000000000000000000000", &gateway_url).await;

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
