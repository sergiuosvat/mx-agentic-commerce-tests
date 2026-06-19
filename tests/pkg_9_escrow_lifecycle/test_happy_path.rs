use multiversx_sc::types::ManagedBuffer;
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;

use crate::common::{
    deploy_all_registries, vm_query, EscrowInteractor, EscrowStatus, wait_for_simulator_ready,
};

/// T-001: Full happy path lifecycle
/// Register agent → employer deposits to escrow → init job → submit proof → employer releases escrow → feedback → verify score
#[tokio::test]
async fn test_happy_path_escrow_lifecycle() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    // Alice = contract owner + agent
    let owner = interactor.register_wallet(test_wallets::alice()).await;
    // Bob = employer (hires the agent)
    let employer = interactor.register_wallet(test_wallets::bob()).await;

    // 1. Deploy all registries (identity + validation + reputation)
    let (identity, validation_addr, reputation_addr) =
        deploy_all_registries(&mut interactor, owner.clone()).await;

    // 2. Deploy escrow (owner = Alice)
    let escrow = EscrowInteractor::deploy(
        &mut interactor,
        owner.clone(),
        &validation_addr,
        identity.address(),
    )
    .await;

    // 3. Register Agent (Alice is the agent)
    identity
        .register_agent(&mut interactor, "LifecycleBot", "https://bot.io", vec![])
        .await;
    let agent_nonce: u64 = 1;

    // 4. Employer (Bob) deposits into escrow — receiver is the agent (Alice)
    let job_id = "job-lifecycle-happy";
    let deposit_amount: u64 = 5_000_000_000_000_000_000; // 5 EGLD
    let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(job_id.as_bytes());
    let receiver_addr: ManagedAddress<StaticApi> = ManagedAddress::from_address(&owner);
    let poa_buf = ManagedBuffer::<StaticApi>::new_from_bytes(b"poa-lifecycle-hash");
    let deadline: u64 = 9_999_999_999;

    interactor
        .tx()
        .from(&employer)
        .to(escrow.address())
        .gas(600_000_000)
        .egld(deposit_amount)
        .raw_call("deposit")
        .argument(&job_id_buf)
        .argument(&receiver_addr)
        .argument(&poa_buf)
        .argument(&deadline)
        .run()
        .await;
    println!("✓ Employer deposited to escrow");

    // 5. Verify escrow is Active
    let data = escrow.get_escrow(&mut interactor, job_id).await;
    assert_eq!(data.status, EscrowStatus::Active);
    println!("✓ Escrow deposited and Active");

    // 6. Init job on validation registry (employer initiates)
    interactor
        .tx()
        .from(&employer)
        .to(&validation_addr)
        .gas(10_000_000)
        .raw_call("init_job")
        .argument(&job_id_buf)
        .argument(&agent_nonce)
        .run()
        .await;
    println!("✓ Job initialized on validation");

    // 7. Agent (Alice) submits proof
    let proof = ManagedBuffer::<StaticApi>::new_from_bytes(b"lifecycle-proof-of-work");
    interactor
        .tx()
        .from(&owner)
        .to(&validation_addr)
        .gas(10_000_000)
        .raw_call("submit_proof")
        .argument(&job_id_buf)
        .argument(&proof)
        .run()
        .await;
    println!("✓ Proof submitted");

    // 8. ERC-8004: validation_request + validation_response (transitions job to Verified)
    let request_hash_buf = ManagedBuffer::<StaticApi>::new_from_bytes(b"req-hash-t001");
    let validator_managed: ManagedAddress<StaticApi> = ManagedAddress::from_address(&owner);
    let request_uri_buf = ManagedBuffer::<StaticApi>::new_from_bytes(b"https://validator.io/check");

    interactor
        .tx()
        .from(&owner)
        .to(&validation_addr)
        .gas(20_000_000)
        .raw_call("validation_request")
        .argument(&job_id_buf)
        .argument(&validator_managed)
        .argument(&request_uri_buf)
        .argument(&request_hash_buf)
        .run()
        .await;
    println!("✓ Validation requested");

    let response_uri_buf =
        ManagedBuffer::<StaticApi>::new_from_bytes(b"https://validator.io/result");
    let response_hash_buf = ManagedBuffer::<StaticApi>::new_from_bytes(b"resp-hash-t001");
    let tag_buf = ManagedBuffer::<StaticApi>::new_from_bytes(b"quality-check");
    let response_code: u8 = 1; // 1 = approved → Verified

    interactor
        .tx()
        .from(&owner)
        .to(&validation_addr)
        .gas(20_000_000)
        .raw_call("validation_response")
        .argument(&request_hash_buf)
        .argument(&response_code)
        .argument(&response_uri_buf)
        .argument(&response_hash_buf)
        .argument(&tag_buf)
        .run()
        .await;
    println!("✓ Validation approved → job Verified");

    // 9. Employer (Bob) releases escrow
    interactor
        .tx()
        .from(&employer)
        .to(escrow.address())
        .gas(600_000_000)
        .raw_call("release")
        .argument(&job_id_buf)
        .run()
        .await;

    // 9. Verify escrow is Released
    let data_released = escrow.get_escrow(&mut interactor, job_id).await;
    assert_eq!(data_released.status, EscrowStatus::Released);
    println!("✓ Escrow released");

    // 10. Submit feedback (Reputation) — employer rates the agent
    let rating: u64 = 95;
    interactor
        .tx()
        .from(&employer)
        .to(&reputation_addr)
        .gas(10_000_000)
        .raw_call("giveFeedbackSimple")
        .argument(&job_id_buf)
        .argument(&agent_nonce)
        .argument(&rating)
        .run()
        .await;
    println!("✓ Feedback submitted");

    // 11. Verify reputation score
    let nonce_mb = ManagedBuffer::<StaticApi>::new_from_bytes(&agent_nonce.to_be_bytes());
    let score: u64 = vm_query(
        &mut interactor,
        &reputation_addr,
        "get_reputation_score",
        vec![nonce_mb],
    )
    .await;

    assert_eq!(score, 95, "Reputation score should be 95");
    println!("✓ Reputation score verified: {}", score);

    println!("✅ T-001 PASSED: Full happy path with escrow lifecycle complete");
}
