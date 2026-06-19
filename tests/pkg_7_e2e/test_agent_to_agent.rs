use multiversx_sc::types::ManagedBuffer;
use multiversx_sc_snippets::imports::*;

use crate::common::{TestEnv, deploy_all_registries, vm_query};

/// E2E-02: Agent A hires Agent B. Agent B does the work, submits proof, gets verified and rated.
#[tokio::test]
async fn test_agent_to_agent_hiring() {
    let env = TestEnv::chain_only().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;

    interactor.generate_blocks_until_all_activations().await;

    let alice = interactor.register_wallet(test_wallets::alice()).await;
    let bob = interactor.register_wallet(test_wallets::bob()).await;

    // 1. Deploy Infrastructure
    let (identity, validation_addr, reputation_addr) =
        deploy_all_registries(&mut interactor, alice.clone()).await;
    println!("Deployed all registries");

    // 2. Register Agent A (owner=Alice)
    identity
        .register_agent(
            &mut interactor,
            "AgentAlpha",
            "https://alpha.agent.io",
            vec![],
        )
        .await;
    let agent_a_nonce: u64 = 1;
    println!("Agent A registered (nonce={})", agent_a_nonce);

    // 3. Register Agent B (owner=Bob) via raw tx
    let name_b = ManagedBuffer::<StaticApi>::new_from_bytes(b"AgentBeta");
    let uri_b = ManagedBuffer::<StaticApi>::new_from_bytes(b"https://beta.agent.io");
    let pk_b = ManagedBuffer::<StaticApi>::new_from_bytes(&[0u8; 32]);
    let metadata_count_b: u32 = 0;
    let metadata_count_buf_b =
        ManagedBuffer::<StaticApi>::new_from_bytes(&metadata_count_b.to_be_bytes());
    let services_count_b: u32 = 0;
    let services_count_buf_b =
        ManagedBuffer::<StaticApi>::new_from_bytes(&services_count_b.to_be_bytes());

    interactor
        .tx()
        .from(&bob)
        .to(&identity.contract_address)
        .gas(600_000_000)
        .raw_call("register_agent")
        .argument(&name_b)
        .argument(&uri_b)
        .argument(&pk_b)
        .argument(&metadata_count_buf_b)
        .argument(&services_count_buf_b)
        .run()
        .await;
    let agent_b_nonce: u64 = 2;
    println!("Agent B registered (nonce={})", agent_b_nonce);

    // 4. Agent A (Alice) hires Agent B: init_job
    let job_id = "job-a2a-001";
    let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(job_id.as_bytes());

    interactor
        .tx()
        .from(&alice)
        .to(&validation_addr)
        .gas(10_000_000)
        .raw_call("init_job")
        .argument(&job_id_buf)
        .argument(&agent_b_nonce)
        .run()
        .await;
    println!("Job initialized: {} (Agent A hires Agent B)", job_id);

    // 5. Agent B (Bob) completes work and submits proof
    let proof = ManagedBuffer::<StaticApi>::new_from_bytes(b"a2a-proof-data-v1");
    interactor
        .tx()
        .from(&bob)
        .to(&validation_addr)
        .gas(10_000_000)
        .raw_call("submit_proof")
        .argument(&job_id_buf)
        .argument(&proof)
        .run()
        .await;
    println!("Agent B submitted proof");


    let rating: u64 = 90;
    interactor
        .tx()
        .from(&alice)
        .to(&reputation_addr)
        .gas(10_000_000)
        .raw_call("giveFeedbackSimple")
        .argument(&job_id_buf)
        .argument(&agent_b_nonce)
        .argument(&rating)
        .run()
        .await;
    println!("Feedback submitted: rating={}", rating);

    // 9. Verify Agent B's reputation
    let nonce_buf = ManagedBuffer::<StaticApi>::new_from_bytes(&agent_b_nonce.to_be_bytes());
    let score: u64 = vm_query(
        &mut interactor,
        &reputation_addr,
        "get_reputation_score",
        vec![nonce_buf.clone()],
    )
    .await;

    assert_eq!(score, rating, "Agent B score should match the rating");
    println!("✅ Agent B reputation: {} (expected {})", score, rating);

    // 10. Verify Agent A still has no reputation (it was hired, not rated)
    let nonce_a_buf = ManagedBuffer::<StaticApi>::new_from_bytes(&agent_a_nonce.to_be_bytes());
    let score_a: u64 = vm_query(
        &mut interactor,
        &reputation_addr,
        "get_reputation_score",
        vec![nonce_a_buf],
    )
    .await;
    assert_eq!(
        score_a, 0,
        "Agent A should have no reputation (was not rated)"
    );
    println!("✅ Agent A reputation: {} (expected 0)", score_a);

    println!("=== Agent-to-Agent Hiring E2E Complete ===");
}
