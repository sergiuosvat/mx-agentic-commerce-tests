use multiversx_sc::types::ManagedBuffer;
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;

use crate::common::{deploy_all_registries, vm_query, wait_for_simulator_ready};

/// E2E-06: Chain of Command — Agent A hires Agent B, who sub-hires Agent C.
/// Tests multi-level delegation: A→B→C, each with separate job tracking and reputation.
#[tokio::test]
async fn test_chain_of_command() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    interactor.generate_blocks_until_all_activations().await;

    let alice = interactor.register_wallet(test_wallets::alice()).await;
    let bob = interactor.register_wallet(test_wallets::bob()).await;
    let carol = interactor.register_wallet(test_wallets::carol()).await;

    // 1. Deploy all registries
    let (identity, validation_addr, reputation_addr) =
        deploy_all_registries(&mut interactor, alice.clone()).await;
    println!("Deployed registries for chain of command");

    // 2. Register 3 agents: A (Alice), B (Bob), C (Carol)
    identity
        .register_agent(&mut interactor, "AgentAlpha", "https://alpha.ai", vec![])
        .await;
    let agent_a_nonce: u64 = 1;

    // Agent B - register via raw tx from Bob
    let name_b = ManagedBuffer::<StaticApi>::new_from_bytes(b"AgentBeta");
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

    // Agent C - register via raw tx from Carol
    let name_c = ManagedBuffer::<StaticApi>::new_from_bytes(b"AgentGamma");
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
    println!(
        "Agents registered: A({}), B({}), C({})",
        agent_a_nonce, agent_b_nonce, agent_c_nonce
    );

    // 3. Agent A hires Agent B (job-ab)
    let job_ab = "chain-job-ab";
    let job_ab_buf = ManagedBuffer::<StaticApi>::new_from_bytes(job_ab.as_bytes());
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
    println!("A hires B: {}", job_ab);

    // 4. Agent B sub-hires Agent C (job-bc)
    let job_bc = "chain-job-bc";
    let job_bc_buf = ManagedBuffer::<StaticApi>::new_from_bytes(job_bc.as_bytes());
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
    println!("B sub-hires C: {}", job_bc);

    // 5. Agent C submits proof for job-bc
    let proof_c = ManagedBuffer::<StaticApi>::new_from_bytes(b"gamma-work-output");
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
    println!("C submitted proof for {}", job_bc);

    // 6. Agent B submits proof for job-ab (using C's output)
    let proof_b = ManagedBuffer::<StaticApi>::new_from_bytes(b"beta-aggregated-output");
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
    println!("B submitted proof for {}", job_ab);


    // Bob rates C
    interactor
        .tx()
        .from(&bob)
        .to(&reputation_addr)
        .gas(10_000_000)
        .raw_call("giveFeedbackSimple")
        .argument(&job_bc_buf)
        .argument(&agent_c_nonce)
        .argument(&95u64)
        .run()
        .await;
    println!("B rated C: 95");

    // Alice rates B (ERC-8004: no authorization needed)
    interactor
        .tx()
        .from(&alice)
        .to(&reputation_addr)
        .gas(10_000_000)
        .raw_call("giveFeedbackSimple")
        .argument(&job_ab_buf)
        .argument(&agent_b_nonce)
        .argument(&88u64)
        .run()
        .await;
    println!("A rated B: 88");

    // 11. Verify reputations
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

    let nonce_a_buf = ManagedBuffer::<StaticApi>::new_from_bytes(&agent_a_nonce.to_be_bytes());
    let score_a: u64 = vm_query(
        &mut interactor,
        &reputation_addr,
        "get_reputation_score",
        vec![nonce_a_buf],
    )
    .await;

    assert_eq!(score_a, 0, "A was never rated");
    assert_eq!(score_b, 88, "B was rated 88 by A");
    assert_eq!(score_c, 95, "C was rated 95 by B");
    println!(
        "✅ Reputations: A={}, B={}, C={}",
        score_a, score_b, score_c
    );

    println!("=== Chain of Command E2E Complete ===");
}
