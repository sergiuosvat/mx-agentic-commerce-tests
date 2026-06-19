use multiversx_sc::derive_imports::*;
use multiversx_sc::types::{Address, CodeMetadata, ManagedBuffer};
use multiversx_sc_snippets::imports::*;

pub const ESCROW_WASM_PATH: &str = "artifacts/escrow.wasm";

#[type_abi]
#[derive(TopEncode, TopDecode, NestedEncode, NestedDecode, PartialEq, Debug)]
pub enum EscrowStatus {
    Active,
    Released,
    Refunded,
}

#[type_abi]
#[derive(TopEncode, TopDecode, NestedEncode, NestedDecode, PartialEq, Debug)]
pub struct EscrowData<M: ManagedTypeApi> {
    pub employer: ManagedAddress<M>,
    pub receiver: ManagedAddress<M>,
    pub token_id: EgldOrEsdtTokenIdentifier<M>,
    pub token_nonce: u64,
    pub amount: BigUint<M>,
    pub poa_hash: ManagedBuffer<M>,
    pub deadline: u64,
    pub status: EscrowStatus,
}

pub struct EscrowDeposit<'a> {
    pub job_id: &'a str,
    pub receiver: &'a Address,
    pub poa_hash: &'a str,
    pub deadline: u64,
    pub amount_wei: u64,
}

pub struct EscrowInteractor {
    pub wallet_address: Address,
    pub contract_address: Address,
}

impl EscrowInteractor {
    pub async fn deploy(
        interactor: &mut Interactor,
        wallet_address: Address,
        validation_address: &Address,
        identity_address: &Address,
    ) -> Self {
        let wasm_bytes = std::fs::read(ESCROW_WASM_PATH).expect("Failed to read escrow WASM");
        let code_buf = ManagedBuffer::new_from_bytes(&wasm_bytes);

        let val_addr: ManagedAddress<StaticApi> = ManagedAddress::from_address(validation_address);
        let id_addr: ManagedAddress<StaticApi> = ManagedAddress::from_address(identity_address);

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
            .argument(&val_addr)
            .argument(&id_addr)
            .returns(ReturnsNewAddress)
            .run()
            .await;

        println!("Deployed Escrow at: {}", contract_address);

        Self {
            wallet_address,
            contract_address,
        }
    }

    /// Deposit EGLD into escrow for a job.
    pub async fn deposit_egld(
        &self,
        interactor: &mut Interactor,
        job_id: &str,
        receiver: &Address,
        poa_hash: &str,
        deadline: u64,
        amount_wei: u64,
    ) {
        let job_id_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(job_id.as_bytes());
        let receiver_addr: ManagedAddress<StaticApi> = ManagedAddress::from_address(receiver);
        let poa_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(poa_hash.as_bytes());

        interactor
            .tx()
            .from(&self.wallet_address)
            .to(&self.contract_address)
            .gas(600_000_000)
            .egld(amount_wei)
            .raw_call("deposit")
            .argument(&job_id_buf)
            .argument(&receiver_addr)
            .argument(&poa_buf)
            .argument(&deadline)
            .run()
            .await;

        println!("Deposited {} EGLD for job '{}'", amount_wei, job_id);
    }

    /// Deposit EGLD and expect an error.
    pub async fn deposit_egld_expect_err(
        &self,
        interactor: &mut Interactor,
        deposit: EscrowDeposit<'_>,
        expected_err: &str,
    ) {
        let job_id_buf: ManagedBuffer<StaticApi> =
            ManagedBuffer::new_from_bytes(deposit.job_id.as_bytes());
        let receiver_addr: ManagedAddress<StaticApi> =
            ManagedAddress::from_address(deposit.receiver);
        let poa_buf: ManagedBuffer<StaticApi> =
            ManagedBuffer::new_from_bytes(deposit.poa_hash.as_bytes());

        interactor
            .tx()
            .from(&self.wallet_address)
            .to(&self.contract_address)
            .gas(600_000_000)
            .egld(deposit.amount_wei)
            .raw_call("deposit")
            .argument(&job_id_buf)
            .argument(&receiver_addr)
            .argument(&poa_buf)
            .argument(&deposit.deadline)
            .returns(ExpectError(4, expected_err))
            .run()
            .await;

        println!("deposit_egld correctly failed with: '{}'", expected_err);
    }

    /// Release escrow to the receiver.
    pub async fn release(&self, interactor: &mut Interactor, job_id: &str) {
        let job_id_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(job_id.as_bytes());

        interactor
            .tx()
            .from(&self.wallet_address)
            .to(&self.contract_address)
            .gas(600_000_000)
            .raw_call("release")
            .argument(&job_id_buf)
            .run()
            .await;

        println!("Released escrow for job '{}'", job_id);
    }

    /// Release escrow and expect an error.
    pub async fn release_expect_err(
        &self,
        interactor: &mut Interactor,
        job_id: &str,
        expected_err: &str,
    ) {
        let job_id_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(job_id.as_bytes());

        interactor
            .tx()
            .from(&self.wallet_address)
            .to(&self.contract_address)
            .gas(600_000_000)
            .raw_call("release")
            .argument(&job_id_buf)
            .returns(ExpectError(4, expected_err))
            .run()
            .await;

        println!("release correctly failed with: '{}'", expected_err);
    }

    /// Refund escrow to the employer (callable by anyone if deadline passed).
    pub async fn refund(&self, interactor: &mut Interactor, caller: &Address, job_id: &str) {
        let job_id_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(job_id.as_bytes());

        interactor
            .tx()
            .from(caller)
            .to(&self.contract_address)
            .gas(600_000_000)
            .raw_call("refund")
            .argument(&job_id_buf)
            .run()
            .await;

        println!("Refunded escrow for job '{}'", job_id);
    }

    /// Refund escrow and expect an error.
    pub async fn refund_expect_err(
        &self,
        interactor: &mut Interactor,
        caller: &Address,
        job_id: &str,
        expected_err: &str,
    ) {
        let job_id_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(job_id.as_bytes());

        interactor
            .tx()
            .from(caller)
            .to(&self.contract_address)
            .gas(600_000_000)
            .raw_call("refund")
            .argument(&job_id_buf)
            .returns(ExpectError(4, expected_err))
            .run()
            .await;

        println!("refund correctly failed with: '{}'", expected_err);
    }

    /// Query the escrow data for a job via the vm_query helper.
    pub async fn get_escrow(
        &self,
        interactor: &mut Interactor,
        job_id: &str,
    ) -> EscrowData<StaticApi> {
        let job_id_buf: ManagedBuffer<StaticApi> = ManagedBuffer::new_from_bytes(job_id.as_bytes());

        interactor
            .query()
            .to(&self.contract_address)
            .raw_call("get_escrow")
            .argument(&job_id_buf)
            .original_result()
            .returns(ReturnsResult)
            .run()
            .await
    }

    pub fn address(&self) -> &Address {
        &self.contract_address
    }
}
