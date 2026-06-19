use multiversx_sc::types::ManagedBuffer;
use multiversx_sc_snippets::imports::*;

use crate::common::{TestEnv, 
    address_to_bech32, deploy_all_registries, fund_address_on_simulator,
};

/// Returns (interactor, reputation_addr, validation_addr, owner, employer, mallory)
async fn setup_env() -> (
    Interactor,
    Address,
    Address,
    Address,
    Address,
    Address,
) {
    let env = TestEnv::chain_only().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
    let gateway_url = env.gateway_url.clone();
    let owner = env.owner.clone();
    let employer = interactor.register_wallet(test_wallets::bob()).await;
    let mallory = interactor.register_wallet(test_wallets::carol()).await;

    fund_address_on_simulator(&address_to_bech32(&owner), "100000000000000000000", &gateway_url).await;
    fund_address_on_simulator(&address_to_bech32(&employer), "100000000000000000000", &gateway_url).await;
    fund_address_on_simulator(&address_to_bech32(&mallory), "100000000000000000000", &gateway_url).await;

    let (identity, validation_addr, reputation_addr) =
        deploy_all_registries(&mut interactor, owner.clone()).await;

    // Setup: Register Agent -> Init Job -> Submit Proof
    identity
        .register_agent(&mut interactor, "Bot", "uri", vec![])
        .await;

    let job_id = "job-rep-err";
    let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(job_id.as_bytes());
    let agent_nonce = 1u64;

    // Init Job
    interactor
        .tx()
        .from(&employer)
        .to(&validation_addr)
        .gas(20_000_000)
        .raw_call("init_job")
        .argument(&job_id_buf)
        .argument(&agent_nonce)
        .run()
        .await;

    // Submit Proof
    interactor
        .tx()
        .from(&owner)
        .to(&validation_addr)
        .gas(20_000_000)
        .raw_call("submit_proof")
        .argument(&job_id_buf)
        .argument(&ManagedBuffer::<StaticApi>::new_from_bytes(b"proof"))
        .run()
        .await;

    (
        interactor,
        reputation_addr,
        validation_addr,
        owner,
        employer,
        mallory,
    )
}


#[tokio::test]
async fn test_submit_feedback_non_employer() {
    let (mut interactor, reputation_addr, .., mallory) = setup_env().await;

    let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(b"job-rep-err");

    // Mallory (not the employer) tries to submit feedback — should fail
    interactor
        .tx()
        .from(&mallory)
        .to(&reputation_addr)
        .gas(20_000_000)
        .raw_call("giveFeedbackSimple")
        .argument(&job_id_buf)
        .argument(&1u64) // agent nonce
        .argument(&80u64) // rating
        .returns(ExpectError(4, "Only the employer can provide feedback"))
        .run()
        .await;
}

#[tokio::test]
async fn test_submit_feedback_duplicate() {
    let (mut interactor, reputation_addr, _, _, employer, _) = setup_env().await;

    let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(b"job-rep-err");

    // First feedback — should succeed (ERC-8004: no authorization needed)
    interactor
        .tx()
        .from(&employer)
        .to(&reputation_addr)
        .gas(20_000_000)
        .raw_call("giveFeedbackSimple")
        .argument(&job_id_buf)
        .argument(&1u64)
        .argument(&80u64)
        .run()
        .await;

    // Second feedback — should fail
    interactor
        .tx()
        .from(&employer)
        .to(&reputation_addr)
        .gas(20_000_000)
        .raw_call("giveFeedbackSimple")
        .argument(&job_id_buf)
        .argument(&1u64)
        .argument(&90u64)
        .returns(ExpectError(4, "Feedback already provided for this job"))
        .run()
        .await;
}

#[tokio::test]
async fn test_append_response_permissionless() {
    // ERC-8004: append_response is now permissionless — anyone can call it
    let (mut interactor, reputation_addr, .., mallory) = setup_env().await;

    let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(b"job-rep-err");
    let response_uri = ManagedBuffer::<StaticApi>::new_from_bytes(b"https://response.example.com");

    // Anyone (even mallory) can append response — should succeed
    interactor
        .tx()
        .from(&mallory)
        .to(&reputation_addr)
        .gas(20_000_000)
        .raw_call("append_response")
        .argument(&job_id_buf)
        .argument(&response_uri)
        .run()
        .await;

    println!("append_response by anyone succeeded — ERC-8004 compliant");
}
