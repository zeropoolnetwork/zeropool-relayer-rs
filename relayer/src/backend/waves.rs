use anyhow::{bail, Result};
use axum::async_trait;
use serde::Deserialize;
use waves_rust::{
    api::{Node, Profile},
    model::{
        data_entry::DataEntry, Address, Amount, Arg::Binary, Base64String, ByteString, ChainId,
        Function, InvokeScriptTransaction, PrivateKey, PublicKey, Transaction, TransactionData,
    },
    util::get_current_epoch_millis,
};
use zeropool_tx::TxData;

use crate::{
    backend::{BlockchainBackend, TxCalldata, TxHash},
    tx::{ParsedTxData, TxValidationError},
    Engine,
};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    seed: String,
}

pub struct WavesBackend {
    private_key: PrivateKey,
    public_key: PublicKey,
    address: Address,
    node: Node,
}

impl WavesBackend {
    pub async fn new(config: Config) -> Result<Self> {
        let private_key = PrivateKey::from_seed(&config.seed, 0)?;
        let public_key = private_key.public_key();
        let address = public_key.address(ChainId::TESTNET.byte())?;
        let node = Node::from_profile(Profile::TESTNET);

        tracing::info!("Current height is {}", node.get_height().await?);
        tracing::info!("Relayer balance: {}", node.get_balance(&address).await?);

        Ok(Self {
            private_key,
            public_key,
            address,
            node,
        })
    }
}

#[async_trait]
impl BlockchainBackend for WavesBackend {
    async fn init_state(&self) -> Result<Vec<TxCalldata>> {
        Ok(vec![])
    }

    fn validate_tx(&self, _tx: &ParsedTxData) -> Vec<TxValidationError> {
        vec![]
    }

    /// Sign and send a transaction to the blockchain.
    async fn send_tx(&self, tx: TxData<Engine>) -> Result<TxHash> {
        let mut tx_data = Vec::new();
        zeropool_tx::waves::write(&tx, &mut tx_data)?;

        let transaction_data = TransactionData::InvokeScript(InvokeScriptTransaction::new(
            self.address.clone(),
            Function::new(
                "transact".to_owned(),
                vec![Binary(Base64String::from_bytes(tx_data))],
            ),
            vec![],
        ));

        let timestamp = get_current_epoch_millis();
        let signed_tx = Transaction::new(
            transaction_data,
            Amount::new(0, None),
            timestamp,
            self.public_key.clone(),
            3,
            ChainId::TESTNET.byte(),
        )
        .sign(&self.private_key)?;

        let res = self.node.broadcast(&signed_tx).await?;
        let tx_id = res.id()?;
        Ok(ByteString::bytes(&tx_id))
    }

    async fn get_pool_index(&self) -> Result<u64> {
        let index = self
            .node
            .get_data_by_key(&self.address, "PoolIndex")
            .await?;

        let index_num = match index {
            DataEntry::IntegerEntry { value, .. } => value as u64,
            _ => bail!("PoolIndex is not an integer"),
        };

        Ok(index_num)
    }

    fn parse_calldata(&self, calldata: Vec<u8>) -> Result<TxData<Engine>> {
        let r = &mut calldata.as_slice();
        let tx = zeropool_tx::waves::read(r)?;
        Ok(tx)
    }

    fn parse_hash(&self, hash: &str) -> Result<Vec<u8>> {
        bs58::decode(hash).into_vec().map_err(Into::into)
    }

    fn format_hash(&self, hash: &[u8]) -> String {
        bs58::encode(hash).into_string()
    }
}
