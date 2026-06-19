use multiversx_sc::types::{Address, CodeMetadata, ManagedBuffer};
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use serde_json::json;
use tokio::time::{sleep, Duration};

mod common;
use common::{IdentityRegistryInteractor, REPUTATION_WASM_PATH, VALIDATION_WASM_PATH};

/// Query a contract view via the chain simulator VM query endpoint.
/// Returns the decoded return data as a list of base64-decoded byte arrays.
async fn vm_query(sc_address_bech32: &str, func_name: &str, args_hex: Vec<&str>, gateway_url: &str) -> Vec<Vec<u8>> {
    let client = reqwest::Client::new();
    let body = json!({
        "scAddress": sc_address_bech32,
        "funcName": func_name,
        "args": args_hex,
    });

    let resp: serde_json::Value = client
        .post(format!("{}/vm-values/query", gateway_url))
        .json(&body)
        .send()
        .await
        .expect("Failed VM query")
        .json()
        .await
        .expect("Failed to parse VM query response");

    let return_data = resp["data"]["data"]["returnData"]
        .as_array()
        .expect("No returnData in VM query response");

    return_data
        .iter()
        .map(|v| {
            let b64 = v.as_str().unwrap_or("");
            if b64.is_empty() {
                vec![]
            } else {
                base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)
                    .unwrap_or_default()
            }
        })
        .collect()
}

