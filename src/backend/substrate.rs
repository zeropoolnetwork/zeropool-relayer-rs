use anyhow::Result;
use axum::async_trait;
use libzeropool_rs::libzeropool::fawkes_crypto::engines::U256;
use serde::Deserialize;
use zeropool_tx::{TxData, TxType};

use crate::{
    backend::{BlockchainBackend, TxCalldata, TxHash},
    tx::{ParsedTxData, TxValidationError},
    Fr, Proof,
};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {}

pub struct SubstrateBackend {
    // private_key: PrivateKey,
    // public_key: PublicKey,
    // address: Address,
    // node: Node,
}

impl SubstrateBackend {
    pub async fn new() -> Result<Self> {
        Ok(Self {
            // private_key,
            // public_key,
            // address,
            // node,
        })
    }
}

#[async_trait]
impl BlockchainBackend for SubstrateBackend {
    fn name(&self) -> &'static str {
        "substrate"
    }

    async fn fetch_latest_transactions(&self) -> Result<Vec<TxCalldata>> {
        todo!()
    }

    async fn validate_tx(&self, _tx: &ParsedTxData) -> Vec<TxValidationError> {
        vec![]
    }

    /// Sign and send a transaction to the blockchain.
    async fn send_tx(&self, _tx: TxData<Fr, Proof>) -> Result<TxHash> {
        todo!()
    }

    async fn get_pool_index(&self) -> Result<u64> {
        todo!()
    }

    async fn get_merkle_root(&self, _index: u64) -> Result<Option<U256>> {
        todo!()
    }

    fn parse_calldata(&self, _calldata: Vec<u8>) -> Result<TxData<Fr, Proof>> {
        todo!()
    }

    fn extract_ciphertext_from_memo(&self, _memo: &[u8], _tx_type: TxType) -> &[u8] {
        todo!()
    }

    fn parse_hash(&self, hash: &str) -> Result<Vec<u8>> {
        bs58::decode(hash).into_vec().map_err(Into::into)
    }

    fn format_hash(&self, hash: &[u8]) -> String {
        bs58::encode(hash).into_string()
    }
}
