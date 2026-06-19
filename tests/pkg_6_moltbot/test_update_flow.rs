use identity_registry_interactor::identity_registry_proxy::IdentityRegistryProxy;
use multiversx_sc::types::{ManagedAddress, TokenIdentifier};
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use tokio::time::{sleep, Duration};

use crate::common::{deploy_all_registries};

/// Test the update_agent flow: register → update name/uri → verify owner unchanged.
#[tokio::test]
async fn test_update_agent_flow() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    sleep(Duration::from_secs(2)).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    interactor.generate_blocks_until_all_activations().await;

    let owner = interactor.register_wallet(test_wallets::alice()).await;

    // 1. Deploy registries
    let (identity, _, _) = deploy_all_registries(&mut interactor, owner.clone()).await;
    println!("Registries deployed");

    // 2. Register agent
    identity
        .register_agent(
            &mut interactor,
            "OriginalBot",
            "https://original.example.com",
            vec![],
        )
        .await;
    println!("Agent registered: OriginalBot");

    // 3. Get token ID via typed proxy
    let contract_addr = identity.contract_address.clone();
    let token_id: TokenIdentifier<StaticApi> = interactor
        .query()
        .to(&contract_addr)
        .typed(IdentityRegistryProxy)
        .agent_token_id()
        .returns(ReturnsResult)
        .run()
        .await;
    let token_str = token_id.to_string();
    println!("Token ID: {}", token_str);

    // 4. Update agent name and URI
    identity
        .update_agent(
            &mut interactor,
            "UpdatedBot",
            "https://updated.example.com",
            vec![],
            vec![],
            (&token_str, 1u64),
        )
        .await;
    println!("Agent updated: UpdatedBot");

    // 5. Verify owner still correct after update
    let owner_managed: ManagedAddress<StaticApi> = interactor
        .query()
        .to(&contract_addr)
        .typed(IdentityRegistryProxy)
        .get_agent_owner(1u64)
        .returns(ReturnsResult)
        .run()
        .await;

    assert_eq!(
        owner_managed.to_address(),
        test_wallets::alice().to_address()
    );
    println!("✅ Owner unchanged after update");

    println!("=== Update Agent Flow Complete ===");
}
