use crate::common::{IdentityRegistryInteractor, TestEnv};
use identity_registry_interactor::identity_registry_proxy::IdentityRegistryProxy;
use multiversx_sc::types::TokenIdentifier;
use multiversx_sc_snippets::imports::*;

#[tokio::test]
async fn test_token_issuance_happy_path() {
    let env = TestEnv::chain_only().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
    let alice_address = env.owner.clone();

    let identity_interactor =
        IdentityRegistryInteractor::init(&mut interactor, alice_address.clone()).await;

    identity_interactor
        .issue_token(&mut interactor, "AgentToken", "AGENT")
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

    assert!(
        token_id.to_string().starts_with("AGENT-"),
        "Token ID should start with AGENT-"
    );
}

#[tokio::test]
async fn test_token_issuance_errors() {
    let env = TestEnv::chain_only().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;

    let bob_address = interactor.register_wallet(test_wallets::bob()).await;

    let identity_interactor =
        IdentityRegistryInteractor::init(&mut interactor, bob_address.clone()).await;

    identity_interactor
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;

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
