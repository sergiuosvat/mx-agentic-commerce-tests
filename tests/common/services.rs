use mx_agentic_commerce_tests::ProcessManager;

use super::wait_for_http_ok;

pub const FACILITATOR_CWD: &str = "../x402_integration/x402_facilitator";
pub const FACILITATOR_SCRIPT: &str = "dist/index.js";
pub const RELAYER_CWD: &str = "../x402_integration/multiversx-openclaw-relayer";
pub const RELAYER_SCRIPT: &str = "dist/index.js";
pub const MPP_FACILITATOR_CWD: &str = "../mpp-facilitator-mvx";
pub const MPP_FACILITATOR_SCRIPT: &str = "dist/main.js";

/// Temp directory for relayer wallet PEM files (not in repo).
pub fn temp_relayer_wallets_dir(label: &str) -> String {
    std::env::temp_dir()
        .join(format!("mx-relayer-{label}-{}", std::process::id()))
        .to_string_lossy()
        .into_owned()
}

/// Start x402 facilitator on a free port and wait until `/health` responds.
pub async fn start_facilitator(
    pm: &mut ProcessManager,
    private_key: &str,
    registry_address: &str,
    gateway_url: &str,
    chain_id: &str,
    extra_env: &[(&str, &str)],
) -> String {
    start_facilitator_with_port(pm, private_key, registry_address, gateway_url, chain_id, extra_env)
        .await
        .1
}

/// Like [`start_facilitator`] but also returns the bound port (for diagnostics).
pub async fn start_facilitator_with_port(
    pm: &mut ProcessManager,
    private_key: &str,
    registry_address: &str,
    gateway_url: &str,
    chain_id: &str,
    extra_env: &[(&str, &str)],
) -> (u16, String) {
    let port = ProcessManager::find_free_port();
    let port_str = port.to_string();

    let mut env: Vec<(String, String)> = vec![
        ("PORT".into(), port_str.clone()),
        ("PRIVATE_KEY".into(), private_key.to_string()),
        ("REGISTRY_ADDRESS".into(), registry_address.to_string()),
        ("IDENTITY_REGISTRY_ADDRESS".into(), registry_address.to_string()),
        ("NETWORK_PROVIDER".into(), gateway_url.to_string()),
        ("GATEWAY_URL".into(), gateway_url.to_string()),
        ("MULTIVERSX_API_URL".into(), gateway_url.to_string()),
        ("MX_PROXY_URL".into(), gateway_url.to_string()),
        ("CHAIN_ID".into(), chain_id.to_string()),
    ];

    for (key, value) in extra_env {
        env.push(((*key).to_string(), (*value).to_string()));
    }

    pm.start_node_service_owned("Facilitator", FACILITATOR_CWD, FACILITATOR_SCRIPT, env, port)
        .expect("Failed to start facilitator");

    let base_url = format!("http://localhost:{port}");
    wait_for_http_ok(&format!("{base_url}/health"), 30).await;
    (port, base_url)
}

/// Start openclaw relayer on a free port and wait until `/health` responds.
pub async fn start_relayer(
    pm: &mut ProcessManager,
    gateway_url: &str,
    registry_address: &str,
    relayer_wallets_dir: &str,
    chain_id: &str,
    extra_env: &[(&str, &str)],
) -> String {
    let port = ProcessManager::find_free_port();
    let port_str = port.to_string();

    let mut env: Vec<(String, String)> = vec![
        ("PORT".into(), port_str.clone()),
        ("NETWORK_PROVIDER".into(), gateway_url.to_string()),
        ("IDENTITY_REGISTRY_ADDRESS".into(), registry_address.to_string()),
        ("RELAYER_WALLETS_DIR".into(), relayer_wallets_dir.to_string()),
        ("CHAIN_ID".into(), chain_id.to_string()),
        ("IS_TEST_ENV".into(), "true".to_string()),
        ("SKIP_SIMULATION".into(), "false".to_string()),
    ];

    for (key, value) in extra_env {
        env.push(((*key).to_string(), (*value).to_string()));
    }

    pm.start_node_service_owned("Relayer", RELAYER_CWD, RELAYER_SCRIPT, env, port)
        .expect("Failed to start relayer");

    let base_url = format!("http://localhost:{port}");
    wait_for_http_ok(&format!("{base_url}/health"), 30).await;
    base_url
}

/// Start mpp-facilitator-mvx on a free port and wait until `/health` responds.
pub async fn start_mpp_facilitator(
    pm: &mut ProcessManager,
    gateway_url: &str,
    chain_id: &str,
    extra_env: &[(&str, &str)],
) -> String {
    let port = ProcessManager::find_free_port();

    let mut env: Vec<(String, String)> = vec![
        ("NETWORK_PROVIDER".into(), gateway_url.to_string()),
        ("GATEWAY_URL".into(), gateway_url.to_string()),
        ("CHAIN_ID".into(), chain_id.to_string()),
    ];

    for (key, value) in extra_env {
        env.push(((*key).to_string(), (*value).to_string()));
    }

    pm.start_node_service_owned(
        "MppFacilitator",
        MPP_FACILITATOR_CWD,
        MPP_FACILITATOR_SCRIPT,
        env,
        port,
    )
    .expect("Failed to start MPP facilitator");

    let base_url = format!("http://localhost:{port}");
    wait_for_http_ok(&format!("{base_url}/health"), 30).await;
    base_url
}
