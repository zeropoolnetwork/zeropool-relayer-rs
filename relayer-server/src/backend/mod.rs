use anyhow::Result;
use axum::async_trait;

use crate::tx::{ParsedTxData, RawTxData, TxDataRequest, TxValidationError};

#[cfg(feature = "evm_backend")]
pub mod evm;
#[cfg(feature = "near_backend")]
pub mod near;
#[cfg(feature = "waves_backend")]
pub mod waves;

pub trait BlockchainBackend: TxSender + Sync + Send {
    /// Disassemble and validate transaction data.
    fn parse_tx(&self, tx: RawTxData) -> Result<ParsedTxData>;
}

pub type TxHash = Vec<u8>;

#[async_trait]
pub trait TxSender {
    /// Create, sign, and send transaction to the blockchain.
    async fn send_tx(&self, tx: ParsedTxData) -> Result<TxHash>;
}
