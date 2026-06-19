use crate::common::{IdentityRegistryInteractor, TestEnv};
use identity_registry_interactor::identity_registry_proxy::IdentityRegistryProxy;
use multiversx_sc::types::{ManagedAddress, ManagedBuffer, TokenIdentifier};
use multiversx_sc_snippets::imports::*;

#[tokio::test]
async fn test_update_agent_full() {
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
        .register_agent(&mut interactor, "Bot1", "uri1", vec![])
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
    let token_str = token_id.to_string();

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
