use anyhow::Result;
use axum::async_trait;
use libzeropool_rs::libzeropool::fawkes_crypto::engines::U256;
use zeropool_tx::TxData;

use crate::{
    tx::{ParsedTxData, TxValidationError},
    Engine, Fr, Proof,
};

#[cfg(feature = "evm_backend")]
pub mod evm;
pub mod mock;
#[cfg(feature = "near_backend")]
pub mod near;
#[cfg(feature = "substrate_backend")]
pub mod substrate;
#[cfg(feature = "waves_backend")]
pub mod waves;

#[async_trait]
pub trait BlockchainBackend: Sync + Send {
    fn name(&self) -> &'static str;

    /// Fetch latest uncached transactions from the blockchain.
    async fn fetch_latest_transactions(&self) -> Result<Vec<TxCalldata>>;

    /// Validate transaction data.
    async fn validate_tx(&self, tx: &ParsedTxData) -> Vec<TxValidationError>;

    /// Create, sign, and send transaction to the blockchain.
    async fn send_tx(&self, tx: TxData<Fr, Proof>) -> Result<TxHash>;

    /// Fetch the current pool index from the blockchain.
    async fn get_pool_index(&self) -> Result<u64>;

    async fn get_merkle_root(&self, index: u64) -> Result<Option<U256>>;

    fn parse_calldata(&self, calldata: Vec<u8>) -> Result<TxData<Fr, Proof>>;

    fn parse_hash(&self, hash: &str) -> Result<Vec<u8>>;
    fn format_hash(&self, hash: &[u8]) -> String;
}

pub type TxHash = Vec<u8>;

pub struct TxCalldata {
    pub hash: TxHash,
    pub calldata: Vec<u8>,
}
