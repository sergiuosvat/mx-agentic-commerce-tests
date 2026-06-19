use multiversx_sc_snippets::imports::*;

use crate::common::{
    address_to_bech32, generate_random_private_key, get_simulator_chain_id,
    start_facilitator_with_port, IdentityRegistryInteractor, TestEnv,
};

#[tokio::test]
async fn test_verify_egld() {
    let env = TestEnv::chain_only().await;
    let mut pm = env.pm;
    let gateway_url = env.gateway_url.clone();
    let mut interactor = env.interactor;

    let facilitator_pk = generate_random_private_key();
    let chain_id = get_simulator_chain_id(&gateway_url).await;

    let owner = env.owner.clone();
    interactor.register_wallet(test_wallets::bob()).await;

    let sender_bech32 = address_to_bech32(&owner);
    crate::common::fund_address_on_simulator(
        &sender_bech32,
        "100000000000000000000",
        &gateway_url,
    )
    .await;

    let identity = IdentityRegistryInteractor::init(&mut interactor, owner).await;
    let registry_address = address_to_bech32(identity.address());

    let (facilitator_port, facilitator_url) = start_facilitator_with_port(
        &mut pm,
        &facilitator_pk,
        &registry_address,
        &gateway_url,
        &chain_id,
        &[],
    )
    .await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{facilitator_url}/health"))
        .send()
        .await
        .expect("Failed to call facilitator health endpoint");

    assert!(
        resp.status().is_success(),
        "Facilitator health check failed on port {facilitator_port}"
    );
}
