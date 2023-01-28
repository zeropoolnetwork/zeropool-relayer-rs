use anyhow::{bail, Result};
use near_lake_framework::{
    near_indexer_primitives::{
        views::{ActionView, ExecutionStatusView},
        StreamerMessage,
    },
    LakeConfigBuilder,
};
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::tx::Tx;

pub type BlockId = u64;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub contract_address: String,
    pub chain_id: String,
    pub block_height: BlockId,
}

pub async fn start(
    config: Config,
    starting_block_height: Option<BlockId>,
    send: mpsc::Sender<Tx>,
) -> Result<()> {
    let block_height = starting_block_height.unwrap_or(config.block_height);
    let mut lake_config = LakeConfigBuilder::default().start_block_height(block_height);

    match config.chain_id.as_str() {
        "mainnet" => lake_config = lake_config.mainnet(),
        "testnet" => lake_config = lake_config.testnet(),
        _ => bail!("Unsupported chain id: {}", config.chain_id),
    };

    let (_, mut stream) = near_lake_framework::streamer(lake_config.build()?);

    tokio::spawn(async move {
        while let Some(streamer_message) = stream.recv().await {
            handle_streamer_message(streamer_message, &config.contract_address, send.clone()).await;
        }
    });

    Ok(())
}

async fn handle_streamer_message(
    message: StreamerMessage,
    contract_address: &str,
    send: mpsc::Sender<Tx>,
) {
    for shard in message.shards {
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
                            tracing::trace!("Skipping tx with irrelevant method name");
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
