use multiversx_sc::types::{Address, BigUint, EgldOrEsdtTokenIdentifier};
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;

use super::{
    address_to_bech32, deploy_all_registries, fund_address_on_simulator,
    wait_for_simulator_ready, IdentityRegistryInteractor, ServiceConfigInput,
};

/// Shared harness for chain-simulator integration tests.
pub struct TestEnv {
    pub pm: ProcessManager,
    pub interactor: Interactor,
    pub gateway_url: String,
    pub owner: Address,
}

impl TestEnv {
    /// Start simulator, fund alice, return ready interactor.
    pub async fn chain_only() -> Self {
        let mut pm = ProcessManager::new();
        let port = pm
            .start_chain_simulator()
            .expect("Failed to start simulator");
        let gateway_url = format!("http://localhost:{port}");
        wait_for_simulator_ready(&gateway_url).await;

        let mut interactor = Interactor::new(&gateway_url)
            .await
            .use_chain_simulator(true);
        let owner = interactor.register_wallet(test_wallets::alice()).await;
        fund_address_on_simulator(
            &address_to_bech32(&owner),
            "100000000000000000000000",
            &gateway_url,
        )
        .await;

        Self {
            pm,
            interactor,
            gateway_url,
            owner,
        }
    }

    /// Chain-only setup plus all three registries deployed.
    pub async fn with_registries(
    ) -> (Self, IdentityRegistryInteractor, Address, Address) {
        let mut env = Self::chain_only().await;
        let (identity, validation_addr, reputation_addr) =
            deploy_all_registries(&mut env.interactor, env.owner.clone()).await;
        (env, identity, validation_addr, reputation_addr)
    }

    /// Full validation test fixture: registries + registered PayBot agent.
    pub async fn with_validation_agent(
    ) -> (Self, Address, IdentityRegistryInteractor) {
        let (mut env, identity, validation_addr, _) =
            Self::with_registries().await;

        let service = ServiceConfigInput::<StaticApi> {
            service_id: 1,
            price: BigUint::<StaticApi>::from(1_000_000_000_000_000_000u64),
            token: EgldOrEsdtTokenIdentifier::<StaticApi>::egld(),
            nonce: 0,
        };
        identity
            .register_agent_with_services(
                &mut env.interactor,
                "PayBot",
                "uri",
                vec![],
                vec![service],
            )
            .await;

        (env, validation_addr, identity)
    }
}
