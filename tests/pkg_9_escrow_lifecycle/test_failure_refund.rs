use multiversx_sc_snippets::imports::*;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{sleep, Duration};

use crate::common::{
    deploy_all_registries, vm_query, EscrowInteractor, EscrowStatus, TestEnv,
};

/// T-002: Failure lifecycle
/// Register → deposit to escrow → init job → deadline passes → refund → submit bad rating → verify low score
#[tokio::test]
async fn test_failure_refund_lifecycle() {
    let env = TestEnv::chain_only().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
    let gateway_url = env.gateway_url.clone();
    let owner = env.owner.clone();
    let employer = interactor.register_wallet(test_wallets::bob()).await;

    let (identity, validation_addr, reputation_addr) =
        deploy_all_registries(&mut interactor, owner.clone()).await;

    let escrow = EscrowInteractor::deploy(
        &mut interactor,
        owner.clone(),
        &validation_addr,
        identity.address(),
    )
    .await;

    identity
        .register_agent(&mut interactor, "FailureBot", "https://fail.bot", vec![])
        .await;
    let agent_nonce: u64 = 1;

    let short_deadline = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 600;

    let job_id = "job-lifecycle-fail";
    let deposit_amount: u64 = 3_000_000_000_000_000_000;

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

    let data = escrow.get_escrow(&mut interactor, job_id).await;
    assert_eq!(data.status, EscrowStatus::Active);

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

    let client = reqwest::Client::new();
    client
        .post(format!("{}/simulator/generate-blocks/200", gateway_url))
        .send()
        .await
        .ok();
    sleep(Duration::from_millis(1000)).await;

    interactor
        .tx()
        .from(&employer)
        .to(escrow.address())
        .gas(600_000_000)
        .raw_call("refund")
        .argument(&job_id_buf)
        .run()
        .await;

    let data_refunded = escrow.get_escrow(&mut interactor, job_id).await;
    assert_eq!(data_refunded.status, EscrowStatus::Refunded);

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

    let nonce_mb = ManagedBuffer::<StaticApi>::new_from_bytes(&agent_nonce.to_be_bytes());
    let score: u64 = vm_query(
        &mut interactor,
        &reputation_addr,
        "get_reputation_score",
        vec![nonce_mb],
    )
    .await;

    assert_eq!(score, 10, "Reputation score should be 10 (bad feedback)");
}
