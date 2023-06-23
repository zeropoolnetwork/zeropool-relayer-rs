use anyhow::Result;
use serde::Deserialize;

use crate::{
    backend::{BlockchainBackend, TxHash},
    tx::{FullTxData, ParsedTxData, TxValidationError},
};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {}

pub struct SubstrateBackend {
    private_key: PrivateKey,
    public_key: PublicKey,
    address: Address,
    node: Node,
}

impl SubstrateBackend {
    pub async fn new() -> Result<Self> {
        Ok(Self {
            private_key,
            public_key,
            address,
            node,
        })
    }
}

#[async_trait]
impl BlockchainBackend for SubstrateBackend {
    async fn init_state(&mut self, _staring_block: u64) -> Result<()> {
        Ok(())
    }
    
    fn validate_tx(&self, tx: &ParsedTxData) -> Vec<TxValidationError> {
        vec![]
    }

    /// Sign and send a transaction to the blockchain.
    async fn send_tx(&self, tx: FullTxData) -> Result<TxHash> {
        todo!()
    }

    async fn get_pool_index(&self) -> Result<u64> {
        todo!()
    }

    fn parse_calldata(&self, calldata: Vec<u8>) -> Result<FullTxData> {
        todo!()
    }

    fn parse_hash(&self, hash: &str) -> Result<Vec<u8>> {
        bs58::decode(hash).into_vec().map_err(Into::into)
    }

    fn format_hash(&self, hash: &[u8]) -> String {
        bs58::encode(hash).into_string()
    }
}
