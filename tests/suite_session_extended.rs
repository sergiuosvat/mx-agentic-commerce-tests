use common::mpp_session_mvx_proxy::MppSessionContractProxy;
use common::mpp_session_helpers::{
    deploy_session_contract, open_session, query_session, setup_session_wallets,
    sign_session_voucher,
};
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;

mod common;
use common::{
    fund_address_on_simulator, generate_blocks_on_simulator,
    get_simulator_block_timestamp_secs, wait_for_simulator_ready,
};

/// Advance simulated chain time until `deadline` (seconds) has passed.
async fn advance_past_deadline(gateway_url: &str, deadline: u64) {
    loop {
        let now = get_simulator_block_timestamp_secs(gateway_url).await;
        if now >= deadline {
            break;
        }
        generate_blocks_on_simulator(10, gateway_url).await;
    }
}

async fn session_deadline(gateway_url: &str, offset_secs: u64) -> u64 {
    get_simulator_block_timestamp_secs(gateway_url).await + offset_secs
}

const FIVE_EGLD: u64 = 5_000_000_000_000_000_000;
const ONE_EGLD: u64 = 1_000_000_000_000_000_000;

async fn session_test_env() -> (ProcessManager, String, Interactor, Address) {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator().expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{port}");
    wait_for_simulator_ready(&gateway_url).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    let alice = interactor.register_wallet(test_wallets::alice()).await;
    fund_address_on_simulator(
        &alice.to_bech32("erd").to_string(),
        "100000000000000000000",
        &gateway_url,
    )
    .await;

    (pm, gateway_url, interactor, alice)
}

/// Port of `mpp-session-mvx` top-up happy path to chain simulator.
#[tokio::test]
async fn test_session_top_up_happy_path() {
    let (_pm, gateway_url, mut interactor, deployer) = session_test_env().await;
    let wallets = setup_session_wallets(&mut interactor).await;
    fund_address_on_simulator(
        &wallets.employer_addr.to_bech32("erd").to_string(),
        "100000000000000000000",
        &gateway_url,
    )
    .await;

    let sc_address = deploy_session_contract(&mut interactor, &deployer).await;
    let deadline = session_deadline(&gateway_url, 10_000).await;

    let channel_id = open_session(
        &mut interactor,
        &wallets.employer_addr,
        &sc_address,
        &wallets.receiver_addr,
        deadline,
        FIVE_EGLD,
    )
    .await;

    interactor
        .tx()
        .from(&wallets.employer_addr)
        .to(&sc_address)
        .gas(30_000_000u64)
        .typed(MppSessionContractProxy)
        .top_up(ManagedBuffer::new_from_bytes(&channel_id))
        .egld(ONE_EGLD)
        .run()
        .await;

    let session = query_session(&mut interactor, &sc_address, &channel_id).await;
    assert_eq!(
        session.amount_locked,
        BigUint::from(FIVE_EGLD + ONE_EGLD),
        "Session should hold 6 EGLD after top-up"
    );
    println!("✅ Session top-up happy path passed");
}

/// Port of `test_top_up_closed_session` — top-up after request_close must revert.
#[tokio::test]
async fn test_session_top_up_after_close_rejected() {
    let (_pm, gateway_url, mut interactor, deployer) = session_test_env().await;
    let wallets = setup_session_wallets(&mut interactor).await;
    fund_address_on_simulator(
        &wallets.employer_addr.to_bech32("erd").to_string(),
        "100000000000000000000",
        &gateway_url,
    )
    .await;

    let sc_address = deploy_session_contract(&mut interactor, &deployer).await;
    let deadline = session_deadline(&gateway_url, 120).await;

    let channel_id = open_session(
        &mut interactor,
        &wallets.employer_addr,
        &sc_address,
        &wallets.receiver_addr,
        deadline,
        FIVE_EGLD,
    )
    .await;

    advance_past_deadline(&gateway_url, deadline).await;

    interactor
        .tx()
        .from(&wallets.employer_addr)
        .to(&sc_address)
        .gas(30_000_000u64)
        .typed(MppSessionContractProxy)
        .request_close(ManagedBuffer::new_from_bytes(&channel_id))
        .run()
        .await;

    interactor
        .tx()
        .from(&wallets.employer_addr)
        .to(&sc_address)
        .gas(30_000_000u64)
        .typed(MppSessionContractProxy)
        .top_up(ManagedBuffer::new_from_bytes(&channel_id))
        .egld(ONE_EGLD)
        .returns(ExpectError(4, "Session already closed"))
        .run()
        .await;

    println!("✅ Session top-up on closed session correctly rejected");
}

