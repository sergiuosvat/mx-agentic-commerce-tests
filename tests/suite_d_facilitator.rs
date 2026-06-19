use mx_agentic_commerce_tests::ProcessManager;
use multiversx_sc_snippets::imports::*;

mod common;
use common::{
    address_to_bech32, generate_random_private_key, get_simulator_chain_id,
    start_facilitator_with_port, wait_for_simulator_ready, IdentityRegistryInteractor,
};

#[tokio::test]
async fn test_facilitator_flow() {
    let mut pm = ProcessManager::new();

    let port = pm.start_chain_simulator().expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{port}");
    wait_for_simulator_ready(&gateway_url).await;

    let mut interactor = Interactor::new(&gateway_url)
        .await
        .use_chain_simulator(true);
    let wallet_alice = interactor.register_wallet(test_wallets::alice()).await;

    let facilitator_pk = generate_random_private_key();
    let wallet_facilitator_address = interactor
        .register_wallet(Wallet::from_private_key(&facilitator_pk).expect("Failed to create wallet"))
        .await;

    interactor
        .tx()
        .from(&wallet_alice)
        .to(&wallet_facilitator_address)
        .egld(1_000_000_000_000_000_000u64)
        .run()
        .await;

    let identity = IdentityRegistryInteractor::init(&mut interactor, wallet_alice.clone()).await;
    let registry_address = address_to_bech32(identity.address());
    let chain_id = get_simulator_chain_id(&gateway_url).await;

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
        .expect("Failed to call health endpoint");

    assert!(
        resp.status().is_success(),
        "Facilitator health check failed on port {facilitator_port}"
    );

    let body = resp.text().await.unwrap_or_default();
    assert!(
        !body.is_empty(),
        "Facilitator health response should not be empty"
    );
}
