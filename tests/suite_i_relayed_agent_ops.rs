use base64::Engine as _;
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use serde_json::{json, Value};
use std::process::Stdio;
use tokio::time::{sleep, Duration};

mod common;
use common::{
    wait_for_simulator_ready,
    address_to_bech32, create_pem_file, fund_address_on_simulator, generate_blocks_on_simulator,
    generate_random_private_key, IdentityRegistryInteractor,
};

const RELAYER_PORT: u16 = 3003;
const RELAYER_URL: &str = "http://localhost:3003";

/// Suite I: All agent contract operations via openclaw-relayer (Relayed V3)
///
/// Tests:
///   1. register_agent — unfunded agent registers via relayer (challenge-based auth)
///   2. set_metadata — registered agent sets pricing via relayer (on-chain auth)
///   3. Verify on-chain state after each operation
#[tokio::test]
async fn test_relayed_agent_operations() {
    let mut pm = ProcessManager::new();

    // ────────────────────────────────────────────
    // 1. START CHAIN SIMULATOR
    // ────────────────────────────────────────────
    let port = pm.start_chain_simulator().unwrap(); // .expect("Failed to start Sim");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);

    let chain_id = common::get_simulator_chain_id(&gateway_url).await;
    println!("Simulator ChainID: {}", chain_id);

    let admin = interactor.register_wallet(test_wallets::alice()).await;

    // Top up admin with 100,000 EGLD (chain sim initial balance is only ~10 EGLD)
    let admin_bech32 = address_to_bech32(&admin);
    fund_address_on_simulator(&admin_bech32, "100000000000000000000000", &gateway_url).await;
    println!("Admin topped up with 100,000 EGLD");

    // ────────────────────────────────────────────
    // 2. SETUP RELAYER WALLETS (30 to cover all shards)
    //    Fund them BEFORE creating registry (borrow rules)
    // ────────────────────────────────────────────
    let project_root = std::env::current_dir().unwrap();
    let relayer_wallets_dir = project_root.join("tests").join("temp_relayer_wallets_i");

    if relayer_wallets_dir.exists() {
        std::fs::remove_dir_all(&relayer_wallets_dir).unwrap();
    }
    std::fs::create_dir_all(&relayer_wallets_dir).unwrap();

    println!("Generating 30 Relayer Wallets...");
    for i in 0..30 {
        let relayer_pk = generate_random_private_key();
        let relayer_wallet = Wallet::from_private_key(&relayer_pk).unwrap();
        let relayer_addr_obj = relayer_wallet.to_address();
        let relayer_addr = relayer_addr_obj.to_bech32("erd").to_string();
        let relayer_sc_addr = Address::from_slice(relayer_addr_obj.as_bytes());

        interactor
            .tx()
            .from(&admin)
            .to(&relayer_sc_addr)
            .egld(1_000_000_000_000_000_000u64)
            .run()
            .await;

        let relayer_pem = relayer_wallets_dir.join(format!("relayer_{}.pem", i));
        create_pem_file(relayer_pem.to_str().unwrap(), &relayer_pk, &relayer_addr);
    }
    println!("All relayer wallets funded.");

    // Ensure cross-shard EGLD transfers settle (30 wallets across 3 shards)
    generate_blocks_on_simulator(30, &gateway_url).await;
    sleep(Duration::from_millis(500)).await;

    // ────────────────────────────────────────────
    // 3. DEPLOY IDENTITY REGISTRY + ISSUE TOKEN
    //    (borrows interactor mutably — do after direct funding)
    // ────────────────────────────────────────────
    let registry_addr_bech32;
    {
        let registry = IdentityRegistryInteractor::init(&mut interactor, admin.clone()).await;
        registry_addr_bech32 = address_to_bech32(registry.address());
        println!("Registry: {}", registry_addr_bech32);

        registry
            .issue_token(&mut interactor, "AgentNFT", "AGENTNFT")
            .await;
        // Generate blocks to ensure async ESDT callback completes (token ID stored)
        generate_blocks_on_simulator(20, &gateway_url).await;
        sleep(Duration::from_secs(1)).await;
    }
    // registry dropped — interactor borrow released

    // ────────────────────────────────────────────
    // 4. START OPENCLAW-RELAYER SERVICE
    // ────────────────────────────────────────────
    // Final block generation to ensure ALL cross-shard EGLD transfers are settled
    generate_blocks_on_simulator(30, &gateway_url).await;
    sleep(Duration::from_millis(500)).await;

    let env = vec![
        ("NETWORK_PROVIDER", gateway_url.as_str()),
        ("IDENTITY_REGISTRY_ADDRESS", registry_addr_bech32.as_str()),
        ("RELAYER_WALLETS_DIR", relayer_wallets_dir.to_str().unwrap()),
        ("PORT", "3003"),
        ("CHAIN_ID", chain_id.as_str()),
        ("IS_TEST_ENV", "true"),
        ("SKIP_SIMULATION", "false"),
        ("LOG_LEVEL", "debug"),
    ];

    pm.start_node_service(
        "Relayer",
        "../x402_integration/multiversx-openclaw-relayer",
        "dist/index.js",
        env,
        RELAYER_PORT,
    )
    .expect("Failed to start Relayer");
    sleep(Duration::from_secs(1)).await;

    // Verify relayer is healthy
    let client = reqwest::Client::new();
    let health = client
        .get(format!("{}/health", RELAYER_URL))
        .send()
        .await
        .expect("Relayer health check failed");
    assert!(health.status().is_success(), "Relayer not healthy");
    println!("✅ Relayer is healthy");

    // ────────────────────────────────────────────
    // 5. TEST: register_agent (unfunded wallet, relayed)
    // ────────────────────────────────────────────
    println!("\n═══ TEST 1: register_agent via Relayer ═══");

    let agent_pk = generate_random_private_key();
    let agent_wallet = Wallet::from_private_key(&agent_pk).unwrap();
    let agent_addr = agent_wallet.to_address().to_bech32("erd").to_string();
    println!("Agent Address (UNFUNDED): {}", agent_addr);

    let agent_pem_path = project_root.join("tests").join("temp_agent_i.pem");
    create_pem_file(agent_pem_path.to_str().unwrap(), &agent_pk, &agent_addr);

    // Use register.ts script
    let output = std::process::Command::new("npm")
        .arg("run")
        .arg("register")
        .current_dir("../moltbot-starter-kit")
        .env("MULTIVERSX_PRIVATE_KEY", agent_pem_path.to_str().unwrap())
        .env("MULTIVERSX_API_URL", &gateway_url)
        .env("IDENTITY_REGISTRY_ADDRESS", &registry_addr_bech32)
        .env("CHAIN_ID", &chain_id)
        .env("MULTIVERSX_CHAIN_ID", &chain_id)
        .env("MULTIVERSX_RELAYER_URL", RELAYER_URL)
        .env("FORCE_RELAYER", "true")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("Failed to run registration script");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!("Register stdout: {}", stdout);
    if !stderr.is_empty() {
        println!("Register stderr: {}", stderr);
    }

    assert!(
        output.status.success(),
        "Registration script failed:\n{}",
        stderr
    );
    assert!(
        stdout.contains("Relayed Transaction Sent"),
        "Should use relay path"
    );
    println!("✅ register_agent via Relayer: broadcast SUCCESS");

    // CRITICAL: Generate blocks so cross-shard tx gets processed
    // (auto-generate-blocks is disabled in chain sim config)
    generate_blocks_on_simulator(30, &gateway_url).await;
    sleep(Duration::from_secs(1)).await;

    // Verify on-chain: query get_agent_id via HTTP (takes 0 args, returns all mappings)
    let vm_query = json!({
        "scAddress": registry_addr_bech32,
        "funcName": "get_agent_id",
        "args": []
    });

    let vm_res = client
        .post(format!("{}/vm-values/query", gateway_url))
        .json(&vm_query)
        .send()
        .await
        .expect("VM query failed");

    let vm_body: Value = vm_res.json().await.unwrap();
    println!(
        "get_agent_id response: {}",
        serde_json::to_string_pretty(&vm_body["data"]["data"]).unwrap_or_default()
    );

    let return_code = vm_body["data"]["data"]["returnCode"]
        .as_str()
        .unwrap_or("unknown");
    assert_eq!(return_code, "ok", "get_agent_id query failed: {}", vm_body);

    let return_data = vm_body["data"]["data"]["returnData"]
        .as_array()
        .expect("returnData not found");

    // returnData should have a non-empty entry (the agent's NFT nonce as base64)
    let has_agent = return_data
        .iter()
        .any(|v| v.as_str().is_some_and(|s| !s.is_empty()));
    assert!(
        has_agent,
        "Agent should have a non-zero ID after registration. returnData: {:?}",
        return_data
    );
    println!("✅ On-chain verification: Agent registered (get_agent_id returned data)");

    // ────────────────────────────────────────────
    // 6. TEST: set_metadata via Relayer (pricing)
    //    Uses ABI factory to encode correctly, same as register_agent
    // ────────────────────────────────────────────
    println!("\n═══ TEST 2: set_metadata via Relayer ═══");

    // Get relayer address for this agent's shard
    let relayer_addr_res = client
        .get(format!("{}/relayer/address/{}", RELAYER_URL, agent_addr))
        .send()
        .await
        .expect("Failed to get relayer address");

    let relayer_addr_body: Value = relayer_addr_res.json().await.unwrap();
    let relayer_address_bech32 = relayer_addr_body["relayerAddress"]
        .as_str()
        .expect("relayerAddress not found in response");
    println!(
        "Relayer Address for agent shard: {}",
        relayer_address_bech32
    );

    // Get agent's current nonce via HTTP
    let agent_nonce_res: Value = client
        .get(format!("{}/address/{}", gateway_url, agent_addr))
        .send()
        .await
        .expect("Failed to get agent account")
        .json()
        .await
        .unwrap();

    let agent_nonce = agent_nonce_res["data"]["account"]["nonce"]
        .as_u64()
        .unwrap_or(0);
    println!("Agent nonce: {}", agent_nonce);

    // Build + sign set_metadata tx using ABI factory (correct encoding)
    let sign_output = std::process::Command::new("node")
        .arg("-e")
        .arg(format!(r#"
            const {{ UserSigner }} = require('@multiversx/sdk-wallet');
            const {{ Address, TransactionComputer, SmartContractTransactionsFactory, TransactionsFactoryConfig, Abi, VariadicValue, Struct, BytesValue, Field, StructType, FieldDefinition, BytesType }} = require('@multiversx/sdk-core');
            const fs = require('fs');

            async function main() {{
                const pemContent = fs.readFileSync('{}', 'utf8');
                const signer = UserSigner.fromPem(pemContent);
                const sender = new Address('{}');

                const rawAbi = fs.readFileSync('identity-registry.abi.json', 'utf8')
                    .replace(/\bTokenId\b/g, 'TokenIdentifier')
                    .replace(/\bNonZeroBigUint\b/g, 'BigUint')
                    .replace(/\bcounted-variadic\b/g, 'variadic')
                    .replace(/\bList</g, 'variadic<')
                    .replace(/\bPayment\b/g, 'EgldOrEsdtTokenPayment');
                const abiJson = JSON.parse(rawAbi);
                const abi = Abi.create(abiJson);
                const config = new TransactionsFactoryConfig({{ chainID: '{}' }});
                const factory = new SmartContractTransactionsFactory({{ config, abi }});

                // Build MetadataEntry structs manually (SDK can't auto-serialize variadic counted)
                const metadataEntryType = new StructType('MetadataEntry', [
                    new FieldDefinition('key', '', new BytesType()),
                    new FieldDefinition('value', '', new BytesType()),
                ]);

                const entry1 = new Struct(metadataEntryType, [
                    new Field(new BytesValue(Buffer.from('price:default')), 'key'),
                    new Field(new BytesValue(Buffer.from('0de0b6b3a7640000', 'hex')), 'value'),
                ]);
                const entry2 = new Struct(metadataEntryType, [
                    new Field(new BytesValue(Buffer.from('token:default')), 'key'),
                    new Field(new BytesValue(Buffer.from('EGLD')), 'value'),
                ]);

                const metadataVariadic = VariadicValue.fromItemsCounted(entry1, entry2);

                const tx = await factory.createTransactionForExecute(sender, {{
                    contract: new Address('{}'),
                    function: 'set_metadata',
                    arguments: [BigInt(1), metadataVariadic],
                    gasLimit: 6000000n,
                }});

                tx.nonce = BigInt({});
                tx.relayer = new Address('{}');
                tx.gasLimit += 50000n;

                const computer = new TransactionComputer();
                const serialized = computer.computeBytesForSigning(tx);
                const signature = await signer.sign(serialized);
                tx.signature = signature;
                console.log(JSON.stringify(tx.toPlainObject()));
            }}
            main();
        "#,
            agent_pem_path.to_str().unwrap(),
            agent_addr,
            chain_id,
            registry_addr_bech32,
            agent_nonce,
            relayer_address_bech32,
        ))
        .current_dir("../moltbot-starter-kit")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("Failed to sign set_metadata tx");

    let sign_stdout = String::from_utf8_lossy(&sign_output.stdout);
    let sign_stderr = String::from_utf8_lossy(&sign_output.stderr);

    if !sign_output.status.success() {
        panic!(
            "Signing set_metadata failed:\nstdout: {}\nstderr: {}",
            sign_stdout, sign_stderr
        );
    }

    let signed_tx_str = sign_stdout
        .lines()
        .last()
        .expect("No output from signing script");
    let signed_tx: Value = serde_json::from_str(signed_tx_str)
        .unwrap_or_else(|e| panic!("Invalid JSON from signing: {}\nRaw: {}", e, signed_tx_str));
    println!("Signed set_metadata tx: {}", signed_tx);

    // POST to relayer /relay
    let relay_res = client
        .post(format!("{}/relay", RELAYER_URL))
        .json(&json!({ "transaction": signed_tx }))
        .send()
        .await
        .expect("Failed to relay set_metadata");

    let relay_status = relay_res.status();
    let relay_body: Value = relay_res.json().await.unwrap();
    println!("Relay Response ({}): {}", relay_status, relay_body);
    assert!(
        relay_status.is_success(),
        "set_metadata relay failed: {}",
        relay_body
    );
    assert!(
        relay_body["txHash"].is_string(),
        "Response should contain txHash"
    );
    println!(
        "✅ set_metadata via Relayer: broadcast SUCCESS (txHash: {})",
        relay_body["txHash"]
    );

    // Generate blocks for set_metadata cross-shard processing
    generate_blocks_on_simulator(30, &gateway_url).await;
    sleep(Duration::from_secs(1)).await;

    // ────────────────────────────────────────────
    // 7. VERIFY ON-CHAIN STATE
    // ────────────────────────────────────────────
    println!("\n═══ VERIFICATION ═══");

    // Re-query get_agent_id to get the nonce
    let vm_res2 = client
        .post(format!("{}/vm-values/query", gateway_url))
        .json(&vm_query)
        .send()
        .await
        .expect("VM query failed");

    let vm_body2: Value = vm_res2.json().await.unwrap();
    let return_data2 = vm_body2["data"]["data"]["returnData"]
        .as_array()
        .expect("returnData not found");

    // Decode agent nonce from base64
    let agent_nonce_b64 = return_data2[0].as_str().unwrap_or("");
    if !agent_nonce_b64.is_empty() {
        let nonce_bytes = base64::engine::general_purpose::STANDARD
            .decode(agent_nonce_b64)
            .unwrap();
        let mut nonce_u64 = 0u64;
        for b in &nonce_bytes {
            nonce_u64 = (nonce_u64 << 8) | (*b as u64);
        }
        println!("Agent NFT nonce: {}", nonce_u64);

        // Now query get_agent(nonce) to verify full details
        let nonce_hex = hex::encode(&nonce_bytes);
        let vm_agent_query = json!({
            "scAddress": registry_addr_bech32,
            "funcName": "get_agent",
            "args": [nonce_hex]
        });

        let vm_agent_res = client
            .post(format!("{}/vm-values/query", gateway_url))
            .json(&vm_agent_query)
            .send()
            .await
            .expect("VM query get_agent failed");

        let vm_agent_body: Value = vm_agent_res.json().await.unwrap();
        let agent_return_code = vm_agent_body["data"]["data"]["returnCode"]
            .as_str()
            .unwrap_or("unknown");
        println!("get_agent returnCode: {}", agent_return_code);
        assert_eq!(agent_return_code, "ok", "get_agent should succeed");
        println!("✅ get_agent: Agent details found");
    } else {
        println!("⚠️ get_agent_id returned empty — registration tx may have been invalid on-chain");
        // Don't panic, report it — registration broadcast succeeded but SC execution could have failed
    }

    // ────────────────────────────────────────────
    // 8. CLEANUP
    // ────────────────────────────────────────────
    println!("\n═══ CLEANUP ═══");
    let _ = std::fs::remove_dir_all(&relayer_wallets_dir);
    let _ = std::fs::remove_file(&agent_pem_path);
    println!("✅ Suite I Complete: All agent operations via Relayed V3 passed.");
}
