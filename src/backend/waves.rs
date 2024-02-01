use std::str::FromStr;

use anyhow::{bail, Result};
use axum::async_trait;
use libzeropool_rs::libzeropool::fawkes_crypto::{engines::U256, ff_uint::Uint};
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
    Fr, Proof,
};

// TODO: Calculate tx fee properly.
// 0.01 WAVES
const TX_FEE: u64 = 10_000_000;

// TODO: Specify pool address separately from relayer address.

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    seed: String,
    profile: String,
    pool_address: String,
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
        let address = Address::from_string(&config.pool_address)?;
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
    fn name(&self) -> &'static str {
        "waves"
    }

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

    async fn validate_tx(&self, _tx: &ParsedTxData) -> Vec<TxValidationError> {
        vec![]
    }

    /// Sign and send a transaction to the blockchain.
    async fn send_tx(&self, tx: TxData<Fr, Proof>) -> Result<TxHash> {
        let mut tx_bytes = Vec::new();
        zeropool_tx::waves::write(&tx, &mut tx_bytes)?;

        let base64_tx = Base64String::from_bytes(tx_bytes);

        tracing::debug!("Transaction {:?}", base64_tx);

        let transaction_data = TransactionData::InvokeScript(InvokeScriptTransaction::new(
            self.address.clone(),
            Function::new("transact".to_owned(), vec![Arg::Binary(base64_tx)]),
            vec![],
        ));

        let timestamp = get_current_epoch_millis();
        let signed_tx = Transaction::new(
            transaction_data,
            Amount::new(TX_FEE, None),
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
        let index = self.node.get_data_by_key(&self.address, "PoolIndex").await;

        match index {
            Ok(DataEntry::IntegerEntry { value, .. }) => Ok(value as u64),
            Ok(_) => {
                bail!("PoolIndex is not an integer");
            }
            Err(err) => {
                tracing::warn!("Failed to get PoolIndex: {}", err);
                Ok(0)
            }
        }
    }

    async fn get_merkle_root(&self, index: u64) -> Result<Option<U256>> {
        if index == 0 {
            let first_root = U256::from_str(
                "11469701942666298368112882412133877458305516134926649826543144744382391691533",
            )
            .unwrap();
            return Ok(Some(first_root));
        }

        let result = self
            .node
            .get_data_by_key(&self.address, &format!("R:{index}"))
            .await;

        match result {
            Ok(DataEntry::BinaryEntry { value, .. }) => {
                if value.len() != std::mem::size_of::<U256>() {
                    bail!("Invalid merkle root length");
                }

                let root = U256::from_big_endian(&value);

                Ok(Some(root))
            }
            _ => {
                bail!("R:{index} is not a binary entry");
            }
        }
    }

    fn parse_calldata(&self, calldata: Vec<u8>) -> Result<TxData<Fr, Proof>> {
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
