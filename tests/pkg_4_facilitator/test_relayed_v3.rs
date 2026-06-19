use crate::common::{
    wait_for_simulator_ready,
    create_pem_file, fund_address_on_simulator, generate_blocks_on_simulator,
    generate_random_private_key, get_simulator_chain_id,
};
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use std::process::Command;
use tokio::time::{sleep, Duration};

const FACILITATOR_PORT: u16 = 3066;

struct FacilitatorGuard {
    child: std::process::Child,
}

impl Drop for FacilitatorGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Also kill any orphaned node processes on the port
        let _ = Command::new("sh")
            .arg("-c")
            .arg(format!(
                "lsof -ti :{} 2>/dev/null | xargs kill -9 2>/dev/null",
                FACILITATOR_PORT
            ))
            .status();
    }
}

fn kill_port(port: u16) {
    let _ = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "lsof -ti :{} 2>/dev/null | xargs kill -9 2>/dev/null",
            port
        ))
        .status();
    std::thread::sleep(std::time::Duration::from_millis(500));
}

#[tokio::test]
async fn test_relayed_v3_flow() {
    // Pre-cleanup: kill any stale facilitator on our port
    kill_port(FACILITATOR_PORT);

    let mut pm = ProcessManager::new();
    let sim_port = pm.start_chain_simulator().unwrap();
    let gateway_url = format!("http://localhost:{}", sim_port);
    wait_for_simulator_ready(&gateway_url).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);

    // 1. Setup Actors
    let sender_pk = generate_random_private_key();
    let sender_wallet = Wallet::from_private_key(&sender_pk).unwrap();
    let sender_address = sender_wallet.to_address().to_bech32("erd").to_string();
    let _ = interactor.register_wallet(sender_wallet).await;

    let receiver_pk = generate_random_private_key();
    let receiver_wallet = Wallet::from_private_key(&receiver_pk).unwrap();
    let receiver_address = receiver_wallet.to_address().to_bech32("erd").to_string();

    // 2. Fund Sender
    println!("Funding Sender: {}", sender_address);
    fund_address_on_simulator(&sender_address, "500000000000000000000", &gateway_url).await; // 500 EGLD

    // 3. Generate multiple relayer wallets (covering all 3 shards)
    let project_root = std::env::current_dir().unwrap();
    let relayer_wallets_dir = project_root.join("test_relayers_v3");
    let _ = std::fs::remove_dir_all(&relayer_wallets_dir);
    std::fs::create_dir_all(&relayer_wallets_dir).expect("Failed to create relayer wallets dir");
    let relayer_wallets_dir_str = relayer_wallets_dir.to_str().unwrap().to_string();

    println!(
        "Generating relayer wallets for all shards in {}...",
        relayer_wallets_dir_str
    );
    for i in 0..30 {
        let rk = generate_random_private_key();
        let rw = Wallet::from_private_key(&rk).unwrap();
        let ra = rw.to_address().to_bech32("erd").to_string();

        // Fund each relayer
        fund_address_on_simulator(&ra, "1000000000000000000", &gateway_url).await; // 1 EGLD

        let pem_path = format!("{}/relayer_{}.pem", relayer_wallets_dir_str, i);
        create_pem_file(&pem_path, &rk, &ra);
        println!("Generated Relayer {}: {}", i, ra);
    }

    // Ensure cross-shard funding is finalized
    generate_blocks_on_simulator(5, &gateway_url).await;

    // 4. Start Facilitator with RELAYER_WALLETS_DIR
    let db_path = "./facilitator_relayed.db";
    let _ = std::fs::remove_file(db_path);

    let chain_id = get_simulator_chain_id(&gateway_url).await;

    let facilitator_dir = std::path::Path::new("../x402_integration/x402_facilitator");
    let child = Command::new("npx")
        .arg("tsx")
        .arg("src/index.ts")
        .current_dir(facilitator_dir)
        .env("PORT", FACILITATOR_PORT.to_string())
        .env("PRIVATE_KEY", generate_random_private_key())
        .env(
            "REGISTRY_ADDRESS",
            "erd1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq6gq4hu",
        )
        .env("NETWORK_PROVIDER", &gateway_url)
        .env("GATEWAY_URL", &gateway_url)
        .env("CHAIN_ID", &chain_id)
        .env("SQLITE_DB_PATH", db_path)
        .env("SKIP_SIMULATION", "false")
        .env("RELAYER_WALLETS_DIR", &relayer_wallets_dir_str)
        .spawn()
        .expect("Failed to start facilitator");

    let facilitator_guard = FacilitatorGuard { child };

    // Wait for facilitator to be ready
    let client = reqwest::Client::new();
    let facilitator_url = format!("http://localhost:{}", FACILITATOR_PORT);
    for _ in 0..20 {
        if client
            .get(format!("{}/health", facilitator_url))
            .send()
            .await
            .is_ok()
        {
            break;
        }
        sleep(Duration::from_millis(500)).await;
    }

    // 5. Wait for Epoch 1 (Relayed V3 enabled at epoch 1)
    // RoundsPerEpoch=20, so 25 blocks is enough
    println!("Waiting for Epoch 1 (generating 25 blocks)...");
    generate_blocks_on_simulator(25, &gateway_url).await;
    sleep(Duration::from_secs(3)).await;

    // 6. Query facilitator for the correct shard-matched relayer address
    let relayer_resp = client
        .get(format!(
            "{}/relayer/address/{}",
            facilitator_url, sender_address
        ))
        .send()
        .await
        .expect("Failed to get relayer address");

    assert!(
        relayer_resp.status().is_success(),
        "Failed to get relayer address: {}",
        relayer_resp.text().await.unwrap_or_default()
    );

    let relayer_json: serde_json::Value = relayer_resp.json().await.unwrap();
    let relayer_address = relayer_json["relayerAddress"]
        .as_str()
        .expect("relayerAddress missing");
    println!(
        "Shard-matched relayer for sender {}: {}",
        sender_address, relayer_address
    );

    // 7. Sign inner transaction with the correct relayer address
    let payment_value = "100000000000000000"; // 0.1 EGLD

    let output = Command::new("npx")
        .arg("ts-node")
        .arg("../moltbot-starter-kit/scripts/sign_tx.ts")
        .arg("--sender-pk")
        .arg(&sender_pk)
        .arg("--receiver")
        .arg(&receiver_address)
        .arg("--value")
        .arg(payment_value)
        .arg("--nonce")
        .arg("0")
        .arg("--gas-limit")
        .arg("500000")
        .arg("--gas-price")
        .arg("1000000000")
        .arg("--chain-id")
        .arg(&chain_id)
        .arg("--relayer")
        .arg(relayer_address) // Shard-matched relayer
        .arg("--version")
        .arg("2")
        .output()
        .expect("Failed to sign transaction");

    if !output.status.success() {
        eprintln!("Sign Tx Error: {}", String::from_utf8_lossy(&output.stderr));
        panic!("Sign Tx failed");
    }

    let json_str = String::from_utf8(output.stdout).unwrap();
    let signed_tx: serde_json::Value = serde_json::from_str(json_str.trim()).unwrap();
    println!("Signed Tx Payload: {}", signed_tx);

    // 8. Construct verify/settle request body
    let request_body = serde_json::json!({
        "scheme": "exact",
        "payload": signed_tx,
        "requirements": {
            "payTo": receiver_address,
            "amount": payment_value,
            "asset": "EGLD",
            "network": chain_id
        }
    });

    // 9. Call /verify
    let verify_resp = client
        .post(format!("{}/verify", facilitator_url))
        .json(&request_body)
        .send()
        .await
        .expect("Failed to call verify");

    if !verify_resp.status().is_success() {
        let body = verify_resp.text().await.unwrap_or_default();
        panic!("Facilitator verify failed: {}", body);
    }
    let verify_json: serde_json::Value = verify_resp.json().await.unwrap();
    assert_eq!(verify_json["isValid"], true);

    // 10. Call /settle
    let resp = client
        .post(format!("{}/settle", facilitator_url))
        .json(&request_body)
        .send()
        .await
        .expect("Failed to call settle");

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        panic!("Facilitator settle failed: {}", body);
    }

    let resp_json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(resp_json["success"], true);

    // 11. Verify on-chain
    wait_for_simulator_ready(&gateway_url).await;
    generate_blocks_on_simulator(5, &gateway_url).await;
    sleep(Duration::from_secs(5)).await;

    // Check receiver balance
    let account_url = format!("{}/address/{}", gateway_url, receiver_address);
    let balance_resp = client
        .get(&account_url)
        .send()
        .await
        .expect("Failed to get balance");
    let balance_json: serde_json::Value = balance_resp.json().await.unwrap();
    let balance = balance_json["data"]["account"]["balance"].as_str().unwrap();
    assert_eq!(balance, payment_value, "Receiver balance incorrect");

    // Sender should not have paid gas (relayer did)
    let sender_final_balance_resp = client
        .get(format!("{}/address/{}", gateway_url, sender_address))
        .send()
        .await
        .unwrap();
    let sender_final_json: serde_json::Value = sender_final_balance_resp.json().await.unwrap();
    let sender_balance_str = sender_final_json["data"]["account"]["balance"]
        .as_str()
        .unwrap();
    let sender_balance_big: u128 = sender_balance_str.parse().unwrap();

    let initial_balance: u128 = 500_000_000_000_000_000_000;
    let transfer_value: u128 = 100_000_000_000_000_000;

    assert_eq!(
        sender_balance_big,
        initial_balance - transfer_value,
        "Sender should not pay gas"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&relayer_wallets_dir);
    drop(facilitator_guard);
}
