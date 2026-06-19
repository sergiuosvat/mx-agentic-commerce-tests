use crate::common::{
    address_to_bech32, fund_address_on_simulator, generate_random_private_key,
    get_simulator_chain_id,
};
use base64::Engine;
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use tokio::time::{sleep, Duration};

#[tokio::test]
async fn test_transfer_tools() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    sleep(Duration::from_secs(2)).await;

    let chain_id = get_simulator_chain_id(&gateway_url).await;
    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);

    // CRITICAL: Activate all protocol features (including ESDT System SC)
    // Without this, ESDT issuance fails with "ESDT SC disabled"
    interactor.generate_blocks_until_all_activations().await;

    // 1. Setup Sender (Alice)
    let alice_pk_hex = generate_random_private_key();
    let alice_wallet = Wallet::from_private_key(&alice_pk_hex).unwrap();
    let alice_addr = alice_wallet.to_address();
    let alice_bech32 = address_to_bech32(&alice_addr);

    // Fund Alice
    fund_address_on_simulator(&alice_bech32, "200000000000000000000", &gateway_url).await; // 200 EGLD

    // 2. Create PEM file manually (Hex -> Base64 for SDK)
    let temp_pem_path = std::env::current_dir()
        .unwrap()
        .join("tests/pkg_5_mcp/alice_transfer.pem");
    if let Some(parent) = temp_pem_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }

    let alice_pk_bytes = hex::decode(&alice_pk_hex).unwrap();
    let alice_pub_bytes = alice_addr.as_bytes();

    // Construct 64-byte key (Secret + Public)
    let mut full_key = Vec::new();
    full_key.extend_from_slice(&alice_pk_bytes);
    full_key.extend_from_slice(alice_pub_bytes);

    // SDK expects Base64(Hex(KeyBytes)) based on pem.js inspection
    let hex_key = hex::encode(&full_key);
    let base64_sk = base64::engine::general_purpose::STANDARD.encode(hex_key);

    let pem_content = format!(
        "-----BEGIN PRIVATE KEY for {}-----\n{}\n-----END PRIVATE KEY for {}-----",
        alice_bech32, base64_sk, alice_bech32
    );
    std::fs::write(&temp_pem_path, pem_content).unwrap();

    // 3. Start MCP with Wallet
    let mut client =
        crate::mcp_client::McpClient::new(&chain_id, Some(temp_pem_path.to_str().unwrap()), &gateway_url).await;

    // 4. Test send-egld
    println!("Testing send-egld...");
    let bob_pk = generate_random_private_key();
    let bob_wallet = Wallet::from_private_key(&bob_pk).unwrap();
    let bob_bech32 = address_to_bech32(&bob_wallet.to_address());

    let args = serde_json::json!({
        "receiver": bob_bech32,
        "amount": "1000000000000000000" // 1 EGLD
    });

    let resp = client.call_tool("send-egld", args).await;

    if let Some(err) = resp.get("error") {
        panic!("MCP Error: {:?} Content: {:?}", err, resp.get("result"));
    }

    let result = &resp["result"];
    let content = result["content"].as_array().unwrap();
    if let Some(text_block) = content.iter().find(|c| c["type"] == "text") {
        let text = text_block["text"].as_str().unwrap();
        println!("Send EGLD Output: {}", text);
        assert!(
            text.contains("Transaction sent"),
            "Output should confirm transaction sent"
        );
    } else {
        panic!("No text content in response");
    }

    // Verify Bob received funds
    let client_http = reqwest::Client::new();
    let mut balance_found = false;
    for _ in 0..10 {
        let bob_acc_resp = client_http
            .get(format!("{}/address/{}", gateway_url, bob_bech32))
            .send()
            .await;
        if let Ok(r) = bob_acc_resp {
            if let Ok(json) = r.json::<serde_json::Value>().await {
                if let Some(bal) = json["data"]["account"]["balance"].as_str() {
                    if bal == "1000000000000000000" {
                        balance_found = true;
                        break;
                    }
                }
            }
        }
        let _ = interactor.generate_blocks(1).await;
        sleep(Duration::from_millis(500)).await;
    }
    assert!(balance_found, "Bob should have 1 EGLD");

    // 5. Issue Fungible Token via MCP Tool
    println!("Testing issue-fungible-token...");
    let random_suffix = rand::random::<u32>() % 10000;
    let ticker = format!("TEST{}", random_suffix);
    let name = format!("TestToken{}", random_suffix);

    let issue_args = serde_json::json!({
        "tokenName": name,
        "tokenTicker": ticker,
        "initialSupply": "1000000",
        "numDecimals": 6
    });

    let issue_resp = client.call_tool("issue-fungible-token", issue_args).await;
    if let Some(err) = issue_resp.get("error") {
        panic!("MCP Error: {:?}", err);
    }

    let issue_text = issue_resp["result"]["content"][0]["text"].as_str().unwrap();
    println!("Issue Token Output: {}", issue_text);
    assert!(issue_text.contains("Token issuance transaction sent"));

    // Extract tx hash from output
    let parts: Vec<&str> = issue_text.split("transactions/").collect();
    let hash_part = parts
        .get(1)
        .unwrap_or(&"")
        .split_whitespace()
        .next()
        .unwrap_or("");
    let tx_hash = hash_part.trim_matches(|c: char| !c.is_alphanumeric());
    println!("Extracted Tx Hash: '{}'", tx_hash);

    // Generate blocks & poll for token
    let mut token_id = String::new();
    let alice_esdt_url = format!("{}/address/{}/esdt", gateway_url, alice_bech32);

    for i in 0..20 {
        let _ = interactor.generate_blocks(1).await;
        sleep(Duration::from_millis(500)).await;

        let resp_esdt = client_http.get(&alice_esdt_url).send().await;
        if let Ok(r) = resp_esdt {
            if let Ok(json) = r.json::<serde_json::Value>().await {
                if let Some(esdts) = json["data"]["esdts"].as_object() {
                    println!("Poll {} Found ESDTs: {:?}", i, esdts.keys());
                    if let Some((id, _)) = esdts.iter().find(|(k, _)| k.starts_with(&ticker)) {
                        token_id = id.clone();
                        break;
                    }
                }
            }
        }
    }

    if token_id.is_empty() {
        // Fetch tx result for debugging
        let tx_url = format!("{}/transaction/{}?withResults=true", gateway_url, tx_hash);
        let tx_resp = client_http.get(&tx_url).send().await;
        if let Ok(r) = tx_resp {
            if let Ok(json) = r.json::<serde_json::Value>().await {
                println!("DEBUG Tx Result: {:#}", json);
            }
        }
        panic!(
            "Failed to find issued token {} for address {}",
            ticker, alice_bech32
        );
    }
    println!("Found Token ID: {}", token_id);

    // 6. Test send-tokens (ESDT)
    println!("Testing send-tokens...");
    let args_token = serde_json::json!({
        "receiver": bob_bech32,
        "tokenIdentifier": token_id,
        "amount": "100"
    });

    let resp_token = client.call_tool("send-tokens", args_token).await;
    if let Some(err) = resp_token.get("error") {
        panic!("MCP Error: {:?}", err);
    }

    let text_token_res = resp_token["result"]["content"][0]["text"].as_str().unwrap();
    println!("Send Token Output: {}", text_token_res);
    assert!(text_token_res.contains("Transaction sent"));

    // Verify Bob token balance
    let bob_esdt_url = format!("{}/address/{}/esdt/{}", gateway_url, bob_bech32, token_id);

    let mut token_found = false;
    for _ in 0..15 {
        let resp_esdt = client_http.get(&bob_esdt_url).send().await.unwrap();
        if resp_esdt.status().is_success() {
            let json: serde_json::Value = resp_esdt.json().await.unwrap();
            if let Some(token_data) = json.get("data").and_then(|d| d.get("tokenData")) {
                let balance = token_data["balance"].as_str().unwrap_or("0");
                if balance == "100" {
                    token_found = true;
                    break;
                }
            }
        }
        let _ = interactor.generate_blocks(1).await;
        sleep(Duration::from_millis(500)).await;
    }
    assert!(token_found, "Bob should have 100 tokens");

    // ===================================================================
    // 7. Test track-transaction (use the send-egld tx hash)
    // ===================================================================
    println!("Testing track-transaction...");
    // Re-send a small amount to get a fresh tx hash we can track
    let track_args = serde_json::json!({
        "receiver": bob_bech32,
        "amount": "100000000000000000" // 0.1 EGLD
    });
    let track_resp = client.call_tool("send-egld", track_args).await;
    let track_text = track_resp["result"]["content"][0]["text"].as_str().unwrap();
    println!("Track: send-egld output: {}", track_text);

    // Extract tx hash
    let track_hash = track_text
        .split("transactions/")
        .nth(1)
        .unwrap_or("")
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| !c.is_alphanumeric());
    println!("Track: extracted hash: '{}'", track_hash);
    assert!(!track_hash.is_empty(), "Should extract a valid tx hash");

    // Wait for it to finalize
    for _ in 0..5 {
        let _ = interactor.generate_blocks(1).await;
        sleep(Duration::from_millis(300)).await;
    }

    // Now call track-transaction
    let track_tx_args = serde_json::json!({ "txHash": track_hash });
    let track_tx_resp = client.call_tool("track-transaction", track_tx_args).await;
    if let Some(err) = track_tx_resp.get("error") {
        panic!("track-transaction error: {:?}", err);
    }
    let track_tx_text = track_tx_resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    println!("Track Transaction Output: {}", track_tx_text);
    assert!(
        track_tx_text.contains("success") || track_tx_text.contains("pending"),
        "Track should show success or pending"
    );

    // ===================================================================
    // 8. Test issue-nft-collection
    // ===================================================================
    println!("Testing issue-nft-collection...");
    let nft_suffix = rand::random::<u32>() % 10000;
    let nft_ticker = format!("NFTT{}", nft_suffix);
    let nft_name = format!("TestNFTs{}", nft_suffix);

    let issue_nft_args = serde_json::json!({
        "tokenName": nft_name,
        "tokenTicker": nft_ticker
    });

    let issue_nft_resp = client
        .call_tool("issue-nft-collection", issue_nft_args)
        .await;
    if let Some(err) = issue_nft_resp.get("error") {
        panic!("issue-nft-collection error: {:?}", err);
    }
    let issue_nft_text = issue_nft_resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    println!("Issue NFT Collection Output: {}", issue_nft_text);
    assert!(
        issue_nft_text.contains("issuance sent"),
        "Should confirm NFT collection issuance"
    );

    // Wait for NFT collection to appear
    let mut nft_collection_id = String::new();
    for i in 0..20 {
        let _ = interactor.generate_blocks(1).await;
        sleep(Duration::from_millis(500)).await;

        // Check Alice's tokens for the new collection
        let resp_roles = client_http
            .get(format!(
                "{}/address/{}/registered-nfts", gateway_url, alice_bech32
            ))
            .send()
            .await;
        if let Ok(r) = resp_roles {
            if let Ok(json) = r.json::<serde_json::Value>().await {
                println!("Poll {} registered-nfts: {:?}", i, json);
                if let Some(tokens) = json["data"]["tokens"].as_array() {
                    if let Some(t) = tokens.iter().find(|t| {
                        t.as_str()
                            .map(|s| s.starts_with(&nft_ticker))
                            .unwrap_or(false)
                    }) {
                        nft_collection_id = t.as_str().unwrap().to_string();
                        break;
                    }
                }
            }
        }
    }

    if nft_collection_id.is_empty() {
        // Fallback: Check ESDTs endpoint
        let resp_esdt = client_http
            .get(format!("{}/address/{}/esdt", gateway_url, alice_bech32))
            .send()
            .await;
        if let Ok(r) = resp_esdt {
            if let Ok(json) = r.json::<serde_json::Value>().await {
                if let Some(esdts) = json["data"]["esdts"].as_object() {
                    for (k, _) in esdts {
                        if k.starts_with(&nft_ticker) {
                            // Extract collection ID (before the nonce part)
                            let parts: Vec<&str> = k.split('-').collect();
                            if parts.len() >= 2 {
                                nft_collection_id = format!("{}-{}", parts[0], parts[1]);
                            }
                            break;
                        }
                    }
                }
            }
        }
    }

    if nft_collection_id.is_empty() {
        println!("WARN: Could not find NFT collection, trying to use ticker directly");
        // On simulator, we can still try to set roles and create NFT
        // The collection might not show up in registered-nfts API on simulator
        // Try to find it from the issuance tx
        let tx_hash_nft = issue_nft_text
            .split("transactions/")
            .nth(1)
            .unwrap_or("")
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_matches(|c: char| !c.is_alphanumeric());

        if !tx_hash_nft.is_empty() {
            let tx_url = format!(
                "{}/transaction/{}?withResults=true", gateway_url, tx_hash_nft
            );
            if let Ok(r) = client_http.get(&tx_url).send().await {
                if let Ok(json) = r.json::<serde_json::Value>().await {
                    // Look in SCRs for the token identifier
                    if let Some(scrs) =
                        json["data"]["transaction"]["smartContractResults"].as_array()
                    {
                        for scr in scrs {
                            if let Some(data) = scr["data"].as_str() {
                                // Data format: @ok@<hex_token_id>
                                if data.starts_with("@") && data.contains("@") {
                                    let parts: Vec<&str> = data.split('@').collect();
                                    for part in &parts {
                                        if !part.is_empty() && *part != "ok" && part.len() > 4 {
                                            if let Ok(decoded) = hex::decode(part) {
                                                if let Ok(s) = String::from_utf8(decoded) {
                                                    if s.contains('-')
                                                        && s.to_uppercase()
                                                            .starts_with(&nft_ticker.to_uppercase())
                                                    {
                                                        nft_collection_id = s;
                                                        println!(
                                                            "Found collection from SCR: {}",
                                                            nft_collection_id
                                                        );
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    println!("NFT Collection ID: '{}'", nft_collection_id);
    assert!(
        !nft_collection_id.is_empty(),
        "Should have found NFT collection ID"
    );

    // ===================================================================
    // 9. Set ESDTRoleNFTCreate via interactor (not an MCP tool)
    // ===================================================================
    println!("Setting ESDTRoleNFTCreate for Alice...");

    // Register Alice wallet with interactor for direct tx
    let alice_interactor_addr = interactor.register_wallet(alice_wallet).await;
    let system_sc = Bech32Address::from_bech32_string(
        "erd1qqqqqqqqqqqqqqqpqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqzllls8a5w6u".to_string(),
    );

    // setSpecialRole@<token_hex>@<address_hex>@<role_hex>
    let token_hex = hex::encode(nft_collection_id.as_bytes());
    let addr_hex = hex::encode(alice_interactor_addr.as_bytes());
    let role_hex = hex::encode("ESDTRoleNFTCreate".as_bytes());

    let data = format!("setSpecialRole@{}@{}@{}", token_hex, addr_hex, role_hex);

    interactor
        .tx()
        .from(&alice_interactor_addr)
        .to(system_sc.to_address())
        .gas(60_000_000u64)
        .raw_call(data)
        .run()
        .await;

    // Generate blocks to process
    for _ in 0..5 {
        let _ = interactor.generate_blocks(1).await;
        sleep(Duration::from_millis(300)).await;
    }
    println!("ESDTRoleNFTCreate set");

    // ===================================================================
    // 10. Test create-nft
    // ===================================================================
    println!("Testing create-nft...");
    let create_nft_args = serde_json::json!({
        "collectionIdentifier": nft_collection_id,
        "name": "TestNFT #1",
        "royalties": 500,  // 5%
        "quantity": "1",
        "uris": ["https://example.com/nft1.json"]
    });

    let create_nft_resp = client.call_tool("create-nft", create_nft_args).await;
    if let Some(err) = create_nft_resp.get("error") {
        panic!("create-nft error: {:?}", err);
    }
    let create_nft_text = create_nft_resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    println!("Create NFT Output: {}", create_nft_text);
    assert!(
        create_nft_text.contains("creation transaction sent"),
        "Should confirm NFT creation"
    );

    // Verify NFT appears on Alice's account
    let mut nft_found = false;
    for _ in 0..15 {
        let _ = interactor.generate_blocks(1).await;
        sleep(Duration::from_millis(500)).await;

        let resp_esdt = client_http
            .get(format!("{}/address/{}/esdt", gateway_url, alice_bech32))
            .send()
            .await;
        if let Ok(r) = resp_esdt {
            if let Ok(json) = r.json::<serde_json::Value>().await {
                if let Some(esdts) = json["data"]["esdts"].as_object() {
                    // NFTs appear as COLLECTION-HEXID-NONCE
                    if esdts.keys().any(|k| k.starts_with(&nft_collection_id)) {
                        nft_found = true;
                        println!("NFT found in Alice's account!");
                        break;
                    }
                }
            }
        }
    }
    assert!(nft_found, "Alice should have the minted NFT");

    println!("=== All MCP transfer & token lifecycle tools PASSED ===");

    // Cleanup
    let _ = std::fs::remove_file(temp_pem_path);
}
