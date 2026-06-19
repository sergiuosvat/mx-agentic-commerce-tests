use crate::common::{
    create_pem_file, fund_address_on_simulator, generate_random_private_key,
    issue_fungible_esdt_custom, IdentityRegistryInteractor, ServiceConfigInput,
    ValidationRegistryInteractor,
};
use multiversx_sc::types::{BigUint, EgldOrEsdtTokenIdentifier, ManagedBuffer, TokenIdentifier};
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use tokio::time::{sleep, Duration};

// ── Shared setup helper ──────────────────────────────────────────────

/// Sets up an environment with:
/// - owner-deployed identity + validation registries
/// - agent with service_id=1 (cost=1 EGLD, token=EGLD)
/// - a funded employer wallet
///
/// Returns (pm, interactor, validation_interactor, identity, owner, employer_wallet, agent_nonce, gateway_url).
async fn setup_payment_env() -> (
    ProcessManager,
    Interactor,
    ValidationRegistryInteractor,
    IdentityRegistryInteractor,
    Address,
    Wallet,
    u64,
    String,
) {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    sleep(Duration::from_secs(3)).await;

    let mut interactor = Interactor::new(&gateway_url)
        .await
        .use_chain_simulator(true);

    // Owner
    let owner_pk = generate_random_private_key();
    let owner_wallet = Wallet::from_private_key(&owner_pk).unwrap();
    let owner = owner_wallet.to_address();
    create_pem_file(
        &format!("pay_err_{}.pem", port),
        &owner_pk,
        &owner.to_bech32("erd").to_string(),
    );
    interactor.register_wallet(owner_wallet).await;
    fund_address_on_simulator(
        &owner.to_bech32("erd").to_string(),
        "100000000000000000000000",
        &gateway_url,
    )
    .await;

    // Employer
    let employer_pk = generate_random_private_key();
    let employer_wallet = Wallet::from_private_key(&employer_pk).unwrap();
    let employer = employer_wallet.to_address();
    fund_address_on_simulator(
        &employer.to_bech32("erd").to_string(),
        "100000000000000000000000",
        &gateway_url,
    )
    .await;

    // Deploy identity + issue token
    let identity = IdentityRegistryInteractor::init(&mut interactor, owner.clone()).await;
    identity
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;

    // Register agent with service_id=1, cost=1 EGLD
    let service = ServiceConfigInput {
        service_id: 1,
        price: BigUint::from(1_000_000_000_000_000_000u64), // 1 EGLD
        token: EgldOrEsdtTokenIdentifier::egld(),
        nonce: 0,
    };
    identity
        .register_agent_with_services(&mut interactor, "PayBot", "uri", vec![], vec![service])
        .await;
    let agent_nonce: u64 = 1;

    // Deploy validation registry
    let validation =
        ValidationRegistryInteractor::init(&mut interactor, owner.clone(), identity.address())
            .await;

    // Cleanup PEM
    std::fs::remove_file(format!("pay_err_{}.pem", port)).unwrap_or(());

    (
        pm,
        interactor,
        validation,
        identity,
        owner,
        employer_wallet,
        agent_nonce,
        gateway_url,
    )
}

// ── Test: Wrong token → ERR_INVALID_PAYMENT ──────────────────────────

/// Service expects EGLD but we send a fungible ESDT token.
/// The owner (who issued the ESDT and holds it) calls init_job with the wrong token.
#[tokio::test]
async fn test_init_job_wrong_token() {
    let (
        pm,
        mut interactor,
        validation,
        _,
        owner,
        _,
        agent_nonce,
        gateway_url,
    ) = setup_payment_env().await;

    // Issue a dummy ESDT so we have a real token to send
    let token_id = issue_fungible_esdt_custom(
        &mut interactor,
        &owner,
        "FakeToken",
        "FAKE",
        1_000_000_000_000_000_000_000, // 1000 tokens (18 decimals)
        18,
        &gateway_url,
    )
    .await;

    // Owner sends FAKE ESDT to init_job (service expects EGLD → ERR_INVALID_PAYMENT)
    let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(b"wrong-token-job");
    let token_for_payment: TokenIdentifier<StaticApi> = TokenIdentifier::from(token_id.as_str());
    let payment_amount = BigUint::<StaticApi>::from(1_000_000_000_000_000_000u64); // 1 token

    interactor
        .tx()
        .from(&owner)
        .to(&validation.contract_address)
        .gas(20_000_000)
        .single_esdt(&token_for_payment, 0, &payment_amount)
        .raw_call("init_job")
        .argument(&job_id_buf)
        .argument(&agent_nonce)
        .argument(&1u32) // service_id
        .returns(ExpectError(4, "Invalid payment token"))
        .run()
        .await;

    drop(pm);
}

// ── Test: Insufficient amount → ERR_INSUFFICIENT_PAYMENT ─────────────

/// Service expects 1 EGLD but we send only 0.1 EGLD.
#[tokio::test]
async fn test_init_job_insufficient_payment() {
    let (
        pm,
        _,
        validation,
        _,
        _,
        employer_wallet,
        agent_nonce,
        gateway_url,
    ) = setup_payment_env().await;

    let employer_addr = employer_wallet.to_address();
    let mut interactor_employer = Interactor::new(&gateway_url)
        .await
        .use_chain_simulator(true);
    interactor_employer
        .register_wallet(employer_wallet)
        .await;

    let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(b"low-pay-job");

    interactor_employer
        .tx()
        .from(&employer_addr)
        .to(&validation.contract_address)
        .gas(20_000_000)
        .egld(100_000_000_000_000_000u64) // 0.1 EGLD — insufficient
        .raw_call("init_job")
        .argument(&job_id_buf)
        .argument(&agent_nonce)
        .argument(&1u32) // service_id
        .returns(ExpectError(4, "Insufficient payment"))
        .run()
        .await;

    drop(pm);
}

// ── Test: init_job WITHOUT service_id → no payment required ──────────

/// When init_job is called without a service_id (OptionalValue::None),
/// the payment validation branch is skipped entirely → no payment required.
#[tokio::test]
async fn test_init_job_no_service_id() {
    let (
        pm,
        _,
        validation,
        _,
        _,
        employer_wallet,
        agent_nonce,
        gateway_url,
    ) = setup_payment_env().await;

    let employer = employer_wallet.to_address();
    let mut interactor_employer = Interactor::new(&gateway_url)
        .await
        .use_chain_simulator(true);
    interactor_employer.register_wallet(employer_wallet).await;

    let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(b"no-service-job");

    // Call init_job with only job_id + agent_nonce, NO service_id → should succeed
    interactor_employer
        .tx()
        .from(&employer)
        .to(&validation.contract_address)
        .gas(20_000_000)
        .raw_call("init_job")
        .argument(&job_id_buf)
        .argument(&agent_nonce)
        .run()
        .await;

    println!("No-service-id init_job succeeded as expected");

    drop(pm);
}
