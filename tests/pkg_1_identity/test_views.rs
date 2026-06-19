use crate::common::{
    create_pem_file, fund_address_on_simulator, generate_random_private_key,
    IdentityRegistryInteractor,
};
use identity_registry_interactor::identity_registry_proxy::IdentityRegistryProxy;
use multiversx_sc::types::{ManagedAddress, ManagedBuffer};
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use tokio::time::{sleep, Duration};

#[tokio::test]
async fn test_views() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    sleep(Duration::from_secs(3)).await;

    let mut interactor = Interactor::new(&gateway_url)
        .await
        .use_chain_simulator(true);

    // Setup Alice
    let alice_private_key = generate_random_private_key();
    let alice_wallet = Wallet::from_private_key(&alice_private_key).unwrap();
    let alice_address = alice_wallet.to_address();
    create_pem_file(
        "alice_views.pem",
        &alice_private_key,
        &alice_address.to_bech32("erd").to_string(),
    );
    interactor.register_wallet(alice_wallet).await;
    fund_address_on_simulator(
        &alice_address.to_bech32("erd").to_string(),
        "100000000000000000000000",
        &gateway_url,
    )
    .await;

    // Deploy & Register
    let identity_interactor =
        IdentityRegistryInteractor::init(&mut interactor, alice_address.clone()).await;
    identity_interactor
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;

    let agent_name = "ViewBot";
    let agent_uri = "https://view.bot";
    identity_interactor
        .register_agent(
            &mut interactor,
            agent_name,
            agent_uri,
            vec![("key1", b"val1".to_vec())],
        )
        .await;

    let contract_address = identity_interactor.address().clone();
    let nonce = 1u64; // First registered agent

    // 1. Test get_agent_owner(nonce)
    let owner: ManagedAddress<StaticApi> = interactor
        .query()
        .to(&contract_address)
        .typed(IdentityRegistryProxy)
        .get_agent_owner(nonce)
        .returns(ReturnsResult)
        .run()
        .await;
    assert_eq!(
        owner.to_address(),
        alice_address,
        "Owner should match Alice"
    );

    // 2. Test get_metadata(nonce, key)
    let metadata_opt: OptionalValue<ManagedBuffer<StaticApi>> = interactor
        .query()
        .to(&contract_address)
        .typed(IdentityRegistryProxy)
        .get_metadata(nonce, ManagedBuffer::new_from_bytes(b"key1"))
        .returns(ReturnsResult)
        .run()
        .await;
    assert!(metadata_opt.is_some(), "Metadata should be present");
    assert_eq!(metadata_opt.into_option().unwrap().to_vec(), b"val1");

    // 3. Test get_metadata non-existent
    let metadata_missing: OptionalValue<ManagedBuffer<StaticApi>> = interactor
        .query()
        .to(&contract_address)
        .typed(IdentityRegistryProxy)
        .get_metadata(nonce, ManagedBuffer::new_from_bytes(b"nonexistent"))
        .returns(ReturnsResult)
        .run()
        .await;
    assert!(
        metadata_missing.into_option().is_none(),
        "Missing metadata should return None"
    );

    // Cleanup
    std::fs::remove_file("alice_views.pem").unwrap_or(());
}
