#![allow(dead_code, unused_imports)]

use bech32::{self, Bech32, Hrp};
use multiversx_sc::derive_imports::*;
use multiversx_sc::proxy_imports::*;
use multiversx_sc::types::{Address, CodeMetadata, EgldOrEsdtTokenIdentifier, ManagedBuffer};
use multiversx_sc_snippets::imports::*;

pub mod escrow_interactor;
pub mod mpp_session_helpers;
pub mod mpp_session_mvx_proxy;
pub mod services;
pub mod simulator;
pub mod test_env;
pub use escrow_interactor::*;
pub use services::*;
pub use simulator::*;
pub use test_env::TestEnv;

pub const WASM_PATH: &str = "artifacts/identity-registry.wasm";
pub const VALIDATION_WASM_PATH: &str = "artifacts/validation-registry.wasm";
pub const REPUTATION_WASM_PATH: &str = "artifacts/reputation-registry.wasm";

use rand::RngCore;

pub fn generate_random_private_key() -> String {
    let mut rng = rand::thread_rng();
    let mut key = [0u8; 32];
    rng.fill_bytes(&mut key);
    hex::encode(key)
}

pub fn address_to_bech32(address: &Address) -> String {
    let hrp = Hrp::parse("erd").expect("Invalid HRP");
    bech32::encode::<Bech32>(hrp, address.as_bytes()).expect("Failed to encode address")
}

use base64::{engine::general_purpose, Engine as _};

pub async fn vm_query<T: TopDecode>(
    interactor: &mut Interactor,
    contract: &Address,
    func: &str,
    args: Vec<ManagedBuffer<StaticApi>>,
) -> T {
    let mut query = interactor.query().to(contract).raw_call(func);

    for arg in args {
        query = query.argument(&arg);
    }

    query.original_result().returns(ReturnsResult).run().await
}

/// Write a PEM file under the system temp directory (avoids polluting the repo root).
pub fn create_temp_pem_file(label: &str, private_key_hex: &str, address_bech32: &str) -> String {
    let path = std::env::temp_dir().join(format!(
        "mx-agentic-{label}-{}-{address_bech32}.pem",
        std::process::id()
    ));
    let path_str = path.to_string_lossy().into_owned();
    create_pem_file(&path_str, private_key_hex);
    path_str
}

pub fn create_pem_file(file_path: &str, private_key_hex: &str) {
    let priv_bytes = hex::decode(private_key_hex).expect("Invalid hex");
    let wallet = Wallet::from_private_key(private_key_hex).expect("Wallet failed");
    let address = wallet.to_address(); // multiversx_chain_core::types::Address
    let pub_bytes = address.as_bytes();

    // We need bech32 for formatting the PEM header/footer correctly if we were using it for calling, but here we just write it.
    let address_bech32 = address.to_bech32("erd").to_string();

    let mut combined = Vec::new(); // 32 priv + 32 pub
    combined.extend_from_slice(&priv_bytes);
    combined.extend_from_slice(pub_bytes);

    let hex_combined = hex::encode(&combined);
    let b64 = general_purpose::STANDARD.encode(hex_combined);

    // Split into lines of 64 chars for standard PEM format
    let chunks: Vec<String> = b64
        .chars()
        .collect::<Vec<char>>()
        .chunks(64)
        .map(|c| c.iter().collect())
        .collect();
    let b64_formatted = chunks.join("\n");

    let pem_content = format!(
        "-----BEGIN PRIVATE KEY for {}-----\n{}\n-----END PRIVATE KEY for {}-----",
        address_bech32, b64_formatted, address_bech32
    );

    std::fs::write(file_path, pem_content).expect("Failed to write PEM");
}

#[type_abi]
#[derive(
    TopEncode, TopDecode, ManagedVecItem, NestedEncode, NestedDecode, Clone, PartialEq, Debug,
)]
pub struct MetadataEntry<M: ManagedTypeApi> {
    pub key: ManagedBuffer<M>,
    pub value: ManagedBuffer<M>,
}

