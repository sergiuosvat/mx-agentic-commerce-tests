use crate::common::{
    create_pem_file, fund_address_on_simulator, generate_random_private_key,
    IdentityRegistryInteractor,
};
use identity_registry_interactor::identity_registry_proxy::IdentityRegistryProxy;
use multiversx_sc::types::TokenIdentifier;
use multiversx_sc_snippets::imports::*;

use mx_agentic_commerce_tests::ProcessManager;
use tokio::time::{sleep, Duration};

#[tokio::test]
async fn test_token_issuance_happy_path() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    sleep(Duration::from_secs(2)).await;

    let gateway_url = format!("http://localhost:{}", port);


    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);

    // Generate Alice wallet
    let alice_private_key = generate_random_private_key();
    let alice_wallet = Wallet::from_private_key(&alice_private_key).unwrap();
    let alice_address = alice_wallet.to_address();
    create_pem_file(
        "alice.pem",
        &alice_private_key,
        &alice_address.to_bech32("erd").to_string(),
    );

    interactor.register_wallet(alice_wallet).await;
    let wallet_bech32 = alice_address.to_bech32("erd").to_string();
    fund_address_on_simulator(&wallet_bech32, "100000000000000000000000", &gateway_url).await; // 100k EGLD

    // Deploy
    let identity_interactor =
        IdentityRegistryInteractor::init(&mut interactor, alice_address.clone()).await;

    // Issue Token
    identity_interactor
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;

    // Verify using Proxy View
    let address = identity_interactor.address().clone();
    let token_id: TokenIdentifier<StaticApi> = interactor
        .query()
        .to(&address)
        .typed(IdentityRegistryProxy)
        .agent_token_id()
        .returns(ReturnsResult)
        .run()
        .await;

    assert!(
        token_id.to_string().starts_with("AGENT-"),
        "Token ID should start with AGENT-"
    );
}

#[tokio::test]
async fn test_token_issuance_errors() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator"); // Port config handling? Parallel tests?
    sleep(Duration::from_secs(2)).await;

    let gateway_url = format!("http://localhost:{}", port);


    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);

    // Generate Bob wallet
    let bob_private_key = generate_random_private_key();
    let bob_wallet = Wallet::from_private_key(&bob_private_key).unwrap();
    let bob_address = bob_wallet.to_address();
    create_pem_file(
        "bob.pem",
        &bob_private_key,
        &bob_address.to_bech32("erd").to_string(),
    );

    interactor.register_wallet(bob_wallet).await;
    let wallet_bech32 = bob_address.to_bech32("erd").to_string();
    fund_address_on_simulator(&wallet_bech32, "100000000000000000000000", &gateway_url).await;

    let identity_interactor =
        IdentityRegistryInteractor::init(&mut interactor, bob_address.clone()).await;

    // 1. Issue Token (Success)
    identity_interactor
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;

    // 2. Tries to Issue Again (Fail)
    let address = identity_interactor.address().clone();
    interactor
        .tx()
        .from(&bob_address)
        .to(&address)
        .gas(60_000_000)
        .egld(50_000_000_000_000_000u64)
        .raw_call("issue_token")
        .argument(&ManagedBuffer::<StaticApi>::new_from_bytes(b"AgentToken"))
        .argument(&ManagedBuffer::<StaticApi>::new_from_bytes(b"AGENT"))
        .returns(ExpectError(4, "Token already issued"))
        .run()
        .await;
}
