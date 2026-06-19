use multiversx_sc_snippets::imports::*;

/// One simulated day per block (`--round-duration` value in milliseconds).
pub const ONE_DAY_ROUND_DURATION_MS: u64 = 86_400_000;

/// `clean_old_jobs` threshold in the validation registry (3 days, in milliseconds).
pub const THREE_DAYS_MS: u64 = 3 * ONE_DAY_ROUND_DURATION_MS;

/// Poll the chain simulator until it responds to network config requests.
pub async fn wait_for_simulator_ready(gateway_url: &str) {
    let client = reqwest::Client::new();
    let url = format!("{}/network/config", gateway_url);

    for attempt in 0..60 {
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => return,
            _ => {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
        }
        if attempt == 59 {
            panic!("Chain simulator not ready at {gateway_url} after 30s");
        }
    }
}

/// Poll an HTTP endpoint until it returns 2xx or timeout.
pub async fn wait_for_http_ok(url: &str, timeout_secs: u64) {
    let client = reqwest::Client::new();
    let deadline =
        tokio::time::Instant::now() + tokio::time::Duration::from_secs(timeout_secs);

    while tokio::time::Instant::now() < deadline {
        if let Ok(resp) = client.get(url).send().await {
            if resp.status().is_success() {
                return;
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }

    panic!("HTTP endpoint not ready: {url} (timeout {timeout_secs}s)");
}

pub async fn get_simulator_chain_id(gateway_url: &str) -> String {
    let client = reqwest::Client::new();
    let resp: serde_json::Value = client
        .get(format!("{}/network/config", gateway_url))
        .send()
        .await
        .expect("Failed to get network config")
        .json()
        .await
        .expect("Failed to parse network config");

    resp["data"]["config"]["erd_chain_id"]
        .as_str()
        .expect("Chain ID not found")
        .to_string()
}

/// Fund an address on the chain simulator using /simulator/set-state.
pub async fn fund_address_on_simulator(address_bech32: &str, balance_wei: &str, gateway_url: &str) {
    let client = reqwest::Client::new();
    let body = serde_json::json!([{
        "address": address_bech32,
        "balance": balance_wei,
        "nonce": 0
    }]);

    for _ in 0..5 {
        let res = client
            .post(format!("{}/simulator/set-state", gateway_url))
            .json(&body)
            .send()
            .await;

        match res {
            Ok(resp) if resp.status().is_success() => return,
            Ok(resp) => {
                println!(
                    "fund_address failed with status {}, retrying...",
                    resp.status()
                );
            }
            Err(e) => {
                println!("fund_address request failed: {e}, retrying...");
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    }
    panic!("Failed to set state on simulator after retries");
}

/// Generate blocks on the chain simulator (needed when broadcasting via HTTP services).
pub async fn generate_blocks_on_simulator(num_blocks: u32, gateway_url: &str) {
    let client = reqwest::Client::new();
    let res = client
        .post(format!(
            "{}/simulator/generate-blocks/{}",
            gateway_url, num_blocks
        ))
        .send()
        .await
        .expect("Failed to generate blocks on simulator");
    assert!(res.status().is_success(), "generate-blocks failed");
}

/// Advance simulated chain time by `days` when the simulator was started with
/// `--round-duration ONE_DAY_ROUND_DURATION_MS` (one day per generated block).
pub async fn advance_simulator_days(days: u32, gateway_url: &str) {
    generate_blocks_on_simulator(days, gateway_url).await;
}

/// Read `erd_block_timestamp` (seconds) from shard 0 network status.
pub async fn get_simulator_block_timestamp_secs(gateway_url: &str) -> u64 {
    let client = reqwest::Client::new();
    let resp: serde_json::Value = client
        .get(format!("{}/network/status/0", gateway_url))
        .send()
        .await
        .expect("Failed to get network status")
        .json()
        .await
        .expect("Failed to parse network status");

    resp["data"]["status"]["erd_block_timestamp"]
        .as_u64()
        .expect("erd_block_timestamp not found")
}