/// Suite N: Reputation & Validation Loop
///
/// Full 3-registry lifecycle:
/// 1. Deploy identity, validation, reputation
/// 2. Register agent
/// 3. init_job → submit_proof
/// 4. submit_feedback (no authorization needed, ERC-8004)
/// 5. Query reputation score → assert > 0
#[tokio::test]
async fn test_reputation_validation_loop() {
    let mut pm = ProcessManager::new();

    // ── 1. Start Chain Simulator ──
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    sleep(Duration::from_secs(2)).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    let wallet_alice = interactor.register_wallet(test_wallets::alice()).await;

    // ── 2. Deploy Identity Registry ──
    let identity =
        IdentityRegistryInteractor::init(&mut interactor, wallet_alice.clone()).await;
    identity
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;
    let identity_addr = identity.address().clone();
    let identity_bech32 = common::address_to_bech32(&identity_addr);
    println!("Identity Registry: {}", identity_bech32);

    // Register an agent (nonce=1)
    identity
        .register_agent(
            &mut interactor,
            "WorkerBot",
            "https://workerbot.example.com/manifest.json",
            vec![("type", b"worker".to_vec())],
        )
        .await;
    println!("Agent registered: WorkerBot (nonce=1)");

    // ── 3. Deploy Validation Registry ──
    println!("Deploying Validation Registry...");
    let validation_wasm = std::fs::read(VALIDATION_WASM_PATH)
        .expect("Failed to read validation WASM — run setup.sh first");
    let validation_code: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(&validation_wasm);
    let identity_addr_arg: ManagedBuffer<StaticApi> =
        ManagedBuffer::new_from_bytes(identity_addr.as_bytes());

    let validation_addr: Address = interactor
        .tx()
        .from(&wallet_alice)
        .gas(600_000_000)
        .raw_deploy()
        .code(validation_code)
        .code_metadata(CodeMetadata::PAYABLE_BY_SC | CodeMetadata::READABLE)
        .argument(&identity_addr_arg)
        .returns(ReturnsNewAddress)
        .run()
        .await;

    let validation_bech32 = common::address_to_bech32(&validation_addr);
    println!("Validation Registry: {}", validation_bech32);

    // ── 4. Deploy Reputation Registry ──
    println!("Deploying Reputation Registry...");
    let reputation_wasm = std::fs::read(REPUTATION_WASM_PATH)
        .expect("Failed to read reputation WASM — run setup.sh first");
    let reputation_code: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(&reputation_wasm);
    let validation_addr_arg: ManagedBuffer<StaticApi> =
        ManagedBuffer::new_from_bytes(validation_addr.as_bytes());

    let reputation_addr: Address = interactor
        .tx()
        .from(&wallet_alice)
        .gas(600_000_000)
        .raw_deploy()
        .code(reputation_code)
        .code_metadata(CodeMetadata::PAYABLE_BY_SC | CodeMetadata::READABLE)
        .argument(&validation_addr_arg)
        .argument(&identity_addr_arg)
        .returns(ReturnsNewAddress)
        .run()
        .await;

    let reputation_bech32 = common::address_to_bech32(&reputation_addr);
    println!("Reputation Registry: {}", reputation_bech32);

    // ── 5. Init Job on Validation Registry ──
    let job_id = "job-001-test";
    let job_id_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(job_id.as_bytes());
    let agent_nonce: u64 = 1;
    let agent_nonce_buf: ManagedBuffer<StaticApi> =
        ManagedBuffer::new_from_bytes(&agent_nonce.to_be_bytes());

    interactor
        .tx()
        .from(&wallet_alice)
        .to(&validation_addr)
        .gas(20_000_000)
        .raw_call("init_job")
        .argument(&job_id_buf)
        .argument(&agent_nonce_buf)
        .run()
        .await;
    println!("Job initialized: {}", job_id);

    // ── 6. Submit Proof ──
    let proof = "sha256:abc123proof";
    let proof_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(proof.as_bytes());

    interactor
        .tx()
        .from(&wallet_alice)
        .to(&validation_addr)
        .gas(10_000_000)
        .raw_call("submit_proof")
        .argument(&job_id_buf)
        .argument(&proof_buf)
        .run()
        .await;
    println!("Proof submitted: {}", proof);

    // ── 7. validation_request + validation_response ──
    let request_hash = "req-hash-001";
    let request_hash_buf: ManagedBuffer<StaticApi> =
        ManagedBuffer::new_from_bytes(request_hash.as_bytes());
    let validator_managed: ManagedAddress<StaticApi> = ManagedAddress::from_address(&wallet_alice);
    let request_uri_buf: ManagedBuffer<StaticApi> =
        ManagedBuffer::new_from_bytes(b"https://validator.example.com/check");

    interactor
        .tx()
        .from(&wallet_alice)
        .to(&validation_addr)
        .gas(20_000_000)
        .raw_call("validation_request")
        .argument(&job_id_buf)
        .argument(&validator_managed)
        .argument(&request_uri_buf)
        .argument(&request_hash_buf)
        .run()
        .await;
    println!("Validation requested: {}", request_hash);

    // validation_response: approve (1 = approved)
    let response_uri_buf: ManagedBuffer<StaticApi> =
        ManagedBuffer::new_from_bytes(b"https://validator.example.com/result");
    let response_hash_buf: ManagedBuffer<StaticApi> =
        ManagedBuffer::new_from_bytes(b"resp-hash-001");
    let tag_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(b"quality-check");
    let response_code: u8 = 1; // 1 = approved

    interactor
        .tx()
        .from(&wallet_alice)
        .to(&validation_addr)
        .gas(20_000_000)
        .raw_call("validation_response")
        .argument(&request_hash_buf)
        .argument(&response_code)
        .argument(&response_uri_buf)
        .argument(&response_hash_buf)
        .argument(&tag_buf)
        .run()
        .await;
    println!("Validation approved: {}", request_hash);

    // ── 8. Submit Feedback (rating = 5) ──
    let rating: u64 = 5;
    let rating_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(&rating.to_be_bytes());

    interactor
        .tx()
        .from(&wallet_alice)
        .to(&reputation_addr)
        .gas(20_000_000)
        .raw_call("giveFeedbackSimple")
        .argument(&job_id_buf)
        .argument(&agent_nonce_buf)
        .argument(&rating_buf)
        .run()
        .await;
    println!("Feedback submitted: rating={}", rating);

    // ── 11. Query Reputation Score via VM Query ──
    let nonce_hex = hex::encode(agent_nonce.to_be_bytes());
    let result = vm_query(&reputation_bech32, "get_reputation_score", vec![&nonce_hex], &gateway_url).await;
    assert!(!result.is_empty(), "Score query should return data");

    // Parse BigUint bytes as u64
    let score_bytes = &result[0];
    let mut score_val: u64 = 0;
    for byte in score_bytes {
        score_val = (score_val << 8) | (*byte as u64);
    }
    println!(
        "Reputation Score: {} (raw bytes: {:?})",
        score_val, score_bytes
    );
    assert!(
        score_val > 0,
        "Reputation score should be > 0 after positive feedback"
    );

    println!("\nSuite N: Reputation & Validation Loop — PASSED ✅");
    println!("  Deployed: Identity, Validation, Reputation");
    println!("  Flow: init_job → submit_proof → feedback (ERC-8004)");
    println!("  Score: {}", score_val);
}
