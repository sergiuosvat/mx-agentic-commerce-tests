use crate::common::{
    IdentityRegistryInteractor, ServiceConfigInput, TestEnv,
};
use identity_registry_interactor::identity_registry_proxy::IdentityRegistryProxy;
use multiversx_sc::types::{
    BigUint, EgldOrEsdtTokenIdentifier, ManagedAddress, ManagedBuffer, TokenIdentifier,
};
use multiversx_sc_snippets::imports::*;

#[tokio::test]
async fn test_basic_registration() {
    let env = TestEnv::chain_only().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
    let alice_address = env.owner.clone();

    let identity_interactor =
        IdentityRegistryInteractor::init(&mut interactor, alice_address.clone()).await;
    identity_interactor
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;

    identity_interactor
        .register_agent(
            &mut interactor,
            "Bot1",
            "https://bot1.io/manifest.json",
            vec![],
        )
        .await;

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
    let env = TestEnv::chain_only().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
    let alice_address = env.owner.clone();

    let identity_interactor =
        IdentityRegistryInteractor::init(&mut interactor, alice_address.clone()).await;
    identity_interactor
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;

    let metadata = vec![
        ("price:default", "1000000".as_bytes().to_vec()),
        ("token:default", "EGLD".as_bytes().to_vec()),
    ];

    identity_interactor
        .register_agent(
            &mut interactor,
            "BotMeta",
            "https://bot-meta.io/manifest.json",
            metadata,
        )
        .await;

    let address = identity_interactor.address().clone();

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
    let env = TestEnv::chain_only().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
    let alice_address = env.owner.clone();

    let identity_interactor =
        IdentityRegistryInteractor::init(&mut interactor, alice_address.clone()).await;
    identity_interactor
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;

    let service = ServiceConfigInput {
        service_id: 1,
        price: BigUint::from(1_000_000_000_000_000_000u64),
        token: EgldOrEsdtTokenIdentifier::esdt(TokenIdentifier::from("TOKEN-123456")),
        nonce: 0,
    };

    identity_interactor
        .register_agent_with_services(
            &mut interactor,
            "BotService",
            "https://bot-service.io",
            vec![],
            vec![service],
        )
        .await;

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
