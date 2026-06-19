use crate::common::{IdentityRegistryInteractor, TestEnv};
use identity_registry_interactor::identity_registry_proxy::IdentityRegistryProxy;
use multiversx_sc::types::ManagedBuffer;
use multiversx_sc_snippets::imports::*;

#[tokio::test]
async fn test_metadata_ops() {
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
        .register_agent(&mut interactor, "BotMeta", "uri", vec![])
        .await;

    let address = identity_interactor.address().clone();

    // 1. Set Metadata (3 items)
    let meta1 = vec![
        ("key1", b"val1".to_vec()),
        ("key2", b"val2".to_vec()),
        ("key3", b"val3".to_vec()),
    ];
    identity_interactor
        .set_metadata(&mut interactor, meta1, 1)
        .await;

    // Verify key1
    let val1_opt: OptionalValue<ManagedBuffer<StaticApi>> = interactor
        .query()
        .to(&address)
        .typed(IdentityRegistryProxy)
        .get_metadata(1u64, ManagedBuffer::new_from_bytes(b"key1"))
        .returns(ReturnsResult)
        .run()
        .await;
    assert_eq!(val1_opt.into_option().unwrap().to_vec(), b"val1");

    // 2. Overwrite key2
    let meta2 = vec![("key2", b"val2_updated".to_vec())];
    identity_interactor
        .set_metadata(&mut interactor, meta2, 1)
        .await;

    let val2_opt: OptionalValue<ManagedBuffer<StaticApi>> = interactor
        .query()
        .to(&address)
        .typed(IdentityRegistryProxy)
        .get_metadata(1u64, ManagedBuffer::new_from_bytes(b"key2"))
        .returns(ReturnsResult)
        .run()
        .await;
    assert_eq!(val2_opt.into_option().unwrap().to_vec(), b"val2_updated");

    // 3. Remove key3
    identity_interactor
        .remove_metadata(&mut interactor, vec!["key3"], 1)
        .await;

    let val3_opt: OptionalValue<ManagedBuffer<StaticApi>> = interactor
        .query()
        .to(&address)
        .typed(IdentityRegistryProxy)
        .get_metadata(1u64, ManagedBuffer::new_from_bytes(b"key3"))
        .returns(ReturnsResult)
        .run()
        .await;
    assert!(val3_opt.into_option().is_none());

    // key1 should still exist
    let val1_check: OptionalValue<ManagedBuffer<StaticApi>> = interactor
        .query()
        .to(&address)
        .typed(IdentityRegistryProxy)
        .get_metadata(1u64, ManagedBuffer::new_from_bytes(b"key1"))
        .returns(ReturnsResult)
        .run()
        .await;
    assert!(val1_check.into_option().is_some());
}
