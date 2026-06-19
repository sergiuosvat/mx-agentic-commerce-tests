use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use tokio::time::{sleep, Duration};

mod common;
use common::{
    address_to_bech32, generate_blocks_on_simulator, IdentityRegistryInteractor,
    ServiceConfigInput,
};

/// Suite P: Identity Registry Extended Tests
///
/// Tests the following uncovered flows:
/// 1. set_service_configs — standalone service config setup
/// 2. remove_metadata — metadata removal
/// 3. remove_service_configs — service config removal
/// 4. Views: get_agent_service_config, get_agent_owner
///
/// Starts after epoch 1: generate 25 blocks after simulator start.
#[tokio::test]
async fn test_identity_extended_operations() {
    let mut pm = ProcessManager::new();

    // ── 1. Start Chain Simulator ──
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    sleep(Duration::from_secs(2)).await;

    // Generate 25 blocks to pass epoch 1
    generate_blocks_on_simulator(25, &gateway_url).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);
    let wallet_alice = interactor.register_wallet(test_wallets::alice()).await;

    // ── 2. Deploy Identity Registry ──
    let identity = IdentityRegistryInteractor::init(&mut interactor, wallet_alice.clone()).await;
    identity
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;
    let identity_bech32 = address_to_bech32(identity.address());
    println!("Identity Registry: {}", identity_bech32);

    // ── 3. Register an agent with metadata and service configs ──
    let services = vec![ServiceConfigInput::<StaticApi> {
        service_id: 1,
        price: BigUint::from(1_000_000_000_000_000_000u64), // 1 EGLD
        token: EgldOrEsdtTokenIdentifier::egld(),
        nonce: 0,
    }];

    identity
        .register_agent_with_services(
            &mut interactor,
            "TestAgent",
            "https://test.example.com/manifest.json",
            vec![("type", b"worker".to_vec()), ("version", b"1.0".to_vec())],
            services,
        )
        .await;
    println!("Agent registered with services: TestAgent (nonce=1)");

    // ── 4. Query agent owner view ──
    let client = reqwest::Client::new();
    let nonce_hex = hex::encode(1u64.to_be_bytes());

    let body_owner = serde_json::json!({
        "scAddress": identity_bech32,
        "funcName": "get_agent_owner",
        "args": [nonce_hex],
    });
    let resp_owner: serde_json::Value = client
        .post(format!("{}/vm-values/query", gateway_url))
        .json(&body_owner)
        .send()
        .await
        .expect("VM query failed")
        .json()
        .await
        .expect("VM query parse failed");
    let return_data_owner = resp_owner["data"]["data"]["returnData"]
        .as_array()
        .expect("No returnData");
    assert!(
        !return_data_owner.is_empty(),
        "Agent owner query should return data"
    );
    println!("✅ get_agent_owner verified");

    // ── 5. Query agent service config view ──
    let service_id_hex = hex::encode(1u32.to_be_bytes());
    let body_svc = serde_json::json!({
        "scAddress": identity_bech32,
        "funcName": "get_agent_service_config",
        "args": [nonce_hex, service_id_hex],
    });

    let resp_svc: serde_json::Value = client
        .post(format!("{}/vm-values/query", gateway_url))
        .json(&body_svc)
        .send()
        .await
        .expect("VM query failed")
        .json()
        .await
        .expect("VM query parse failed");

    let return_data_svc = resp_svc["data"]["data"]["returnData"]
        .as_array()
        .expect("No returnData");
    assert!(
        !return_data_svc.is_empty(),
        "Service config should be set for service_id=1"
    );
    println!("✅ get_agent_service_config returned data for service_id=1");

    // ── 6. Set additional service configs via set_service_configs ──
    let new_services = vec![
        ServiceConfigInput::<StaticApi> {
            service_id: 2,
            price: BigUint::from(500_000_000_000_000_000u64), // 0.5 EGLD
            token: EgldOrEsdtTokenIdentifier::egld(),
            nonce: 0,
        },
        ServiceConfigInput::<StaticApi> {
            service_id: 3,
            price: BigUint::from(2_000_000_000_000_000_000u64), // 2 EGLD
            token: EgldOrEsdtTokenIdentifier::egld(),
            nonce: 0,
        },
    ];

    identity
        .set_service_configs(&mut interactor, new_services, "AGENT", 1)
        .await;
    println!("✅ set_service_configs executed for service_id=2,3");

    // Verify service_id=2 is now queryable
    let service_id_2_hex = hex::encode(2u32.to_be_bytes());
    let body_svc2 = serde_json::json!({
        "scAddress": identity_bech32,
        "funcName": "get_agent_service_config",
        "args": [nonce_hex, service_id_2_hex],
    });
    let resp_svc2: serde_json::Value = client
        .post(format!("{}/vm-values/query", gateway_url))
        .json(&body_svc2)
        .send()
        .await
        .expect("VM query failed")
        .json()
        .await
        .expect("VM query parse failed");
    let return_data_svc2 = resp_svc2["data"]["data"]["returnData"]
        .as_array()
        .expect("No returnData");
    assert!(
        !return_data_svc2.is_empty(),
        "Service config should exist for service_id=2"
    );
    println!("✅ get_agent_service_config verified for service_id=2");

    // ── 7. Remove metadata ──
    identity
        .remove_metadata(&mut interactor, vec!["version"], "AGENT", 1)
        .await;
    println!("✅ remove_metadata executed for key='version'");

    // Verify metadata was removed
    let version_key_hex = hex::encode("version".as_bytes());
    let body_meta = serde_json::json!({
        "scAddress": identity_bech32,
        "funcName": "get_metadata",
        "args": [nonce_hex, version_key_hex],
    });
    let resp_meta: serde_json::Value = client
        .post(format!("{}/vm-values/query", gateway_url))
        .json(&body_meta)
        .send()
        .await
        .expect("VM query failed")
        .json()
        .await
        .expect("VM query parse failed");
    let return_data_meta = resp_meta["data"]["data"]["returnData"]
        .as_array()
        .expect("No returnData");
    let is_removed = return_data_meta.is_empty()
        || return_data_meta.iter().all(|v| {
            let s = v.as_str().unwrap_or("");
            s.is_empty()
        });
    assert!(is_removed, "Metadata 'version' should be removed");
    println!("✅ Verified metadata 'version' was removed");

    // Verify metadata 'type' still exists
    let type_key_hex = hex::encode("type".as_bytes());
    let body_type = serde_json::json!({
        "scAddress": identity_bech32,
        "funcName": "get_metadata",
        "args": [nonce_hex, type_key_hex],
    });
    let resp_type: serde_json::Value = client
        .post(format!("{}/vm-values/query", gateway_url))
        .json(&body_type)
        .send()
        .await
        .expect("VM query failed")
        .json()
        .await
        .expect("VM query parse failed");
    let return_data_type = resp_type["data"]["data"]["returnData"]
        .as_array()
        .expect("No returnData");
    assert!(
        !return_data_type.is_empty(),
        "Metadata 'type' should still exist"
    );
    println!("✅ Verified metadata 'type' still exists after selective removal");

    // ── 8. Remove service configs ──
    identity
        .remove_service_configs(&mut interactor, vec![1], "AGENT", 1)
        .await;
    println!("✅ remove_service_configs executed for service_id=1");

    // Verify service_id=1 is removed
    let body_svc_removed = serde_json::json!({
        "scAddress": identity_bech32,
        "funcName": "get_agent_service_config",
        "args": [nonce_hex, service_id_hex],
    });
    let resp_svc_removed: serde_json::Value = client
        .post(format!("{}/vm-values/query", gateway_url))
        .json(&body_svc_removed)
        .send()
        .await
        .expect("VM query failed")
        .json()
        .await
        .expect("VM query parse failed");
    let return_data_svc_removed = resp_svc_removed["data"]["data"]["returnData"]
        .as_array()
        .expect("No returnData");
    let is_removed_svc = return_data_svc_removed.is_empty()
        || return_data_svc_removed.iter().all(|v| {
            let s = v.as_str().unwrap_or("");
            s.is_empty()
        });
    assert!(
        is_removed_svc,
        "Service config for service_id=1 should be removed"
    );
    println!("✅ Verified service config for service_id=1 was removed");

    // Verify service_id=2 still exists after removing only service_id=1
    let body_svc2_check = serde_json::json!({
        "scAddress": identity_bech32,
        "funcName": "get_agent_service_config",
        "args": [nonce_hex, service_id_2_hex],
    });
    let resp_svc2_check: serde_json::Value = client
        .post(format!("{}/vm-values/query", gateway_url))
        .json(&body_svc2_check)
        .send()
        .await
        .expect("VM query failed")
        .json()
        .await
        .expect("VM query parse failed");
    let return_data_svc2_check = resp_svc2_check["data"]["data"]["returnData"]
        .as_array()
        .expect("No returnData");
    assert!(
        !return_data_svc2_check.is_empty(),
        "Service config for service_id=2 should still exist"
    );
    println!("✅ Verified service config for service_id=2 still exists after selective removal");

    println!("\n🎉 Suite P: Identity Extended Operations — PASSED ✅");
    println!("  Tested: set_service_configs, remove_metadata, remove_service_configs");
    println!("  Tested: get_agent_owner, get_agent_service_config views");
}
