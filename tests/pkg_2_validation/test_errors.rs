use multiversx_sc::types::ManagedBuffer;
use multiversx_sc_snippets::imports::*;

use crate::common::{fund_address_on_simulator, TestEnv};

#[tokio::test]
#[should_panic(expected = "Insufficient payment")]
async fn test_init_job_insufficient_payment() {
    let (env, validation_addr, _) = TestEnv::with_validation_agent().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
    let gateway_url = env.gateway_url;

    let employer = interactor.register_wallet(test_wallets::bob()).await;
    fund_address_on_simulator(
        &crate::common::address_to_bech32(&employer),
        "100000000000000000000",
        &gateway_url,
    )
    .await;

    interactor
        .tx()
        .from(&employer)
        .to(&validation_addr)
        .gas(20_000_000)
        .egld(500_000_000_000_000_000u64)
        .raw_call("init_job")
        .argument(&ManagedBuffer::<StaticApi>::new_from_bytes(b"job-fail-1"))
        .argument(&1u64)
        .argument(&1u32)
        .run()
        .await;
}

#[tokio::test]
#[should_panic(expected = "Job already initialized")]
async fn test_init_job_duplicate_id() {
    let (env, validation_addr, _) = TestEnv::with_validation_agent().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
    let gateway_url = env.gateway_url;

    let employer = interactor.register_wallet(test_wallets::bob()).await;
    fund_address_on_simulator(
        &crate::common::address_to_bech32(&employer),
        "100000000000000000000",
        &gateway_url,
    )
    .await;

    interactor
        .tx()
        .from(&employer)
        .to(&validation_addr)
        .gas(20_000_000)
        .raw_call("init_job")
        .argument(&ManagedBuffer::<StaticApi>::new_from_bytes(b"job-dup"))
        .argument(&1u64)
        .run()
        .await;

    interactor
        .tx()
        .from(&employer)
        .to(&validation_addr)
        .gas(20_000_000)
        .raw_call("init_job")
        .argument(&ManagedBuffer::<StaticApi>::new_from_bytes(b"job-dup"))
        .argument(&1u64)
        .run()
        .await;
}
