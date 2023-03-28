use anyhow::{bail, Result};
use near_lake_framework::{
    near_indexer_primitives::{
        views::{ActionView, ExecutionStatusView},
        StreamerMessage,
    },
    LakeConfigBuilder,
};
use serde::Deserialize;
use tokio::{sync::mpsc, task::JoinHandle};
use zeropool_indexer_tx_storage::Tx;

use crate::backend::{Backend, BackendMethods};

const LATEST_BLOCK_HEIGHT_FILE: &str = "near_latest_checked_block_height";

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub contract_address: String,
    pub chain_id: String,
    /// Starting block height
    pub block_height: u64,
}

pub struct NearLakeFrameworkBackend {
    config: Config,
    latest_tx_block_height: Option<u64>,
}

impl Backend for NearLakeFrameworkBackend {
    type Config = Config;

    fn new(backend_config: Self::Config, latest_tx: Option<Tx>) -> Result<Self> {
        Ok(Self {
            config: backend_config,
            latest_tx_block_height: latest_tx.map(|tx| tx.block_height),
        })
    }
}

#[async_trait::async_trait]
impl BackendMethods for NearLakeFrameworkBackend {
    async fn start(self, send: mpsc::Sender<Tx>) -> Result<JoinHandle<Result<()>>> {
        let block_height = read_latest_block_height()
            .await
            .unwrap_or(self.config.block_height)
            .max(self.latest_tx_block_height.unwrap_or(0));

        tracing::info!("Starting sync from block {}", block_height);

        let mut lake_config = LakeConfigBuilder::default().start_block_height(block_height);

        match self.config.chain_id.as_str() {
            "mainnet" => lake_config = lake_config.mainnet(),
            "testnet" => lake_config = lake_config.testnet(),
            _ => bail!("Unsupported chain id: {}", self.config.chain_id),
        };

        let (_, mut stream) = near_lake_framework::streamer(lake_config.build()?);

        let handle = tokio::spawn(async move {
            while let Some(streamer_message) = stream.recv().await {
                handle_streamer_message(
                    streamer_message,
                    &self.config.contract_address,
                    send.clone(),
                )
                .await;
            }

            Ok(())
        });

        Ok(handle)
    }
}

async fn handle_streamer_message(
    message: StreamerMessage,
    contract_address: &str,
    send: mpsc::Sender<Tx>,
) {
    for shard in message.shards {
        if let Err(err) = cache_latest_block_height(message.block.header.height).await {
            tracing::warn!("Failed to cache latest block id: {}", err);
        }

        if let Some(chunk) = shard.chunk {
            for t in chunk.transactions {
                match t.outcome.execution_outcome.outcome.status {
                    ExecutionStatusView::Unknown => {
                        tracing::trace!("Skipping tx with unknown status");
                        continue;
                    }
                    ExecutionStatusView::Failure(_) => {
                        tracing::trace!("Skipping failed tx");
                        continue;
                    }
                    _ => (),
                }

                if t.transaction.receiver_id.as_str() != contract_address {
                    continue;
                }

                for action in t.transaction.actions {
                    if let ActionView::FunctionCall {
                        method_name, args, ..
                    } = action
                    {
                        if method_name != "transact" {
                            tracing::info!("Skipping non-'transact' transaction");
                            continue;
                        }

                        let tx = Tx {
                            hash: t.transaction.hash.to_string(),
                            block_hash: message.block.header.hash.to_string(),
                            block_height: message.block.header.height,
                            timestamp: message.block.header.timestamp_nanosec,
                            sender_address: t.transaction.signer_id.to_string(),
                            receiver_address: t.transaction.receiver_id.to_string(),
                            signature: t.transaction.signature.to_string(),
                            calldata: args,
                        };

                        send.send(tx)
                            .await
                            .expect("Failed to send tx to the channel");
                    }
                }
            }
        }
    }
}

async fn cache_latest_block_height(block_id: u64) -> Result<()> {
    tokio::fs::write(LATEST_BLOCK_HEIGHT_FILE, block_id.to_string()).await?;

    Ok(())
}

async fn read_latest_block_height() -> Result<u64> {
    let latest_block_id = tokio::fs::read_to_string(LATEST_BLOCK_HEIGHT_FILE).await?;

    Ok(latest_block_id.parse()?)
}
