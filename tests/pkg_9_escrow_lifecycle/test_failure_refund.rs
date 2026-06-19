use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{sleep, Duration};

use crate::common::{
    wait_for_simulator_ready,deploy_all_registries, vm_query, EscrowInteractor, EscrowStatus};

/// T-002: Failure lifecycle
/// Register → deposit to escrow → init job → deadline passes → refund → submit bad rating → verify low score
#[tokio::test]
async fn test_failure_refund_lifecycle() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    let mut interactor = Interactor::new(&gateway_url)
        .await
        .use_chain_simulator(true);
    let owner = interactor.register_wallet(test_wallets::alice()).await;
    let employer = interactor.register_wallet(test_wallets::bob()).await;

    // 1. Deploy all registries + escrow
    let (identity, validation_addr, reputation_addr) =
        deploy_all_registries(&mut interactor, owner.clone()).await;

    let escrow = EscrowInteractor::deploy(
        &mut interactor,
        owner.clone(),
        &validation_addr,
        identity.address(),
    )
    .await;

    // 2. Register Agent
    identity
        .register_agent(&mut interactor, "FailureBot", "https://fail.bot", vec![])
        .await;
    let agent_nonce: u64 = 1;

    // 3. Employer deposits into escrow with a short deadline
    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let short_deadline = now_unix + 600; // 10 minutes from now

    let job_id = "job-lifecycle-fail";
    let deposit_amount: u64 = 3_000_000_000_000_000_000; // 3 EGLD

    // Deposit from employer using raw_call
    let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(job_id.as_bytes());
    let receiver_addr: ManagedAddress<StaticApi> = ManagedAddress::from_address(&owner);
    let poa_buf = ManagedBuffer::<StaticApi>::new_from_bytes(b"poa-failure-hash");

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
        .argument(&short_deadline)
        .run()
        .await;
    println!("✓ Employer deposited to escrow");

    // 4. Verify Active
    let data = escrow.get_escrow(&mut interactor, job_id).await;
    assert_eq!(data.status, EscrowStatus::Active);

    // 5. Init job but DO NOT submit proof (simulating failure/abandonment)
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
    println!("✓ Job initialized, but agent will NOT submit proof");

    // 6. Advance time past deadline (200 blocks × 6s = 1200s >> 600s deadline)
    let client = reqwest::Client::new();
    let _ = client
        .post(format!("{}/simulator/generate-blocks/200", gateway_url))
        .send()
        .await;
    sleep(Duration::from_millis(1000)).await;
    println!("✓ Generated 200 blocks to pass deadline");

    // 7. Refund escrow (anyone can call after deadline)
    interactor
        .tx()
        .from(&employer)
        .to(escrow.address())
        .gas(600_000_000)
        .raw_call("refund")
        .argument(&job_id_buf)
        .run()
        .await;

    // 8. Verify Refunded
    let data_refunded = escrow.get_escrow(&mut interactor, job_id).await;
    assert_eq!(data_refunded.status, EscrowStatus::Refunded);
    println!("✓ Escrow refunded after deadline");

    // 9. Submit bad feedback — low rating reflects poor delivery
    let rating: u64 = 10;
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
    println!("✓ Bad feedback submitted (rating=10)");

    // 10. Verify low reputation score
    let nonce_mb = ManagedBuffer::<StaticApi>::new_from_bytes(&agent_nonce.to_be_bytes());
    let score: u64 = vm_query(
        &mut interactor,
        &reputation_addr,
        "get_reputation_score",
        vec![nonce_mb],
    )
    .await;

    assert_eq!(score, 10, "Reputation score should be 10 (bad feedback)");
    println!("✓ Reputation score verified: {}", score);

    println!("✅ T-002 PASSED: Failure lifecycle with refund and bad rating complete");
}
