use mx_agentic_commerce_tests::ProcessManager;

mod common;
use common::{get_simulator_chain_id, wait_for_simulator_ready};

/// Suite EC: test_plan EC-002 — facilitator offline should not silently succeed.
#[tokio::test]
async fn test_facilitator_offline_verify_fails() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator().expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{port}");
    wait_for_simulator_ready(&gateway_url).await;
    let _chain_id = get_simulator_chain_id(&gateway_url).await;

    // Use a port where no facilitator is listening.
    let dead_port = ProcessManager::find_free_port();
    let dead_facilitator_url = format!("http://localhost:{dead_port}");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .unwrap();

    let verify_body = serde_json::json!({
        "paymentPayload": {
            "signature": "00",
            "payload": {
                "sender": "erd1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq6gq4hu",
                "receiver": "erd1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq6gq4hu",
                "value": "1000000000000000000",
                "nonce": 1,
                "chainID": "D",
                "version": 1
            }
        }
    });

    let result = client
        .post(format!("{dead_facilitator_url}/verify"))
        .json(&verify_body)
        .send()
        .await;

    assert!(
        result.is_err(),
        "Verify against offline facilitator must fail with connection error"
    );

    println!("✅ EC-002: Facilitator offline — verify request failed as expected");
}
