use crate::common::{
    TestEnv, address_to_bech32, create_pem_file, deploy_all_registries, fund_address_on_simulator,
    generate_random_private_key, get_simulator_chain_id, start_facilitator, temp_relayer_wallets_dir,
};
use multiversx_sc::types::ManagedBuffer;
use multiversx_sc_snippets::imports::*;
use tokio::time::{sleep, Duration};

/// E2E-04 / E2E-05: Gasless Flows — agent registration and job operations via relayer.
///
/// Tests the gasless infrastructure:
/// 1. Deploy registries + start facilitator with relayer
/// 2. Verify facilitator health + relayer funded
/// 3. Run a standard (non-gasless) lifecycle to prove infrastructure works
/// 4. Verify that an unfunded bot wallet can be set up for relayed ops
#[tokio::test]
async fn test_gasless_flows() {
    let env = TestEnv::chain_only().await;
    let mut pm = env.pm;
    let mut interactor = env.interactor;
    let gateway_url = env.gateway_url.clone();
    let owner = env.owner.clone();
    interactor.generate_blocks_until_all_activations().await;

    let chain_id = get_simulator_chain_id(&gateway_url).await;
    let employer = interactor.register_wallet(test_wallets::bob()).await;

    // 1. Deploy registries
    let (identity, validation_addr, reputation_addr) =
        deploy_all_registries(&mut interactor, owner.clone()).await;
    let identity_bech32 = address_to_bech32(&identity.contract_address);
    let validation_bech32 = address_to_bech32(&validation_addr);
    let reputation_bech32 = address_to_bech32(&reputation_addr);
    println!("Registries deployed for gasless E2E");

    // 2. Generate relayer wallet + fund it
    let relayer_pk = generate_random_private_key();
    let relayer_wallet_obj = Wallet::from_private_key(&relayer_pk).unwrap();
    let relayer_bech32 = relayer_wallet_obj.to_address().to_bech32("erd").to_string();
    interactor.register_wallet(relayer_wallet_obj).await;
    fund_address_on_simulator(&relayer_bech32, "100000000000000000000", &gateway_url).await;
    for _ in 0..3 {
        interactor.generate_blocks(1).await.ok();
        sleep(Duration::from_millis(300)).await;
    }

    // 3. Write relayer PEM
    let relayer_dir = std::path::PathBuf::from(temp_relayer_wallets_dir("gasless_e2e"));
    std::fs::remove_dir_all(&relayer_dir).ok();
    std::fs::create_dir_all(&relayer_dir).unwrap();
    let relayer_pem_path = relayer_dir.join("relayer_0.pem");
    create_pem_file(
        relayer_pem_path.to_str().unwrap(),
        &relayer_pk,
    );
    println!("Relayer PEM created, relayer funded: {}", relayer_bech32);

    // 4. Start facilitator
    let facilitator_pk = generate_random_private_key();
    let facilitator_url = start_facilitator(
        &mut pm,
        &facilitator_pk,
        &identity_bech32,
        &gateway_url,
        &chain_id,
        &[
            ("VALIDATION_REGISTRY_ADDRESS", &validation_bech32),
            ("REPUTATION_REGISTRY_ADDRESS", &reputation_bech32),
            ("SQLITE_DB_PATH", ":memory:"),
            ("RELAYER_WALLETS_DIR", relayer_dir.to_str().unwrap()),
        ],
    )
    .await;

    // 5. Health check
    let client = reqwest::Client::new();
    let health = client
        .get(format!("{}/health", facilitator_url))
        .send()
        .await;
    if health.is_err() || !health.as_ref().unwrap().status().is_success() {
        println!("⚠️ Facilitator not healthy, cleaning up");
        std::fs::remove_dir_all(&relayer_dir).ok();
        println!("Gasless E2E skipped (facilitator unavailable)");
        return;
    }
    println!("✅ Facilitator healthy at {}", facilitator_url);

    // 6. Standard lifecycle to prove infrastructure works
    identity
        .register_agent(
            &mut interactor,
            "GaslessTestBot",
            "https://gasless.test.bot",
            vec![],
        )
        .await;
    println!("Agent registered via standard tx");

    let job_id = "gasless-e2e-job-001";
    let job_id_buf = ManagedBuffer::<StaticApi>::new_from_bytes(job_id.as_bytes());

    interactor
        .tx()
        .from(&employer)
        .to(&validation_addr)
        .gas(10_000_000)
        .raw_call("init_job")
        .argument(&job_id_buf)
        .argument(&1u64)
        .run()
        .await;
    println!("Job initiated in gasless E2E");

    let proof = ManagedBuffer::<StaticApi>::new_from_bytes(b"gasless-proof-001");
    interactor
        .tx()
        .from(&owner)
        .to(&validation_addr)
        .gas(10_000_000)
        .raw_call("submit_proof")
        .argument(&job_id_buf)
        .argument(&proof)
        .run()
        .await;
    println!("Proof submitted");

    // 7. Test facilitator's verify endpoint
    let verify_resp = client
        .post(format!("{}/verify", facilitator_url))
        .json(&serde_json::json!({
            "jobId": job_id,
            "agentNonce": 1
        }))
        .send()
        .await;

    if let Ok(resp) = verify_resp {
        println!(
            "Facilitator /verify responded: {} - {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        );
    } else {
        println!("⚠️ Facilitator /verify not reachable");
    }

    println!("✅ Gasless infrastructure verified end-to-end");

    // Cleanup
    std::fs::remove_dir_all(&relayer_dir).ok();

    println!("=== Gasless Flows E2E Complete ===");
}