/// Port of slashing flow — employer recovers locked funds after deadline via request_close.
#[tokio::test]
async fn test_session_slashing_request_close_after_deadline() {
    let (_pm, gateway_url, mut interactor, deployer) = session_test_env().await;
    let wallets = setup_session_wallets(&mut interactor).await;
    let employer_bech32 = wallets.employer_addr.to_bech32("erd").to_string();
    fund_address_on_simulator(&employer_bech32, "100000000000000000000", &gateway_url).await;

    let sc_address = deploy_session_contract(&mut interactor, &deployer).await;
    let deadline = session_deadline(&gateway_url, 120).await;

    let channel_id = open_session(
        &mut interactor,
        &wallets.employer_addr,
        &sc_address,
        &wallets.receiver_addr,
        deadline,
        FIVE_EGLD,
    )
    .await;

    let balance_after_open: u128 = interactor
        .get_account(&wallets.employer_addr)
        .await
        .balance
        .parse()
        .unwrap_or(0);

    advance_past_deadline(&gateway_url, deadline).await;

    interactor
        .tx()
        .from(&wallets.employer_addr)
        .to(&sc_address)
        .gas(30_000_000u64)
        .typed(MppSessionContractProxy)
        .request_close(ManagedBuffer::new_from_bytes(&channel_id))
        .run()
        .await;

    let session = query_session(&mut interactor, &sc_address, &channel_id).await;
    assert_eq!(session.status, 2, "Session should be Closed");

    let balance_after: u128 = interactor
        .get_account(&wallets.employer_addr)
        .await
        .balance
        .parse()
        .unwrap_or(0);
    // Recovered ~5 EGLD minus gas for open + request_close
    assert!(
        balance_after >= balance_after_open + FIVE_EGLD as u128 / 2,
        "Employer should recover most escrowed EGLD after slashing close (after_open={balance_after_open}, after_close={balance_after})"
    );
    println!("✅ Session slashing (request_close after deadline) passed");
}

/// Port of `test_slashing_flow_fail_before_deadline`.
#[tokio::test]
async fn test_session_request_close_before_deadline_rejected() {
    let (_pm, gateway_url, mut interactor, deployer) = session_test_env().await;
    let wallets = setup_session_wallets(&mut interactor).await;
    fund_address_on_simulator(
        &wallets.employer_addr.to_bech32("erd").to_string(),
        "100000000000000000000",
        &gateway_url,
    )
    .await;

    let sc_address = deploy_session_contract(&mut interactor, &deployer).await;
    let deadline = session_deadline(&gateway_url, 10_000).await;

    let channel_id = open_session(
        &mut interactor,
        &wallets.employer_addr,
        &sc_address,
        &wallets.receiver_addr,
        deadline,
        FIVE_EGLD,
    )
    .await;

    interactor
        .tx()
        .from(&wallets.employer_addr)
        .to(&sc_address)
        .gas(30_000_000u64)
        .typed(MppSessionContractProxy)
        .request_close(ManagedBuffer::new_from_bytes(&channel_id))
        .returns(ExpectError(4, "Challenge period not over"))
        .run()
        .await;

    println!("✅ Session request_close before deadline correctly rejected");
}

/// Port of `test_negative_flow_invalid_signature`.
#[tokio::test]
async fn test_session_settle_invalid_signature_rejected() {
    let (_pm, gateway_url, mut interactor, deployer) = session_test_env().await;
    let wallets = setup_session_wallets(&mut interactor).await;
    fund_address_on_simulator(
        &wallets.employer_addr.to_bech32("erd").to_string(),
        "100000000000000000000",
        &gateway_url,
    )
    .await;

    let sc_address = deploy_session_contract(&mut interactor, &deployer).await;
    let deadline = session_deadline(&gateway_url, 10_000).await;

    let channel_id = open_session(
        &mut interactor,
        &wallets.employer_addr,
        &sc_address,
        &wallets.receiver_addr,
        deadline,
        FIVE_EGLD,
    )
    .await;

    let invalid_signature = [0u8; 64];

    interactor
        .tx()
        .from(&wallets.receiver_addr)
        .to(&sc_address)
        .gas(30_000_000u64)
        .typed(MppSessionContractProxy)
        .settle(
            ManagedBuffer::new_from_bytes(&channel_id),
            BigUint::from(1_000_000u64),
            1u64,
            ManagedBuffer::new_from_bytes(&invalid_signature),
        )
        .returns(ExpectError(4, "invalid signature"))
        .run()
        .await;

    println!("✅ Session settle with invalid signature correctly rejected");
}

/// Port of `test_negative_flow_insufficient_funds`.
#[tokio::test]
async fn test_session_settle_insufficient_funds_rejected() {
    let (_pm, gateway_url, mut interactor, deployer) = session_test_env().await;
    let wallets = setup_session_wallets(&mut interactor).await;
    fund_address_on_simulator(
        &wallets.employer_addr.to_bech32("erd").to_string(),
        "100000000000000000000",
        &gateway_url,
    )
    .await;

    let sc_address = deploy_session_contract(&mut interactor, &deployer).await;
    let deadline = get_simulator_block_timestamp_secs(&gateway_url).await + 10_000;
    let deposit = 5_000_000u64;
    let settle_amount = 6_000_000u64;

    let channel_id = open_session(
        &mut interactor,
        &wallets.employer_addr,
        &sc_address,
        &wallets.receiver_addr,
        deadline,
        deposit,
    )
    .await;

    let signature = sign_session_voucher(
        &wallets.employer_signing_key,
        &sc_address,
        &channel_id,
        settle_amount,
        1,
    );

    interactor
        .tx()
        .from(&wallets.receiver_addr)
        .to(&sc_address)
        .gas(30_000_000u64)
        .typed(MppSessionContractProxy)
        .settle(
            ManagedBuffer::new_from_bytes(&channel_id),
            BigUint::from(settle_amount),
            1u64,
            ManagedBuffer::new_from_bytes(&signature),
        )
        .returns(ExpectError(4, "Insufficient funds in session"))
        .run()
        .await;

    println!("✅ Session settle over locked amount correctly rejected");
}
