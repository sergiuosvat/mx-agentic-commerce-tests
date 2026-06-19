use crate::common::{
    create_pem_file, fund_address_on_simulator, generate_random_private_key,
    IdentityRegistryInteractor,
};
use multiversx_sc::types::ManagedBuffer;
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;
use tokio::time::{sleep, Duration};

#[tokio::test]
async fn test_error_paths() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    sleep(Duration::from_secs(3)).await;
    let gateway_url = format!("http://localhost:{}", port);

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);

    // Setup Alice (Admin/Owner)
    let alice_private_key = generate_random_private_key();
    let alice_wallet = Wallet::from_private_key(&alice_private_key).unwrap();
    let alice_address = alice_wallet.to_address();
    create_pem_file(
        "alice_errors.pem",
        &alice_private_key,
        &alice_address.to_bech32("erd").to_string(),
    );
    interactor.register_wallet(alice_wallet).await;
    fund_address_on_simulator(
		&alice_address.to_bech32("erd").to_string(),
		"100000000000000000000000",
		&gateway_url,
	)
    .await;

    // Setup Bob (Attacker)
    let bob_private_key = generate_random_private_key();
    let bob_wallet = Wallet::from_private_key(&bob_private_key).unwrap();
    let bob_address = bob_wallet.to_address();
    interactor.register_wallet(bob_wallet).await;
    fund_address_on_simulator(
		&bob_address.to_bech32("erd").to_string(),
		"100000000000000000000000",
		&gateway_url,
	)
    .await;

    // Deploy contract
    let identity_interactor =
        IdentityRegistryInteractor::init(&mut interactor, alice_address.clone()).await;
    let contract_address = identity_interactor.address().clone();

    // 1. Register Agent BEFORE Issue Token -> ERR_TOKEN_NOT_ISSUED
    println!("Test: Register before issue token...");
    interactor
        .tx()
        .from(&alice_address)
        .to(&contract_address)
        .raw_call("register_agent")
        .argument(&ManagedBuffer::<StaticApi>::new_from_bytes(b"EarlyBot"))
        .argument(&ManagedBuffer::<StaticApi>::new_from_bytes(b"uri"))
        .argument(&ManagedBuffer::<StaticApi>::new_from_bytes(&[0u8; 32]))
        .argument(&0u32) // metadata count
        .argument(&0u32) // services count
        .returns(ExpectError(4, "Token not issued"))
        .run()
        .await;
    // Can't easily check error message with current sc-snippets unless we parse the error string
    // But failure is good.

    // 2. Issue Token Twice -> ERR_TOKEN_ALREADY_ISSUED
    println!("Test: Issue token...");
    identity_interactor
        .issue_token(&mut interactor, "AgentToken", "AGENT")
        .await;

    println!("Test: Issue token twice...");
    // Attempt second issuance
    interactor
        .tx()
        .from(&alice_address)
        .to(&contract_address)
        .egld(50_000_000_000_000_000u64)
        .raw_call("issue_token")
        .argument(&ManagedBuffer::<StaticApi>::new_from_bytes(b"AgentToken2"))
        .argument(&ManagedBuffer::<StaticApi>::new_from_bytes(b"AGENT2"))
        .returns(ExpectError(4, "Token already issued"))
        .run()
        .await;

    // 3. Register Agent Twice -> ERR_AGENT_ALREADY_REGISTERED
    println!("Test: Register agent twice...");
    identity_interactor
        .register_agent(&mut interactor, "Bot1", "uri", vec![])
        .await; // Success

    interactor
        .tx()
        .from(&alice_address)
        .to(&contract_address)
        .raw_call("register_agent")
        .argument(&ManagedBuffer::<StaticApi>::new_from_bytes(b"Bot1Dup"))
        .argument(&ManagedBuffer::<StaticApi>::new_from_bytes(b"uri"))
        .argument(&ManagedBuffer::<StaticApi>::new_from_bytes(&[0u8; 32]))
        .argument(&0u32) // metadata count
        .argument(&0u32) // services count
        .returns(ExpectError(4, "Agent already registered for this address"))
        .run()
        .await;

    // 4. Update Agent by Non-Owner -> ERR_NOT_OWNER
    // Commented out due to complexity with OptionalValue and Payment simulation without actual NFT
    /*
    println!("Test: Update by non-owner...");
    let token_id: TokenIdentifier<StaticApi> = identity_interactor
        .interactor
        .query()
        .to(&contract_address)
        .typed(IdentityRegistryProxy)
        .agent_token_id()
        .returns(ReturnsResult)
        .run()
        .await;

    // We need a NEW interactor instance for Bob to avoid borrow checker hell
    // or just use raw tx from interactor but changing sender?
    // sc-snippets `interactor.tx().from(&bob)` works fine.

    let err_update = identity_interactor
        .interactor
        .tx()
        .from(&bob_wallet)
        .to(&contract_address)
        .typed(IdentityRegistryProxy)
        .update_agent(
            ManagedBuffer::new_from_bytes(b"HackedName"),
            ManagedBuffer::new_from_bytes(b"uri"),
            ManagedBuffer::new_from_bytes(&[0u8; 32]),
            OptionalValue::None,
            OptionalValue::None,
        )
        .payment((token_id, 1, BigUint::from(1u64))) // Fake payment of the NFT (which Bob doesn't have!)
        // Actually, update_agent expects the NFT payment.
        // If Bob doesn't have the NFT, it will fail with "Account has no balance" or "Invalid payment".
        // To test ERR_NOT_OWNER, Bob would need to own the NFT but NOT be the address registered in `agents()` mapper?
        // Wait, `agents()` mapper maps nonce -> owner_address.
        // And update_agent checks `call_value().single_esdt().token_identifier == token_id`.
        // And then checks `caller == self.agents().get(&nonce)`.
        // If Bob manages to steal the NFT, he becomes the owner of NFT.
        // But `agents()` storage still points to Alice.
        // So `require!(caller == owner, ERR_NOT_OWNER)` would fail.
        // BUT, Bob needs the NFT to call `update_agent` (it's payable with NFT).
        // Since Bob doesn't have the NFT, this call will fail at protocol level (insufficient funds/NFT).
        // So we can't easily test `ERR_NOT_OWNER` logic unless we transfer NFT to Bob first.
        // Let's test `ERR_NOT_OWNER` on `set_metadata` instead, which is NOT payable and uses `require_agent_owner`.
        .run()
        .await;

    assert!(err_update.is_err()); // Will fail due to missing NFT, but still an error path.
    */

    // 5. Set Metadata by Non-Owner
    println!("Test: Set metadata by non-owner...");
    // `set_metadata` checks `require_agent_owner`.
    // It is NOT payable.
    interactor
        .tx()
        .from(&bob_address)
        .to(&contract_address)
        .raw_call("set_metadata")
        .argument(&1u64) // Alice's nonce
        .argument(&0u32) // metadata count
        .returns(ExpectError(
            4,
            "Only the agent owner can perform this action",
        ))
        .run()
        .await;

    std::fs::remove_file("alice_errors.pem").unwrap_or(());
}
