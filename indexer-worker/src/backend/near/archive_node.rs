use anyhow::Result;
use near_primitives::views::{
    ActionView, BlockHeaderView, BlockView, FinalExecutionOutcomeView, FinalExecutionStatus,
};
use serde::Deserialize;
use tokio::{sync::mpsc, task::JoinHandle};
use zeropool_indexer_tx_storage::Tx;

use crate::backend::{Backend, BackendMethods};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub contract_address: String,
    pub chain_id: String,
    pub rpc_url: String,
    pub initial_tx_hashes_path: Option<String>,
}

pub struct NearArchiveNodeBackend {
    config: Config,
}

impl Backend for NearArchiveNodeBackend {
    type Config = Config;

    fn new(backend_config: Self::Config, _latest_tx: Option<Tx>) -> Result<Self> {
        Ok(Self {
            config: backend_config,
        })
    }
}

#[async_trait::async_trait]
impl BackendMethods for NearArchiveNodeBackend {
    async fn start(self, send: mpsc::Sender<Tx>) -> Result<JoinHandle<Result<()>>> {
        let hashes_path = self
            .config
            .initial_tx_hashes_path
            .unwrap_or_else(|| "initial_tx_hashes.json".to_owned());

        let hashes: Vec<String> = serde_json::from_str(&std::fs::read_to_string(hashes_path)?)?;

        for tx_hash in hashes {
            let tx = get_tx_status(
                &self.config.rpc_url,
                &tx_hash,
                &self.config.contract_address,
            )
            .await?;

            let block = get_block(
                &self.config.rpc_url,
                &tx.transaction_outcome.block_hash.to_string(),
            )
            .await?;

            process_tx(
                tx,
                block.header,
                &self.config.contract_address,
                send.clone(),
            )
            .await;
        }

        Ok(tokio::spawn(async move { Ok(()) }))
    }
}

async fn process_tx(
    tx: FinalExecutionOutcomeView,
    block: BlockHeaderView,
    contract_address: &str,
    send: mpsc::Sender<Tx>,
) {
    match tx.status {
        FinalExecutionStatus::Failure(_) => {
            tracing::debug!("Skipping failed tx");
            return;
        }
        _ => (),
    }

    if tx.transaction.receiver_id.as_str() != contract_address {
        tracing::debug!("Skipping tx to another contract");
        return;
    }

    for action in tx.transaction.actions {
        if let ActionView::FunctionCall {
            method_name, args, ..
        } = action
        {
            if method_name != "transact" {
                tracing::info!("Skipping non-'transact' transaction");
                continue;
            }

            let tx = Tx {
                hash: tx.transaction.hash.to_string(),
                block_hash: block.hash.to_string(),
                block_height: block.height,
                timestamp: block.timestamp_nanosec,
                sender_address: tx.transaction.signer_id.to_string(),
                receiver_address: tx.transaction.receiver_id.to_string(),
                signature: tx.transaction.signature.to_string(),
                calldata: args,
            };

            send.send(tx)
                .await
                .expect("Failed to send tx to the channel");
        }
    }
}

async fn get_tx_status(
    rpc_url: &str,
    hash: &str,
    sender: &str,
) -> Result<FinalExecutionOutcomeView> {
    let client = reqwest::Client::new();
    let res: serde_json::Value = client
        .post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": "dontcare",
            "method": "tx",
            "params": [hash, sender]
        }))
        .send()
        .await?
        .json()
        .await?;

    let tx = serde_json::from_value::<FinalExecutionOutcomeView>(res["result"].clone())?;

    Ok(tx)
}

async fn get_block(rpc_url: &str, hash: &str) -> Result<BlockView> {
    let client = reqwest::Client::new();
    let res: serde_json::Value = client
        .post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": "dontcare",
            "method": "block",
            "params": {
                "block_id": hash,
            }
        }))
        .send()
        .await?
        .json()
        .await?;

    let block = serde_json::from_value::<BlockView>(res["result"].clone())?;

    Ok(block)
}
