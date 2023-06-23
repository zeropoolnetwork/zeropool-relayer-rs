use anyhow::Result;
use axum::async_trait;
use zeropool_tx::TxData;

use crate::{
    tx::{ParsedTxData, TxValidationError},
    Engine,
};

#[cfg(feature = "evm_backend")]
pub mod evm;
pub mod mock;
#[cfg(feature = "near_backend")]
pub mod near;
#[cfg(feature = "waves_backend")]
pub mod waves;

#[async_trait]
pub trait BlockchainBackend: Sync + Send {
    async fn init_state(&mut self, staring_block: u64) -> Result<()>;

    /// Validate transaction data.
    fn validate_tx(&self, tx: &ParsedTxData) -> Vec<TxValidationError>;

    /// Create, sign, and send transaction to the blockchain.
    async fn send_tx(&self, tx: TxData<Engine>) -> Result<TxHash>;

    /// Fetch the current pool index from the blockchain.
    async fn get_pool_index(&self) -> Result<u64>;
    // async fn get_merkle_root(&self) -> Result<Vec<u8>>;

    fn parse_calldata(&self, calldata: Vec<u8>) -> Result<TxData<Engine>>;

    fn parse_hash(&self, hash: &str) -> Result<Vec<u8>>;
    fn format_hash(&self, hash: &[u8]) -> String;
}

pub type TxHash = Vec<u8>;