#[type_abi]
#[derive(
    TopEncode, TopDecode, ManagedVecItem, NestedEncode, NestedDecode, Clone, PartialEq, Debug,
)]
pub struct ServiceConfigInput<M: ManagedTypeApi> {
    pub service_id: u32,
    pub price: BigUint<M>,
    pub token: EgldOrEsdtTokenIdentifier<M>,
    pub nonce: u64,
}

pub struct IdentityRegistryInteractor {
    pub wallet_address: Address,
    pub contract_address: Address,
}

impl IdentityRegistryInteractor {
    pub async fn init(interactor: &mut Interactor, wallet_address: Address) -> Self {
        println!("Reading WASM from: {}", WASM_PATH);
        let wasm_bytes = std::fs::read(WASM_PATH).expect("Failed to read WASM file");
        println!("Read WASM size: {}", wasm_bytes.len());

        let code_buf = ManagedBuffer::new_from_bytes(&wasm_bytes);

        interactor.generate_blocks_until_all_activations().await;

        let contract_address = interactor
            .tx()
            .from(&wallet_address)
            .gas(600_000_000)
            .raw_deploy()
            .code(code_buf)
            .code_metadata(
                CodeMetadata::UPGRADEABLE
                    | CodeMetadata::READABLE
                    | CodeMetadata::PAYABLE
                    | CodeMetadata::PAYABLE_BY_SC,
            )
            .returns(ReturnsNewAddress)
            .run()
            .await;

        println!("Deployed Identity Registry at: {}", contract_address);

        Self {
            wallet_address,
            contract_address,
        }
    }

    pub async fn issue_token(&self, interactor: &mut Interactor, name: &str, ticker: &str) {
        let name_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(name.as_bytes());
        let ticker_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(ticker.as_bytes());

        interactor
            .tx()
            .from(&self.wallet_address)
            .to(&self.contract_address)
            .gas(600_000_000)
            .egld(50_000_000_000_000_000u64)
            .raw_call("issue_token")
            .argument(&name_buf)
            .argument(&ticker_buf)
            .run()
            .await;

        interactor.generate_blocks(3).await.ok();
        println!("Issued Token: {}", ticker);
    }

    pub async fn register_agent(
        &self,
        interactor: &mut Interactor,
        name: &str,
        uri: &str,
        metadata: Vec<(&str, Vec<u8>)>,
    ) {
        let name_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(name.as_bytes());
        let uri_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(uri.as_bytes());
        let pk_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(&[0u8; 32]);

        let metadata_count = metadata.len() as u32;
        let metadata_count_buf: ManagedBuffer<StaticApi> =
            ManagedBuffer::new_from_bytes(&metadata_count.to_be_bytes());

        let mut request = interactor
            .tx()
            .from(&self.wallet_address)
            .to(&self.contract_address)
            .gas(600_000_000)
            .raw_call("register_agent")
            .argument(&name_buf)
            .argument(&uri_buf)
            .argument(&pk_buf)
            .argument(&metadata_count_buf);

        if !metadata.is_empty() {
            for (key, value) in metadata {
                let mut encoded_bytes = Vec::new();
                let key_len = (key.len() as u32).to_be_bytes();
                encoded_bytes.extend_from_slice(&key_len);
                encoded_bytes.extend_from_slice(key.as_bytes());

                let val_len = (value.len() as u32).to_be_bytes();
                encoded_bytes.extend_from_slice(&val_len);
                encoded_bytes.extend_from_slice(&value);

                let encoded_buf: ManagedBuffer<StaticApi> =
                    ManagedBuffer::new_from_bytes(&encoded_bytes);
                request = request.argument(&encoded_buf);
            }
        }

        let services_count: u32 = 0;
        let services_count_buf: ManagedBuffer<StaticApi> =
            ManagedBuffer::new_from_bytes(&services_count.to_be_bytes());
        request = request.argument(&services_count_buf);

        request.run().await;
        println!("Registered Agent: {}", name);
    }

