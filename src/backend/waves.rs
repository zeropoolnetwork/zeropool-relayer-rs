use anyhow::{bail, Result};
use axum::async_trait;
use serde::Deserialize;
use waves_rust::{
    api::{Node, Profile},
    model::{
        data_entry::DataEntry, Address, Amount, ApplicationStatus, Arg, Base64String, ByteString,
        Function, InvokeScriptTransaction, PrivateKey, PublicKey, Transaction, TransactionData,
        TransactionDataInfo,
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
    profile: String,
}

pub struct WavesBackend {
    private_key: PrivateKey,
    public_key: PublicKey,
    address: Address,
    node: Node,
    chain_id: u8,
}

impl WavesBackend {
    pub async fn new(config: Config) -> Result<Self> {
        let profile = match config.profile.as_str() {
            "MAINNET" => Profile::MAINNET,
            "TESTNET" => Profile::TESTNET,
            "STAGENET" => Profile::STAGENET,
            _ => bail!("Invalid profile {}", config.profile),
        };

        let chain_id = profile.chain_id();

        let private_key = PrivateKey::from_seed(&config.seed, 0)?;
        let public_key = private_key.public_key();
        let address = public_key.address(chain_id)?;
        let node = Node::from_profile(profile);

        tracing::info!("Current height is {}", node.get_height().await?);
        tracing::info!("Relayer balance: {}", node.get_balance(&address).await?);

        Ok(Self {
            private_key,
            public_key,
            address,
            node,
            chain_id,
        })
    }
}

#[async_trait]
impl BlockchainBackend for WavesBackend {
    async fn fetch_latest_transactions(&self) -> Result<Vec<TxCalldata>> {
        let mut txs = Vec::new();

        let mut latest_tx_id = None; // FIXME: initialize with latest tx id
        loop {
            let result = self
                .node
                .get_transactions_by_address(&self.address, 100, latest_tx_id.clone())
                .await?;

            if result.is_empty() {
                break;
            }

            for tx in result {
                if tx.status() != ApplicationStatus::Succeed {
                    tracing::debug!("Skipping failed transaction {:?}", tx.id());
                    continue;
                }

                latest_tx_id = Some(tx.id());

                match tx.data() {
                    TransactionDataInfo::Invoke(inv) => {
                        let func = inv.function();
                        if func.name() != "transact" {
                            continue;
                        }

                        let args = inv.function().args();
                        let calldata = if let Some(Arg::Binary(arg)) = args.first() {
                            arg.bytes()
                        } else {
                            tracing::warn!("Found invalid transaction with no calldata");
                            continue;
                        };

                        let hash = tx.id().bytes();
                        let tx_calldata = TxCalldata { hash, calldata };

                        txs.push(tx_calldata);
                    }
                    _ => (),
                }
            }
        }

        Ok(txs)
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
                vec![Arg::Binary(Base64String::from_bytes(tx_data))],
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
            self.chain_id,
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
