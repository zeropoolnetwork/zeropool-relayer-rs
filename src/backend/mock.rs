use anyhow::Result;
use axum::async_trait;
use fawkes_crypto::engines::U256;
use tokio::sync::Mutex;
use zeropool_tx::TxData;

use crate::{
    backend::{BlockchainBackend, TxCalldata, TxHash},
    tx::{ParsedTxData, TxValidationError},
    Engine,
};

pub struct MockBackend {
    pool_index: Mutex<u64>,
}

impl MockBackend {
    pub fn new() -> Self {
        Self {
            pool_index: Mutex::new(0),
        }
    }
}

#[async_trait]
impl BlockchainBackend for MockBackend {
    async fn fetch_latest_transactions(&self) -> Result<Vec<TxCalldata>> {
        Ok(vec![])
    }

    fn validate_tx(&self, _tx: &ParsedTxData) -> Vec<TxValidationError> {
        vec![]
    }

    /// Sign and send a transaction to the blockchain.
    async fn send_tx(&self, _tx: TxData<Engine>) -> Result<TxHash> {
        let mut pool_index = self.pool_index.lock().await;
        *pool_index += 128;
        Ok(pool_index.to_be_bytes().to_vec())
    }

    async fn get_pool_index(&self) -> Result<u64> {
        Ok(*self.pool_index.lock().await)
    }

    async fn get_merkle_root(&self, index: u64) -> Result<Option<U256>> {
        return Ok(Some(U256::from(index)));
    }

    fn parse_calldata(&self, calldata: Vec<u8>) -> Result<TxData<Engine>> {
        Ok(bincode::deserialize(&calldata)?)
    }

    fn parse_hash(&self, hash: &str) -> Result<Vec<u8>> {
        let hash = hex::decode(hash)?;
        Ok(hash)
    }

    fn format_hash(&self, hash: &[u8]) -> String {
        hex::encode(hash)
    }
}