    pub async fn register_agent_with_services(
        &self,
        interactor: &mut Interactor,
        name: &str,
        uri: &str,
        metadata: Vec<(&str, Vec<u8>)>,
        services: Vec<ServiceConfigInput<StaticApi>>,
    ) {
        let name_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(name.as_bytes());
        let uri_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(uri.as_bytes());
        let pk_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(&[0u8; 32]);

        let metadata_count = metadata.len() as u32;
        let metadata_count_buf: ManagedBuffer<StaticApi> =
            ManagedBuffer::new_from_bytes(&metadata_count.to_be_bytes());

        let mut request = interactor
            .tx()
            .from(&self.wallet_address)
            .to(&self.contract_address)
            .gas(600_000_000)
            .raw_call("register_agent")
            .argument(&name_buf)
            .argument(&uri_buf)
            .argument(&pk_buf)
            .argument(&metadata_count_buf);

        if !metadata.is_empty() {
            for (key, value) in metadata {
                let mut encoded_bytes = Vec::new();
                let key_len = (key.len() as u32).to_be_bytes();
                encoded_bytes.extend_from_slice(&key_len);
                encoded_bytes.extend_from_slice(key.as_bytes());

                let val_len = (value.len() as u32).to_be_bytes();
                encoded_bytes.extend_from_slice(&val_len);
                encoded_bytes.extend_from_slice(&value);

                let encoded_buf: ManagedBuffer<StaticApi> =
                    ManagedBuffer::new_from_bytes(&encoded_bytes);
                request = request.argument(&encoded_buf);
            }
        }

        let services_count = services.len() as u32;
        let services_count_buf: ManagedBuffer<StaticApi> =
            ManagedBuffer::new_from_bytes(&services_count.to_be_bytes());
        request = request.argument(&services_count_buf);

        if !services.is_empty() {
            for service in services {
                let mut encoded_bytes = Vec::new();
                service.dep_encode(&mut encoded_bytes).unwrap();

                let encoded_buf: ManagedBuffer<StaticApi> =
                    ManagedBuffer::new_from_bytes(&encoded_bytes);
                request = request.argument(&encoded_buf);
            }
        }

        request.run().await;
    }

    pub async fn update_agent(
        &self,
        interactor: &mut Interactor,
        name: &str,
        uri: &str,
        metadata: Vec<(&str, Vec<u8>)>,
        services: Vec<ServiceConfigInput<StaticApi>>,
        agent_token: (&str, u64),
    ) {
        let (token_identifier, nonce) = agent_token;
        let name_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(name.as_bytes());
        let uri_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(uri.as_bytes());
        let pk_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(&[0u8; 32]);

        let metadata_count = metadata.len() as u32;
        let metadata_count_buf: ManagedBuffer<StaticApi> =
            ManagedBuffer::new_from_bytes(&metadata_count.to_be_bytes());

        let payment_token: TokenIdentifier<StaticApi> = token_identifier.into();
        let payment_amount: BigUint<StaticApi> = 1u64.into();

        let mut request = interactor
            .tx()
            .from(&self.wallet_address)
            .to(&self.contract_address)
            .gas(600_000_000)
            .single_esdt(&payment_token, nonce, &payment_amount)
            .raw_call("update_agent")
            .argument(&name_buf)
            .argument(&uri_buf)
            .argument(&pk_buf)
            .argument(&metadata_count_buf);

        if !metadata.is_empty() {
            for (key, value) in metadata {
                let mut encoded_bytes = Vec::new();
                let key_len = (key.len() as u32).to_be_bytes();
                encoded_bytes.extend_from_slice(&key_len);
                encoded_bytes.extend_from_slice(key.as_bytes());

                let val_len = (value.len() as u32).to_be_bytes();
                encoded_bytes.extend_from_slice(&val_len);
                encoded_bytes.extend_from_slice(&value);

                let encoded_buf: ManagedBuffer<StaticApi> =
                    ManagedBuffer::new_from_bytes(&encoded_bytes);
                request = request.argument(&encoded_buf);
            }
        }

        let services_count = services.len() as u32;
        let services_count_buf: ManagedBuffer<StaticApi> =
            ManagedBuffer::new_from_bytes(&services_count.to_be_bytes());
        request = request.argument(&services_count_buf);

        if !services.is_empty() {
            for service in services {
                let mut encoded_bytes = Vec::new();
                service.dep_encode(&mut encoded_bytes).unwrap();

                let encoded_buf: ManagedBuffer<StaticApi> =
                    ManagedBuffer::new_from_bytes(&encoded_bytes);
                request = request.argument(&encoded_buf);
            }
        }

        request.run().await;
        println!("Updated Agent: {}", name);
    }

