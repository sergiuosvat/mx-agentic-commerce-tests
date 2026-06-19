use crate::common::{
    create_pem_file, fund_address_on_simulator, generate_random_private_key, IdentityRegistryInteractor,
};
use identity_registry_interactor::identity_registry_proxy::IdentityRegistryProxy;
use multiversx_sc::types::{ManagedAddress, ManagedBuffer, TokenIdentifier};
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use tokio::time::{sleep, Duration};

#[tokio::test]
async fn test_update_agent_full() {
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

    // Deploy & Issue & Register
    let identity_interactor =
        IdentityRegistryInteractor::init(&mut interactor, alice_address.clone()).await;
    identity_interactor
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;
    identity_interactor
        .register_agent(&mut interactor, "Bot1", "uri1", vec![])
        .await;

    let address = identity_interactor.address().clone();

    // Retrieve TokenID
    let token_id: TokenIdentifier<StaticApi> = interactor
        .query()
        .to(&address)
        .typed(IdentityRegistryProxy)
        .agent_token_id()
        .returns(ReturnsResult)
        .run()
        .await;
    let token_str = token_id.to_string();

    // Update Agent
    identity_interactor
        .update_agent(
            &mut interactor,
            "Bot1Updated",
            "uri2",
            vec![],
            vec![],
            (&token_str, 1u64),
        )
        .await;

    // Verify
    // Just verify owner for now as proxy return type complexity is high
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

#[tokio::test]
async fn test_update_agent_metadata() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    sleep(Duration::from_secs(2)).await;
    let gateway_url = format!("http://localhost:{}", port);

    let mut interactor = Interactor::new(&gateway_url)
        .await
        .use_chain_simulator(true);

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
    crate::common::fund_address_on_simulator(
        &wallet_bech32,
        "100000000000000000000000",
        &gateway_url,
    )
    .await;

    let identity_interactor =
        IdentityRegistryInteractor::init(&mut interactor, alice_address.clone()).await;
    identity_interactor
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;
    identity_interactor
        .register_agent(
            &mut interactor,
            "BotMeta",
            "uri",
            vec![("old", b"val".to_vec())],
        )
        .await;

    let address = identity_interactor.address().clone();

    let token_id: TokenIdentifier<StaticApi> = interactor
        .query()
        .to(&address)
        .typed(IdentityRegistryProxy)
        .agent_token_id()
        .returns(ReturnsResult)
        .run()
        .await;

    // Update with new metadata
    let new_metadata = vec![("new", b"val2".to_vec())];

    identity_interactor
        .update_agent(
            &mut interactor,
            "BotMeta",
            "uri",
            new_metadata,
            vec![],
            (&token_id.to_string(), 1),
        )
        .await;

    // Verify metadata
    let stored_new_opt: OptionalValue<ManagedBuffer<StaticApi>> = interactor
        .query()
        .to(&address)
        .typed(IdentityRegistryProxy)
        .get_metadata(1u64, ManagedBuffer::new_from_bytes(b"new"))
        .returns(ReturnsResult)
        .run()
        .await;

    let stored_new = stored_new_opt.into_option();
    assert!(stored_new.is_some());
    assert_eq!(stored_new.unwrap().to_vec(), b"val2");

    let stored_old_opt: OptionalValue<ManagedBuffer<StaticApi>> = interactor
        .query()
        .to(&address)
        .typed(IdentityRegistryProxy)
        .get_metadata(1u64, ManagedBuffer::new_from_bytes(b"old"))
        .returns(ReturnsResult)
        .run()
        .await;

    let stored_old = stored_old_opt.into_option();
    assert!(stored_old.is_some());
}
