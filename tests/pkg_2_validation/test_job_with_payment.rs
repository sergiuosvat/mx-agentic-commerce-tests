use crate::common::{
    fund_address_on_simulator, generate_random_private_key, IdentityRegistryInteractor,
    ServiceConfigInput, TestEnv, ValidationRegistryInteractor,
};
use multiversx_sc::types::{BigUint, EgldOrEsdtTokenIdentifier};
use multiversx_sc_snippets::imports::*;

#[tokio::test]
async fn test_job_with_payment() {
    let env = TestEnv::chain_only().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
    let gateway_url = env.gateway_url.clone();
    let owner_address = env.owner.clone();

    let identity_interactor =
        IdentityRegistryInteractor::init(&mut interactor, owner_address.clone()).await;
    identity_interactor
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;

    let service_cost_egld = BigUint::from(1_000_000_000_000_000_000u64);
    let service1 = ServiceConfigInput {
        service_id: 1,
        price: service_cost_egld.clone(),
        token: EgldOrEsdtTokenIdentifier::egld(),
        nonce: 0,
    };

    identity_interactor
        .register_agent_with_services(
            &mut interactor,
            "PaidServiceBot",
            "uri",
            vec![],
            vec![service1],
        )
        .await;

    let agent_nonce = 1;

    let validation_interactor = ValidationRegistryInteractor::init(
        &mut interactor,
        owner_address.clone(),
        identity_interactor.address(),
    )
    .await;

    let employer_private_key = generate_random_private_key();
    let employer_wallet = Wallet::from_private_key(&employer_private_key).unwrap();
    let employer_address = employer_wallet.to_address();

    fund_address_on_simulator(
        &employer_address.to_bech32("erd").to_string(),
        "50000000000000000000000",
        &gateway_url,
    )
    .await;

    let mut interactor_employer = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    interactor_employer.register_wallet(employer_wallet).await;

    let contract_address = validation_interactor.contract_address.clone();
    let employer_validation_interactor = ValidationRegistryInteractor {
        wallet_address: employer_address.clone(),
        contract_address,
    };

    let job_id = "paid-job-001";
    let payment_amount = 1_000_000_000_000_000_000u64;

    employer_validation_interactor
        .init_job_with_payment(
            &mut interactor_employer,
            job_id,
            agent_nonce,
            1,
            "EGLD",
            payment_amount,
        )
        .await;
}