    pub async fn set_metadata(
        &self,
        interactor: &mut Interactor,
        metadata: Vec<(&str, Vec<u8>)>,
        nonce: u64,
    ) {
        let metadata_count = metadata.len() as u32;
        let metadata_count_buf: ManagedBuffer<StaticApi> =
            ManagedBuffer::new_from_bytes(&metadata_count.to_be_bytes());

        let mut request = interactor
            .tx()
            .from(&self.wallet_address)
            .to(&self.contract_address)
            .gas(600_000_000)
            .raw_call("set_metadata")
            .argument(&nonce)
            .argument(&metadata_count_buf);

        if !metadata.is_empty() {
            for (key, value) in metadata {
                let mut encoded_bytes = Vec::new();
                let key_len = (key.len() as u32).to_be_bytes();
                encoded_bytes.extend_from_slice(&key_len);
                encoded_bytes.extend_from_slice(key.as_bytes());

                let val_len = (value.len() as u32).to_be_bytes();
                encoded_bytes.extend_from_slice(&val_len);
                encoded_bytes.extend_from_slice(&value);

                let encoded_buf: ManagedBuffer<StaticApi> =
                    ManagedBuffer::new_from_bytes(&encoded_bytes);
                request = request.argument(&encoded_buf);
            }
        }

        request.run().await;
    }

    pub async fn remove_metadata(
        &self,
        interactor: &mut Interactor,
        keys: Vec<&str>,
        nonce: u64,
    ) {
        let mut request = interactor
            .tx()
            .from(&self.wallet_address)
            .to(&self.contract_address)
            .gas(600_000_000)
            .raw_call("remove_metadata")
            .argument(&nonce);

        for key in keys {
            let key_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(key.as_bytes());
            request = request.argument(&key_buf);
        }

        request.run().await;
    }

    pub async fn set_service_configs(
        &self,
        interactor: &mut Interactor,
        services: Vec<ServiceConfigInput<StaticApi>>,
        nonce: u64,
    ) {
        let mut request = interactor
            .tx()
            .from(&self.wallet_address)
            .to(&self.contract_address)
            .gas(600_000_000)
            .raw_call("set_service_configs")
            .argument(&nonce);

        let services_count = services.len() as u32;
        let services_count_buf: ManagedBuffer<StaticApi> =
            ManagedBuffer::new_from_bytes(&services_count.to_be_bytes());
        request = request.argument(&services_count_buf);

        if !services.is_empty() {
            for service in services {
                let mut encoded_bytes = Vec::new();
                service.dep_encode(&mut encoded_bytes).unwrap();

                let encoded_buf: ManagedBuffer<StaticApi> =
                    ManagedBuffer::new_from_bytes(&encoded_bytes);
                request = request.argument(&encoded_buf);
            }
        }

        request.run().await;
    }

    pub async fn remove_service_configs(
        &self,
        interactor: &mut Interactor,
        service_ids: Vec<u32>,
        nonce: u64,
    ) {
        let mut request = interactor
            .tx()
            .from(&self.wallet_address)
            .to(&self.contract_address)
            .gas(600_000_000)
            .raw_call("remove_service_configs")
            .argument(&nonce);

        for id in service_ids {
            let id_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(&id.to_be_bytes());
            request = request.argument(&id_buf);
        }

        request.run().await;
    }

