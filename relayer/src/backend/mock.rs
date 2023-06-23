use anyhow::Result;
use axum::async_trait;
use tokio::sync::Mutex;
use zeropool_tx::TxData;

use crate::{
    backend::{BlockchainBackend, TxHash},
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
    async fn init_state(&mut self, _staring_block: u64) -> Result<()> {
        Ok(())
    }
    
    fn validate_tx(&self, _tx: &ParsedTxData) -> Vec<TxValidationError> {
        // let address = recover(&tx.signature, &tx.hash).unwrap();
        // let balance = self
        //     .token
        //     .query("balanceOf", tx.sender, None, Options::default(), None);
        // TODO: Check the balance of the sender for deposits.
        vec![]
    }

    /// Sign and send a transaction to the blockchain.
    async fn send_tx(&self, _tx: TxData<Engine>) -> Result<TxHash> {
        let mut pool_index = self.pool_index.lock().await;
        *pool_index += 1;
        Ok(pool_index.to_be_bytes().to_vec())
    }

    async fn get_pool_index(&self) -> Result<u64> {
        Ok(*self.pool_index.lock().await)
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
