use crate::common::{
    IdentityRegistryInteractor, TestEnv, ValidationRegistryInteractor,
};
use multiversx_sc_snippets::imports::*;

#[tokio::test]
async fn test_job_lifecycle() {
    let env = TestEnv::chain_only().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;
    let owner_address = env.owner.clone();

    let identity_interactor =
        IdentityRegistryInteractor::init(&mut interactor, owner_address.clone()).await;
    identity_interactor
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;

    identity_interactor
        .register_agent(&mut interactor, "WorkerBot", "uri", vec![])
        .await;

    let agent_nonce = 1;

    let validation_interactor = ValidationRegistryInteractor::init(
        &mut interactor,
        owner_address.clone(),
        identity_interactor.address(),
    )
    .await;

    let job_id = "job-001";
    validation_interactor
        .init_job(&mut interactor, job_id, agent_nonce)
        .await;

    let proof_hash = "QmProofHash123";
    validation_interactor
        .submit_proof(&mut interactor, job_id, proof_hash)
        .await;

    validation_interactor
        .validation_request(
            &mut interactor,
            job_id,
            &owner_address,
            "https://val.uri",
            "req_hash_001",
        )
        .await;
    validation_interactor
        .validation_response(
            &mut interactor,
            "req_hash_001",
            80,
            "https://resp.uri",
            "resp_hash_001",
            "quality",
        )
        .await;
}
