use anyhow::Result;
use axum::async_trait;

use crate::tx::{FullTxData, ParsedTxData, TxDataRequest, TxValidationError};

#[cfg(feature = "evm_backend")]
pub mod evm;
#[cfg(feature = "near_backend")]
pub mod near;
#[cfg(feature = "waves_backend")]
pub mod waves;

#[async_trait]
pub trait BlockchainBackend: Sync + Send {
    /// Validate transaction data.
    fn validate_tx(&self, tx: &ParsedTxData) -> Vec<TxValidationError>;

    /// Create, sign, and send transaction to the blockchain.
    async fn send_tx(&self, tx: FullTxData) -> Result<TxHash>;

    async fn get_pool_index(&self) -> Result<u64>;

    fn parse_calldata(&self, calldata: Vec<u8>) -> Result<FullTxData>;
    fn parse_hash(&self, hash: &str) -> Result<Vec<u8>>;
}

pub type TxHash = Vec<u8>;