    pub fn address(&self) -> &Address {
        &self.contract_address
    }
}

#[type_abi]
#[derive(
    TopEncode, TopDecode, ManagedVecItem, NestedEncode, NestedDecode, Clone, PartialEq, Debug,
)]
pub struct JobData<M: ManagedTypeApi> {
    pub job_id: ManagedBuffer<M>,
    pub status: u8, // 0=New, 1=Pending, 2=Verified
    pub employer: ManagedAddress<M>,
    pub agent_nonce: u64,
    pub service_id: u32,
    pub payment_token: TokenIdentifier<M>,
    pub payment_amount: BigUint<M>,
    pub proof_hash: ManagedBuffer<M>,
    pub timestamp: u64,
}

pub struct ValidationRegistryInteractor {
    pub wallet_address: Address,
    pub contract_address: Address,
}

impl ValidationRegistryInteractor {
    pub async fn init(
        interactor: &mut Interactor,
        wallet_address: Address,
        identity_registry_address: &Address,
    ) -> Self {
        println!("Reading Validation WASM from: {}", VALIDATION_WASM_PATH);
        let wasm_bytes =
            std::fs::read(VALIDATION_WASM_PATH).expect("Failed to read validation WASM");
        let code_buf = ManagedBuffer::new_from_bytes(&wasm_bytes);

        interactor.generate_blocks_until_all_activations().await;

        let identity_addr_managed: ManagedAddress<StaticApi> =
            ManagedAddress::from_address(identity_registry_address);

        let contract_address = interactor
            .tx()
            .from(&wallet_address)
            .gas(600_000_000)
            .raw_deploy()
            .code(code_buf)
            .code_metadata(
                CodeMetadata::UPGRADEABLE
                    | CodeMetadata::READABLE
                    | CodeMetadata::PAYABLE
                    | CodeMetadata::PAYABLE_BY_SC,
            )
            .argument(&identity_addr_managed)
            .returns(ReturnsNewAddress)
            .run()
            .await;

        println!("Deployed Validation Registry at: {}", contract_address);

        Self {
            wallet_address,
            contract_address,
        }
    }

    pub async fn init_job(&self, interactor: &mut Interactor, job_id: &str, agent_nonce: u64) {
        let job_id_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(job_id.as_bytes());

        interactor
            .tx()
            .from(&self.wallet_address)
            .to(&self.contract_address)
            .gas(600_000_000)
            .raw_call("init_job")
            .argument(&job_id_buf)
            .argument(&agent_nonce)
            .run()
            .await;
    }

    pub async fn init_job_with_payment(
        &self,
        interactor: &mut Interactor,
        job_id: &str,
        agent_nonce: u64,
        service_id: u32,
        payment_token: &str,
        payment_amount: u64,
    ) {
        let job_id_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(job_id.as_bytes());
        if payment_token == "EGLD" {
            interactor
                .tx()
                .from(&self.wallet_address)
                .to(&self.contract_address)
                .gas(600_000_000)
                .egld(payment_amount)
                .raw_call("init_job")
                .argument(&job_id_buf)
                .argument(&agent_nonce)
                .argument(&service_id)
                .run()
                .await;
        } else {
            let token_id: TokenIdentifier<StaticApi> = TokenIdentifier::from(payment_token);
            let amount_big = BigUint::from(payment_amount);
            interactor
                .tx()
                .from(&self.wallet_address)
                .to(&self.contract_address)
                .gas(600_000_000)
                .single_esdt(&token_id, 0, &amount_big)
                .raw_call("init_job")
                .argument(&job_id_buf)
                .argument(&agent_nonce)
                .argument(&service_id)
                .run()
                .await;
        }
    }

    pub async fn submit_proof(&self, interactor: &mut Interactor, job_id: &str, proof_hash: &str) {
        let job_id_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(job_id.as_bytes());
        let proof_buf: ManagedBuffer<StaticApi> =
            ManagedBuffer::new_from_bytes(proof_hash.as_bytes());

        interactor
            .tx()
            .from(&self.wallet_address)
            .to(&self.contract_address)
            .gas(600_000_000)
            .raw_call("submit_proof")
            .argument(&job_id_buf)
            .argument(&proof_buf)
            .run()
            .await;
    }

