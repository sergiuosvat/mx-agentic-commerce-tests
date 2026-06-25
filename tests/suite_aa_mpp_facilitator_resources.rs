use mx_agentic_commerce_tests::ProcessManager;
use multiversx_sc_snippets::imports::Wallet;

mod common;
use common::{
    address_to_bech32, create_temp_pem_file, generate_random_private_key, get_simulator_chain_id,
    mpp_facilitator_available, start_mpp_facilitator, wait_for_simulator_ready,
};

/// Suite AA: MPP Facilitator session + subscription resource endpoints.
///
/// Covers gaps from `mpp-facilitator-mvx` unit tests (`session-resource`, `subscription-resource`)
/// at integration level against a live facilitator process.
#[tokio::test]
async fn test_mpp_facilitator_session_and_subscription_resources() {
    if !mpp_facilitator_available() {
        println!(
            "Skipping: mpp-facilitator-mvx not available (clone sibling repo, run ./setup.sh for mppx deps, npm install, npm run build)"
        );
        return;
    }

    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator().expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{port}");
    wait_for_simulator_ready(&gateway_url).await;
    let chain_id = get_simulator_chain_id(&gateway_url).await;

    let relayer_pk = generate_random_private_key();
    let recipient_addr = {
        let wallet = Wallet::from_private_key(&relayer_pk).expect("wallet");
        address_to_bech32(&wallet.to_address())
    };
    let relayer_pem = create_temp_pem_file("suite_aa_relayer", &relayer_pk, &recipient_addr);
    let mpp_url = start_mpp_facilitator(
        &mut pm,
        &gateway_url,
        &chain_id,
        &[
            ("RELAYER_PEM_PATH", relayer_pem.as_str()),
            ("MPP_SECRET_KEY", "integration-test-secret"),
            ("MPP_DEFAULT_AMOUNT", "1000000000000000000"),
            ("MPP_DEFAULT_RECIPIENT", recipient_addr.as_str()),
        ],
    )
    .await;

    let client = reqwest::Client::new();

    for (path, detail_hint) in [
        ("session-resource", "session payment"),
        ("subscription-resource", "subscription payment"),
    ] {
        let resp = client
            .get(format!("{mpp_url}/{path}"))
            .send()
            .await
            .expect("HTTP request failed");

        assert_eq!(resp.status(), 402, "{path} should return 402 without payment");

        let body: serde_json::Value = resp.json().await.expect("problem+json body");
        assert_eq!(body["status"], 402);
        assert!(
            body["detail"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .contains(detail_hint),
            "Unexpected detail for {path}: {:?}",
            body["detail"]
        );
        assert!(
            body.get("challenge").and_then(|v| v.as_str()).is_some()
                || body.get("challengeId").and_then(|v| v.as_str()).is_some(),
            "{path} should include challenge metadata"
        );
        println!("✅ GET /{path} → 402 with MPP challenge");
    }
}
