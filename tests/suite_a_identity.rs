use identity_registry_interactor::identity_registry_proxy::IdentityRegistryProxy;
use multiversx_sc::types::ManagedAddress;
use multiversx_sc_scenario::imports::InterpreterContext;
use multiversx_sc_snippets::imports::StaticApi;
use multiversx_sc_snippets::imports::*;
use mx_agentic_commerce_tests::ProcessManager;

#[path = "common/mod.rs"]
mod test_utils;
use test_utils::wait_for_simulator_ready;


#[tokio::test]
async fn test_identity_registry_flow() {
    let mut pm = ProcessManager::new();
    let port = pm.start_chain_simulator()
        .expect("Failed to start simulator");
    let gateway_url = format!("http://localhost:{}", port);
    wait_for_simulator_ready(&gateway_url).await;

    let mut interactor = Interactor::new(&gateway_url).await.use_chain_simulator(true);

    let wallet_alice = interactor.register_wallet(test_wallets::alice()).await;

    interactor.generate_blocks_until_all_activations().await;

    println!("Identity Registry Flow Test Started");

    // Deploy using mxsc.json pattern from working interactor
    let mut mxsc_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    mxsc_path.push("artifacts");
    mxsc_path.push("identity-registry.mxsc.json");

    // Check if file exists to avoid confusing error
    if !mxsc_path.exists() {
        panic!("MXSC JSON not found at: {:?}", mxsc_path);
    }

    println!("Loading MXSC from: {:?}", mxsc_path);

    let contract_code = BytesValue::interpret_from(
        format!("mxsc:{}", mxsc_path.to_str().unwrap()),
        &InterpreterContext::default(),
    );

    let new_address = interactor
        .tx()
        .from(&wallet_alice)
        .gas(100_000_000)
        .typed(IdentityRegistryProxy)
        .init()
        .code(&contract_code)
        .returns(ReturnsNewAddress)
        .run()
        .await;

    println!("Deployed at: {}", new_address.to_bech32_default());

    // Issue Token
    interactor
        .tx()
        .from(&wallet_alice)
        .to(&new_address)
        .gas(60_000_000)
        .typed(IdentityRegistryProxy)
        .issue_token(
            ManagedBuffer::new_from_bytes(b"AgentToken"),
            ManagedBuffer::new_from_bytes(b"AGENT"),
        )
        .egld(50_000_000_000_000_000u64)
        .run()
        .await;

    println!("Issued Token");

    // Prepare Metadata using raw_call pattern (MultiValueEncodedCounted requires explicit count + nested encoding)
    let name_buf = ManagedBuffer::<StaticApi>::new_from_bytes(b"MyAgent");
    let uri_buf = ManagedBuffer::<StaticApi>::new_from_bytes(b"https://example.com/agent.json");
    let pk_buf = ManagedBuffer::<StaticApi>::new_from_bytes(&[0u8; 32]);

    // metadata count = 2
    let metadata_count: u32 = 2;
    let metadata_count_buf =
        ManagedBuffer::<StaticApi>::new_from_bytes(&metadata_count.to_be_bytes());

    // Entry 1: price:default = 1 EGLD
    let price: u64 = 1_000_000_000_000_000_000;
    let mut entry1_bytes = Vec::new();
    let key1 = b"price:default";
    entry1_bytes.extend_from_slice(&(key1.len() as u32).to_be_bytes());
    entry1_bytes.extend_from_slice(key1);
    let val1 = price.to_be_bytes();
    entry1_bytes.extend_from_slice(&(val1.len() as u32).to_be_bytes());
    entry1_bytes.extend_from_slice(&val1);
    let entry1_buf = ManagedBuffer::<StaticApi>::new_from_bytes(&entry1_bytes);

    // Entry 2: token:default = EGLD
    let mut entry2_bytes = Vec::new();
    let key2 = b"token:default";
    entry2_bytes.extend_from_slice(&(key2.len() as u32).to_be_bytes());
    entry2_bytes.extend_from_slice(key2);
    let val2 = b"EGLD";
    entry2_bytes.extend_from_slice(&(val2.len() as u32).to_be_bytes());
    entry2_bytes.extend_from_slice(val2);
    let entry2_buf = ManagedBuffer::<StaticApi>::new_from_bytes(&entry2_bytes);

    // services count = 0
    let services_count: u32 = 0;
    let services_count_buf =
        ManagedBuffer::<StaticApi>::new_from_bytes(&services_count.to_be_bytes());

    // Register Agent via raw_call
    interactor
        .tx()
        .from(&wallet_alice)
        .to(&new_address)
        .gas(600_000_000)
        .raw_call("register_agent")
        .argument(&name_buf)
        .argument(&uri_buf)
        .argument(&pk_buf)
        .argument(&metadata_count_buf)
        .argument(&entry1_buf)
        .argument(&entry2_buf)
        .argument(&services_count_buf)
        .run()
        .await;

    println!("Registered Agent");

    let owner_managed: ManagedAddress<StaticApi> = interactor
        .query()
        .to(&new_address)
        .typed(IdentityRegistryProxy)
        .get_agent_owner(1u64)
        .returns(ReturnsResult)
        .run()
        .await;

    assert_eq!(
        owner_managed.to_address(),
        wallet_alice,
        "registered agent owner should be alice"
    );
}