    pub async fn validation_request(
        &self,
        interactor: &mut Interactor,
        job_id: &str,
        validator_address: &Address,
        request_uri: &str,
        request_hash: &str,
    ) {
        let job_id_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(job_id.as_bytes());
        let validator_managed: ManagedAddress<StaticApi> =
            ManagedAddress::from_address(validator_address);
        let uri_buf: ManagedBuffer<StaticApi> =
            ManagedBuffer::new_from_bytes(request_uri.as_bytes());
        let hash_buf: ManagedBuffer<StaticApi> =
            ManagedBuffer::new_from_bytes(request_hash.as_bytes());

        interactor
            .tx()
            .from(&self.wallet_address)
            .to(&self.contract_address)
            .gas(600_000_000)
            .raw_call("validation_request")
            .argument(&job_id_buf)
            .argument(&validator_managed)
            .argument(&uri_buf)
            .argument(&hash_buf)
            .run()
            .await;
    }

    pub async fn validation_response(
        &self,
        interactor: &mut Interactor,
        request_hash: &str,
        response: u8,
        response_uri: &str,
        response_hash: &str,
        tag: &str,
    ) {
        let hash_buf: ManagedBuffer<StaticApi> =
            ManagedBuffer::new_from_bytes(request_hash.as_bytes());
        let uri_buf: ManagedBuffer<StaticApi> =
            ManagedBuffer::new_from_bytes(response_uri.as_bytes());
        let resp_hash_buf: ManagedBuffer<StaticApi> =
            ManagedBuffer::new_from_bytes(response_hash.as_bytes());
        let tag_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(tag.as_bytes());

        interactor
            .tx()
            .from(&self.wallet_address)
            .to(&self.contract_address)
            .gas(600_000_000)
            .raw_call("validation_response")
            .argument(&hash_buf)
            .argument(&response)
            .argument(&uri_buf)
            .argument(&resp_hash_buf)
            .argument(&tag_buf)
            .run()
            .await;
    }

    pub async fn clean_old_jobs(&self, interactor: &mut Interactor, job_ids: Vec<&str>) {
        let mut request = interactor
            .tx()
            .from(&self.wallet_address)
            .to(&self.contract_address)
            .gas(600_000_000)
            .raw_call("clean_old_jobs");

        for id in job_ids {
            let id_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(id.as_bytes());
            request = request.argument(&id_buf);
        }

        request.run().await;
    }

    pub fn address(&self) -> &Address {
        &self.contract_address
    }
}

