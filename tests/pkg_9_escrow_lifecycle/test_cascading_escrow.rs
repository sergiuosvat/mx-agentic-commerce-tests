use multiversx_sc::types::ManagedBuffer;
use multiversx_sc_snippets::imports::*;

use crate::common::{TestEnv, 
    deploy_all_registries, vm_query, EscrowInteractor, EscrowStatus,
};

/// T-003: Agent-to-Agent Cascading Escrow
/// A hires B (escrow-ab), B sub-hires C (escrow-bc)
/// C delivers → proof → validate → B releases C escrow
/// B delivers → proof → validate → A releases B escrow
/// Both get feedback, all states verified
#[tokio::test]
async fn test_cascading_escrow_chain() {
    let env = TestEnv::chain_only().await;
    std::mem::forget(env.pm);
    let mut interactor = env.interactor;

    let alice = interactor.register_wallet(test_wallets::alice()).await;
    let bob = interactor.register_wallet(test_wallets::bob()).await;
    let carol = interactor.register_wallet(test_wallets::carol()).await;

    // 1. Deploy infrastructure
    let (identity, validation_addr, reputation_addr) =
        deploy_all_registries(&mut interactor, alice.clone()).await;

    let escrow = EscrowInteractor::deploy(
        &mut interactor,
        alice.clone(),
        &validation_addr,
        identity.address(),
    )
    .await;

    // 2. Register 3 agents
    identity
        .register_agent(&mut interactor, "AlphaAgent", "https://alpha.ai", vec![])
        .await;

    let name_b = ManagedBuffer::<StaticApi>::new_from_bytes(b"BetaAgent");
    let uri_b = ManagedBuffer::<StaticApi>::new_from_bytes(b"https://beta.ai");
    let pk_b = ManagedBuffer::<StaticApi>::new_from_bytes(&[0u8; 32]);
    let zero_count = ManagedBuffer::<StaticApi>::new_from_bytes(&0u32.to_be_bytes());
    interactor
        .tx()
        .from(&bob)
        .to(&identity.contract_address)
        .gas(600_000_000)
        .raw_call("register_agent")
        .argument(&name_b)
        .argument(&uri_b)
        .argument(&pk_b)
        .argument(&zero_count)
        .argument(&zero_count)
        .run()
        .await;
    let agent_b_nonce: u64 = 2;

    let name_c = ManagedBuffer::<StaticApi>::new_from_bytes(b"GammaAgent");
    let uri_c = ManagedBuffer::<StaticApi>::new_from_bytes(b"https://gamma.ai");
    let pk_c = ManagedBuffer::<StaticApi>::new_from_bytes(&[0u8; 32]);
    interactor
        .tx()
        .from(&carol)
        .to(&identity.contract_address)
        .gas(600_000_000)
        .raw_call("register_agent")
        .argument(&name_c)
        .argument(&uri_c)
        .argument(&pk_c)
        .argument(&zero_count)
        .argument(&zero_count)
        .run()
        .await;
    let agent_c_nonce: u64 = 3;
    println!("✓ 3 agents registered");

    // 3. Alice deposits escrow for Bob (job-ab): 5 EGLD
    let job_ab = "cascade-job-ab";
    let job_ab_buf = ManagedBuffer::<StaticApi>::new_from_bytes(job_ab.as_bytes());
    let bob_addr: ManagedAddress<StaticApi> = ManagedAddress::from_address(&bob);
    let poa_ab = ManagedBuffer::<StaticApi>::new_from_bytes(b"poa-ab");
    let deadline: u64 = 9_999_999_999;

    interactor
        .tx()
        .from(&alice)
        .to(escrow.address())
        .gas(600_000_000)
        .egld(5_000_000_000_000_000_000u64)
        .raw_call("deposit")
        .argument(&job_ab_buf)
        .argument(&bob_addr)
        .argument(&poa_ab)
        .argument(&deadline)
        .run()
        .await;
    println!("✓ A deposited 5 EGLD escrow for B (job-ab)");

    // 4. Bob deposits escrow for Carol (job-bc): 2 EGLD (subset of what he'll earn)
    let job_bc = "cascade-job-bc";
    let job_bc_buf = ManagedBuffer::<StaticApi>::new_from_bytes(job_bc.as_bytes());
    let carol_addr: ManagedAddress<StaticApi> = ManagedAddress::from_address(&carol);
    let poa_bc = ManagedBuffer::<StaticApi>::new_from_bytes(b"poa-bc");

    interactor
        .tx()
        .from(&bob)
        .to(escrow.address())
        .gas(600_000_000)
        .egld(2_000_000_000_000_000_000u64)
        .raw_call("deposit")
        .argument(&job_bc_buf)
        .argument(&carol_addr)
        .argument(&poa_bc)
        .argument(&deadline)
        .run()
        .await;
    println!("✓ B deposited 2 EGLD escrow for C (job-bc)");

    // 5. Verify both escrows are Active
    let data_ab = escrow.get_escrow(&mut interactor, job_ab).await;
    assert_eq!(data_ab.status, EscrowStatus::Active);
    let data_bc = escrow.get_escrow(&mut interactor, job_bc).await;
    assert_eq!(data_bc.status, EscrowStatus::Active);
    println!("✓ Both escrows Active");

    // 6. Init jobs on validation
    interactor
        .tx()
        .from(&alice)
        .to(&validation_addr)
        .gas(10_000_000)
        .raw_call("init_job")
        .argument(&job_ab_buf)
        .argument(&agent_b_nonce)
        .run()
        .await;

    interactor
        .tx()
        .from(&bob)
        .to(&validation_addr)
        .gas(10_000_000)
        .raw_call("init_job")
        .argument(&job_bc_buf)
        .argument(&agent_c_nonce)
        .run()
        .await;
    println!("✓ Both jobs initialized");

    // 7. C delivers proof for job-bc
    let proof_c = ManagedBuffer::<StaticApi>::new_from_bytes(b"gamma-delivery");
    interactor
        .tx()
        .from(&carol)
        .to(&validation_addr)
        .gas(10_000_000)
        .raw_call("submit_proof")
        .argument(&job_bc_buf)
        .argument(&proof_c)
        .run()
        .await;

    // 8. Validate job-bc: Carol (agent owner nonce=3) requests, Bob (validator) responds
    let req_hash_bc = ManagedBuffer::<StaticApi>::new_from_bytes(b"req-bc");
    let validator_bob: ManagedAddress<StaticApi> = ManagedAddress::from_address(&bob);
    let req_uri = ManagedBuffer::<StaticApi>::new_from_bytes(b"https://v.io/check");

    interactor
        .tx()
        .from(&carol) // Carol is agent owner for nonce=3
        .to(&validation_addr)
        .gas(20_000_000)
        .raw_call("validation_request")
        .argument(&job_bc_buf)
        .argument(&validator_bob)
        .argument(&req_uri)
        .argument(&req_hash_bc)
        .run()
        .await;

    let resp_uri = ManagedBuffer::<StaticApi>::new_from_bytes(b"https://v.io/result");
    let resp_hash = ManagedBuffer::<StaticApi>::new_from_bytes(b"resp-bc");
    let tag = ManagedBuffer::<StaticApi>::new_from_bytes(b"qa");
    let approved: u8 = 1;

    interactor
        .tx()
        .from(&bob) // Bob is the validator_address
        .to(&validation_addr)
        .gas(20_000_000)
        .raw_call("validation_response")
        .argument(&req_hash_bc)
        .argument(&approved)
        .argument(&resp_uri)
        .argument(&resp_hash)
        .argument(&tag)
        .run()
        .await;
    println!("✓ job-bc validated (Verified)");

    // 9. B releases escrow for C (job-bc)
    interactor
        .tx()
        .from(&bob)
        .to(escrow.address())
        .gas(600_000_000)
        .raw_call("release")
        .argument(&job_bc_buf)
        .run()
        .await;

    let data_bc_released = escrow.get_escrow(&mut interactor, job_bc).await;
    assert_eq!(data_bc_released.status, EscrowStatus::Released);
    println!("✓ B released escrow for C → C got 2 EGLD");

    // 10. B submits proof for job-ab (using C's delivery)
    let proof_b = ManagedBuffer::<StaticApi>::new_from_bytes(b"beta-aggregated");
    interactor
        .tx()
        .from(&bob)
        .to(&validation_addr)
        .gas(10_000_000)
        .raw_call("submit_proof")
        .argument(&job_ab_buf)
        .argument(&proof_b)
        .run()
        .await;

    // 11. Validate job-ab: Bob (agent owner nonce=2) requests, Alice (validator) responds
    let req_hash_ab = ManagedBuffer::<StaticApi>::new_from_bytes(b"req-ab");
    let validator_alice: ManagedAddress<StaticApi> = ManagedAddress::from_address(&alice);

    interactor
        .tx()
        .from(&bob) // Bob is agent owner for nonce=2
        .to(&validation_addr)
        .gas(20_000_000)
        .raw_call("validation_request")
        .argument(&job_ab_buf)
        .argument(&validator_alice)
        .argument(&req_uri)
        .argument(&req_hash_ab)
        .run()
        .await;

    let resp_hash_ab = ManagedBuffer::<StaticApi>::new_from_bytes(b"resp-ab");
    interactor
        .tx()
        .from(&alice) // Alice is the validator_address
        .to(&validation_addr)
        .gas(20_000_000)
        .raw_call("validation_response")
        .argument(&req_hash_ab)
        .argument(&approved)
        .argument(&resp_uri)
        .argument(&resp_hash_ab)
        .argument(&tag)
        .run()
        .await;
    println!("✓ job-ab validated (Verified)");

    // 12. A releases escrow for B (job-ab)
    interactor
        .tx()
        .from(&alice)
        .to(escrow.address())
        .gas(600_000_000)
        .raw_call("release")
        .argument(&job_ab_buf)
        .run()
        .await;

    let data_ab_released = escrow.get_escrow(&mut interactor, job_ab).await;
    assert_eq!(data_ab_released.status, EscrowStatus::Released);
    println!("✓ A released escrow for B → B got 5 EGLD");

    // 13. Feedback — A rates B, B rates C
    interactor
        .tx()
        .from(&bob)
        .to(&reputation_addr)
        .gas(10_000_000)
        .raw_call("giveFeedbackSimple")
        .argument(&job_bc_buf)
        .argument(&agent_c_nonce)
        .argument(&90u64)
        .run()
        .await;

    interactor
        .tx()
        .from(&alice)
        .to(&reputation_addr)
        .gas(10_000_000)
        .raw_call("giveFeedbackSimple")
        .argument(&job_ab_buf)
        .argument(&agent_b_nonce)
        .argument(&85u64)
        .run()
        .await;
    println!("✓ Feedback: B=85, C=90");

    // 14. Verify reputation scores
    let nonce_b_buf = ManagedBuffer::<StaticApi>::new_from_bytes(&agent_b_nonce.to_be_bytes());
    let score_b: u64 = vm_query(
        &mut interactor,
        &reputation_addr,
        "get_reputation_score",
        vec![nonce_b_buf],
    )
    .await;

    let nonce_c_buf = ManagedBuffer::<StaticApi>::new_from_bytes(&agent_c_nonce.to_be_bytes());
    let score_c: u64 = vm_query(
        &mut interactor,
        &reputation_addr,
        "get_reputation_score",
        vec![nonce_c_buf],
    )
    .await;

    assert_eq!(score_b, 85, "B should have score 85");
    assert_eq!(score_c, 90, "C should have score 90");
    println!("✓ Scores verified: B={}, C={}", score_b, score_c);

    println!("✅ T-003 PASSED: Cascading escrow chain A→B→C complete");
}
