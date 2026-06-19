use crate::common::{
    fund_address_on_simulator, issue_fungible_esdt_custom, TestEnv,
};
use multiversx_sc::types::{BigUint, ManagedBuffer, TokenIdentifier};
use multiversx_sc_snippets::imports::*;

async fn setup_payment_env() -> (TestEnv, Address, Address, Address, u64) {
    let (mut env, validation_addr, ..) = TestEnv::with_validation_agent().await;
    let owner = env.owner.clone();
    let employer = env.interactor.register_wallet(test_wallets::bob()).await;
    fund_address_on_simulator(
        &employer.to_bech32("erd").to_string(),
        "100000000000000000000000",
        &env.gateway_url,
    )
    .await;
    (env, validation_addr, owner, employer, 1)
}

#[tokio::test]
async fn test_init_job_wrong_token() {
    let (env, validation_addr, owner, .., agent_nonce) = setup_payment_env().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
    let gateway_url = env.gateway_url.clone();

    let token_id = issue_fungible_esdt_custom(
        &mut interactor,
        &owner,
        "FakeToken",
        "FAKE",
        1_000_000_000_000_000_000_000,
        18,
        &gateway_url,
    )
    .await;

    let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(b"wrong-token-job");
    let token_for_payment: TokenIdentifier<StaticApi> = TokenIdentifier::from(token_id.as_str());
    let payment_amount = BigUint::<StaticApi>::from(1_000_000_000_000_000_000u64);

    interactor
        .tx()
        .from(&owner)
        .to(&validation_addr)
        .gas(20_000_000)
        .single_esdt(&token_for_payment, 0, &payment_amount)
        .raw_call("init_job")
        .argument(&job_id_buf)
        .argument(&agent_nonce)
        .argument(&1u32)
        .returns(ExpectError(4, "Invalid payment token"))
        .run()
        .await;
}

#[tokio::test]
async fn test_init_job_insufficient_payment() {
    let (env, validation_addr, .., employer, agent_nonce) = setup_payment_env().await;
    std::mem::forget(env.pm);
    let gateway_url = env.gateway_url.clone();

    let mut interactor_employer = Interactor::new(&gateway_url)
        .await
        .use_chain_simulator(true);
    interactor_employer
        .register_wallet(test_wallets::bob())
        .await;

    let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(b"low-pay-job");

    interactor_employer
        .tx()
        .from(&employer)
        .to(&validation_addr)
        .gas(20_000_000)
        .egld(100_000_000_000_000_000u64)
        .raw_call("init_job")
        .argument(&job_id_buf)
        .argument(&agent_nonce)
        .argument(&1u32)
        .returns(ExpectError(4, "Insufficient payment"))
        .run()
        .await;
}

#[tokio::test]
async fn test_init_job_no_service_id() {
    let (env, validation_addr, .., employer, agent_nonce) = setup_payment_env().await;
    std::mem::forget(env.pm);
    let gateway_url = env.gateway_url.clone();

    let mut interactor_employer = Interactor::new(&gateway_url)
        .await
        .use_chain_simulator(true);
    interactor_employer
        .register_wallet(test_wallets::bob())
        .await;

    let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(b"no-service-job");

    interactor_employer
        .tx()
        .from(&employer)
        .to(&validation_addr)
        .gas(20_000_000)
        .raw_call("init_job")
        .argument(&job_id_buf)
        .argument(&agent_nonce)
        .run()
        .await;
}
