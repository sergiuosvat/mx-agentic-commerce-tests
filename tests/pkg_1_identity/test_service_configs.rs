use crate::common::{IdentityRegistryInteractor, ServiceConfigInput, TestEnv};
use multiversx_sc::types::{BigUint, EgldOrEsdtTokenIdentifier, TokenIdentifier};
use multiversx_sc_snippets::imports::*;

#[tokio::test]
async fn test_service_configs() {
    let env = TestEnv::chain_only().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
    let gateway_url = env.gateway_url.clone();
    let alice_address = env.owner.clone();

    let identity_interactor =
        IdentityRegistryInteractor::init(&mut interactor, alice_address.clone()).await;

    identity_interactor
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;

    let agent_name = "ServiceAgent";
    let agent_uri = "https://agent.io/manifest.json";
    identity_interactor
        .register_agent(&mut interactor, agent_name, agent_uri, vec![])
        .await;

    let nonce = 1;

    let service1 = ServiceConfigInput {
        service_id: 1,
        price: BigUint::from(1_000_000u64),
        token: EgldOrEsdtTokenIdentifier::esdt(TokenIdentifier::from("TOKEN-123456")),
        nonce: 0,
    };
    let service2 = ServiceConfigInput {
        service_id: 2,
        price: BigUint::from(500_000u64),
        token: EgldOrEsdtTokenIdentifier::esdt(TokenIdentifier::from("TOKEN-123456")),
        nonce: 0,
    };

    identity_interactor
        .set_service_configs(
            &mut interactor,
            vec![service1.clone(), service2.clone()],
            nonce,
        )
        .await;

    let service1_updated = ServiceConfigInput {
        service_id: 1,
        price: BigUint::from(2_000_000u64),
        token: EgldOrEsdtTokenIdentifier::esdt(TokenIdentifier::from("TOKEN-123456")),
        nonce: 0,
    };
    identity_interactor
        .set_service_configs(
            &mut interactor,
            vec![service1_updated],
            nonce,
        )
        .await;

    identity_interactor
        .remove_service_configs(&mut interactor, vec![2], nonce)
        .await;

    let bob_address = interactor.register_wallet(test_wallets::bob()).await;
    crate::common::fund_address_on_simulator(
        &bob_address.to_bech32("erd").to_string(),
        "100000000000000000000000",
        &gateway_url,
    )
    .await;

    let mut interactor_bob = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    interactor_bob
        .register_wallet(test_wallets::bob())
        .await;
}