pub async fn deploy_all_registries(
    interactor: &mut Interactor,
    owner: Address,
) -> (IdentityRegistryInteractor, Address, Address) {
    // 1. Deploy Identity
    let identity = IdentityRegistryInteractor::init(interactor, owner.clone()).await;
    identity
        .issue_token(interactor, "AgentToken", "AGENT")
        .await;
    let identity_addr = identity.address().clone();

    // 2. Deploy Validation
    println!("Reading Validation WASM from: {}", VALIDATION_WASM_PATH);
    let validation_wasm =
        std::fs::read(VALIDATION_WASM_PATH).expect("Failed to read validation WASM");
    let validation_code = ManagedBuffer::<StaticApi>::new_from_bytes(&validation_wasm);
    let identity_addr_arg = ManagedBuffer::<StaticApi>::new_from_bytes(identity_addr.as_bytes());

    let validation_addr: Address = interactor
        .tx()
        .from(&owner)
        .gas(600_000_000)
        .raw_deploy()
        .code(validation_code)
        .code_metadata(
            CodeMetadata::UPGRADEABLE
                | CodeMetadata::READABLE
                | CodeMetadata::PAYABLE
                | CodeMetadata::PAYABLE_BY_SC,
        )
        .argument(&identity_addr_arg)
        .returns(ReturnsNewAddress)
        .run()
        .await;
    println!("Deployed Validation Registry at: {}", validation_addr);

    // 3. Deploy Reputation
    println!("Reading Reputation WASM from: {}", REPUTATION_WASM_PATH);
    let reputation_wasm =
        std::fs::read(REPUTATION_WASM_PATH).expect("Failed to read reputation WASM");
    let reputation_code = ManagedBuffer::<StaticApi>::new_from_bytes(&reputation_wasm);
    let validation_addr_arg =
        ManagedBuffer::<StaticApi>::new_from_bytes(validation_addr.as_bytes());
    let identity_addr_arg_rep =
        ManagedBuffer::<StaticApi>::new_from_bytes(identity_addr.as_bytes());

    let reputation_addr: Address = interactor
        .tx()
        .from(&owner)
        .gas(600_000_000)
        .raw_deploy()
        .code(reputation_code)
        .code_metadata(
            CodeMetadata::UPGRADEABLE
                | CodeMetadata::READABLE
                | CodeMetadata::PAYABLE
                | CodeMetadata::PAYABLE_BY_SC,
        )
        .argument(&validation_addr_arg)
        .argument(&identity_addr_arg_rep)
        .returns(ReturnsNewAddress)
        .run()
        .await;
    println!("Deployed Reputation Registry at: {}", reputation_addr);

    (identity, validation_addr, reputation_addr)
}

pub async fn issue_fungible_esdt(
    interactor: &mut Interactor,
    issuer: &Address,
    name: &str,
    ticker: &str,
    supply: u128,
    decimals: usize,
    gateway_url: &str,
) -> String {
    issue_fungible_esdt_custom(
        interactor,
        issuer,
        name,
        ticker,
        supply,
        decimals,
        gateway_url,
    )
    .await
}

pub async fn issue_fungible_esdt_custom(
    interactor: &mut Interactor,
    issuer: &Address,
    name: &str,
    ticker: &str,
    supply: u128,
    decimals: usize,
    gateway_url: &str,
) -> String {
    let name_buf = ManagedBuffer::<StaticApi>::from(name.as_bytes());
    let ticker_buf = ManagedBuffer::<StaticApi>::from(ticker.as_bytes());
    let supply_big = BigUint::<StaticApi>::from(supply);
    let decimals_u32 = decimals as u32;

    // ESDT system SC: erd1qqqqqqqqqqqqqqqpqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqzllls8a5w6u
    let esdt_system_sc_bytes =
        hex::decode("000000000000000000010000000000000000000000000000000000000002ffff").unwrap();
    let esdt_system_sc = Address::from_slice(&esdt_system_sc_bytes);

    interactor
        .tx()
        .from(issuer)
        .to(&esdt_system_sc)
        .egld(50_000_000_000_000_000u64) // 0.05 EGLD
        .gas(60_000_000)
        .raw_call("issue")
        .argument(&name_buf)
        .argument(&ticker_buf)
        .argument(&supply_big)
        .argument(&decimals_u32)
        .run()
        .await;

    println!("Issued ESDT {}...", ticker);
    interactor.generate_blocks(15).await.ok();

    // Fetch token ID via HTTP API
    let issuer_bech32 = address_to_bech32(issuer);
    let client = reqwest::Client::new();
    let url = format!("{}/address/{}/esdt", gateway_url, issuer_bech32);

    // Retry loop for API consistency
    for _ in 0..5 {
        let resp = client.get(&url).send().await;
        if let Ok(r) = resp {
            if let Ok(json) = r.json::<serde_json::Value>().await {
                if let Some(tokens) = json["data"]["esdts"].as_object() {
                    for key in tokens.keys() {
                        if key.starts_with(ticker) {
                            println!("Found Token: {}", key);
                            return key.clone();
                        }
                    }
                }
            }
        }
        interactor.generate_blocks(1).await.ok();
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    panic!(
        "Failed to find issued token {} for {}",
        ticker, issuer_bech32
    );
}
