use crate::common::{IdentityRegistryInteractor, TestEnv};
use identity_registry_interactor::identity_registry_proxy::IdentityRegistryProxy;
use multiversx_sc::types::{ManagedAddress, ManagedBuffer};
use multiversx_sc_snippets::imports::*;

#[tokio::test]
async fn test_views() {
    let env = TestEnv::chain_only().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
    let alice_address = env.owner.clone();

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
    let nonce = 1u64;

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
}
