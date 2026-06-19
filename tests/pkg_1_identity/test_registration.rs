use crate::common::{
    create_pem_file, fund_address_on_simulator, generate_random_private_key, IdentityRegistryInteractor, ServiceConfigInput,
};
use identity_registry_interactor::identity_registry_proxy::IdentityRegistryProxy;
use multiversx_sc::types::{
    BigUint, EgldOrEsdtTokenIdentifier, ManagedAddress, ManagedBuffer, TokenIdentifier,
};
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use tokio::time::{sleep, Duration};

#[tokio::test]
async fn test_basic_registration() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    sleep(Duration::from_secs(2)).await;

    let gateway_url = format!("http://localhost:{}", port);


    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);

    // Alice setup
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
    fund_address_on_simulator(&wallet_bech32, "100000000000000000000000", &gateway_url).await;

    // Deploy & Issue Token
    let identity_interactor =
        IdentityRegistryInteractor::init(&mut interactor, alice_address.clone()).await;
    identity_interactor
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;

    // Register Agent (Basic)
    identity_interactor
        .register_agent(
            &mut interactor,
            "Bot1",
            "https://bot1.io/manifest.json",
            vec![],
        )
        .await;

    // Verify
    let address = identity_interactor.address().clone();

    let owner_managed: ManagedAddress<StaticApi> = interactor
        .query()
        .to(&address)
        .typed(IdentityRegistryProxy)
        .get_agent_owner(1u64)
        .returns(ReturnsResult)
        .run()
        .await;

    assert_eq!(
        owner_managed.to_address(),
        alice_address,
        "Owner address should match"
    );
}

#[tokio::test]
async fn test_registration_with_metadata() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    sleep(Duration::from_secs(2)).await;

    let gateway_url = format!("http://localhost:{}", port);


    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);

    // Alice setup
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
    fund_address_on_simulator(&wallet_bech32, "100000000000000000000000", &gateway_url).await;

    // Deploy & Issue
    let identity_interactor =
        IdentityRegistryInteractor::init(&mut interactor, alice_address.clone()).await;
    identity_interactor
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;

    // Metadata entries
    let metadata = vec![
        ("price:default", "1000000".as_bytes().to_vec()),
        ("token:default", "EGLD".as_bytes().to_vec()),
    ];

    // Register
    identity_interactor
        .register_agent(
            &mut interactor,
            "BotMeta",
            "https://bot-meta.io/manifest.json",
            metadata,
        )
        .await;

    // Verify
    let address = identity_interactor.address().clone();

    // Proxy: get_metadata(nonce, key) -> OptionalValue<ManagedBuffer>
    let stored_price_opt: OptionalValue<ManagedBuffer<StaticApi>> = interactor
        .query()
        .to(&address)
        .typed(IdentityRegistryProxy)
        .get_metadata(1u64, ManagedBuffer::new_from_bytes(b"price:default"))
        .returns(ReturnsResult)
        .run()
        .await;

    let stored_price = stored_price_opt.into_option();

    assert!(stored_price.is_some(), "Metadata should exist");
    assert_eq!(
        stored_price.unwrap().to_vec(),
        b"1000000",
        "Price metadata mismatch"
    );
}

#[tokio::test]
async fn test_registration_with_services() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    sleep(Duration::from_secs(2)).await;

    let gateway_url = format!("http://localhost:{}", port);


    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);

    // Alice setup
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
    fund_address_on_simulator(&wallet_bech32, "100000000000000000000000", &gateway_url).await;

    // Deploy & Issue
    let identity_interactor =
        IdentityRegistryInteractor::init(&mut interactor, alice_address.clone()).await;
    identity_interactor
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;

    // Service Config
    let service = ServiceConfigInput {
        service_id: 1,
        price: BigUint::from(1_000_000_000_000_000_000u64), // 1 Token
        token: EgldOrEsdtTokenIdentifier::esdt(TokenIdentifier::from("TOKEN-123456")),
        nonce: 0,
    };

    // Register
    identity_interactor
        .register_agent_with_services(
            &mut interactor,
            "BotService",
            "https://bot-service.io",
            vec![],
            vec![service],
        )
        .await;

    // Verify
    let address = identity_interactor.address().clone();

    let owner_managed: ManagedAddress<StaticApi> = interactor
        .query()
        .to(&address)
        .typed(IdentityRegistryProxy)
        .get_agent_owner(1u64)
        .returns(ReturnsResult)
        .run()
        .await;

    assert_eq!(owner_managed.to_address(), alice_address);
}
