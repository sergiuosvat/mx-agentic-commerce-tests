use super::mpp_session_mvx_proxy::MppSessionContractProxy;
use ed25519_dalek::{Signer, SigningKey};
use multiversx_sc_snippets::imports::*;
use tiny_keccak::{Hasher, Keccak};

pub const SESSION_MXSC_PATH: &str = "mxsc:../mpp-session-mvx/output/mpp-session-mvx.mxsc.json";

pub fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut keccak = Keccak::v256();
    keccak.update(data);
    let mut output = [0u8; 32];
    keccak.finalize(&mut output);
    output
}

/// Build voucher signature bytes matching `mpp-session-mvx` `verify_voucher`.
pub fn sign_session_voucher(
    signing_key: &SigningKey,
    sc_address: &Address,
    channel_id: &[u8],
    amount: u64,
    nonce: u64,
) -> Vec<u8> {
    let mut message = Vec::new();
    message.extend_from_slice(b"mpp-session-v1");
    message.extend_from_slice(sc_address.as_bytes());
    message.extend_from_slice(channel_id);

    let mut amount_vec = amount.to_be_bytes().to_vec();
    while amount_vec.len() > 1 && amount_vec[0] == 0 {
        amount_vec.remove(0);
    }
    message.extend_from_slice(&amount_vec);
    message.extend_from_slice(&nonce.to_be_bytes());

    let hash = keccak256(&message);
    signing_key.sign(&hash).to_bytes().to_vec()
}

pub async fn deploy_session_contract(
    interactor: &mut Interactor,
    deployer: &Address,
) -> Address {
    let contract_code = BytesValue::interpret_from(
        SESSION_MXSC_PATH,
        &InterpreterContext::default(),
    );

    interactor
        .tx()
        .from(deployer)
        .gas(30_000_000u64)
        .typed(MppSessionContractProxy)
        .init()
        .code(&contract_code)
        .returns(ReturnsNewAddress)
        .run()
        .await
}

pub async fn open_session(
    interactor: &mut Interactor,
    employer: &Address,
    sc_address: &Address,
    receiver: &Address,
    deadline: u64,
    egld_amount: u64,
) -> Vec<u8> {
    let channel_id_buf: ManagedBuffer<StaticApi> = interactor
        .tx()
        .from(employer)
        .to(sc_address)
        .gas(30_000_000u64)
        .typed(MppSessionContractProxy)
        .open(receiver.clone(), deadline)
        .egld(egld_amount)
        .returns(ReturnsResult)
        .run()
        .await;

    channel_id_buf.to_vec()
}

pub async fn query_session(
    interactor: &mut Interactor,
    sc_address: &Address,
    channel_id: &[u8],
) -> super::mpp_session_mvx_proxy::SessionData<StaticApi> {
    interactor
        .query()
        .to(sc_address)
        .typed(MppSessionContractProxy)
        .sessions(ManagedBuffer::new_from_bytes(channel_id))
        .returns(ReturnsResult)
        .run()
        .await
}

pub struct SessionWallets {
    pub employer_signing_key: SigningKey,
    pub employer_addr: Address,
    pub receiver_addr: Address,
}

pub async fn setup_session_wallets(interactor: &mut Interactor) -> SessionWallets {
    let mut csprng = rand::rngs::OsRng;
    let employer_signing_key = SigningKey::generate(&mut csprng);
    let employer_pk_hex = hex::encode(employer_signing_key.to_bytes());
    let employer_wallet = Wallet::from_private_key(&employer_pk_hex).unwrap();
    let employer_addr = interactor.register_wallet(employer_wallet).await;

    let receiver_pk_hex = hex::encode(SigningKey::generate(&mut csprng).to_bytes());
    let receiver_wallet = Wallet::from_private_key(&receiver_pk_hex).unwrap();
    let receiver_addr = interactor.register_wallet(receiver_wallet).await;

    SessionWallets {
        employer_signing_key,
        employer_addr,
        receiver_addr,
    }
}
