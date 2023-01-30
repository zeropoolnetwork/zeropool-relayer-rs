use anyhow::Result;
use near_indexer::{
    near_primitives::views::{ActionView, ExecutionStatusView},
    InitConfigArgs,
};
use serde::Deserialize;
use tokio::{sync::mpsc, task::JoinHandle};
use zeropool_indexer_tx_storage::Tx;

pub type BlockId = u64;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub contract_address: String,
    pub chain_id: String,
    pub node_url: Option<String>,
    pub block_height: BlockId,
}

pub async fn start(
    backend_config: Config,
    starting_block_height: Option<BlockId>,
    send: mpsc::Sender<Tx>,
) -> Result<JoinHandle<Result<()>>> {
    tracing::info!("Starting indexer");

    let home_dir = near_indexer::get_default_home();

    let genesis_file = home_dir.join("genesis.json");

    if !genesis_file.is_file() {
        tracing::info!("genesis.json not found, initializing state");

        let init_config = InitConfigArgs {
            chain_id: Some(backend_config.chain_id.clone()),
            account_id: None,
            test_seed: None,
            num_shards: 0,
            fast: false,
            genesis: None,
            download_genesis: true,
            download_genesis_url: None,
            download_config: true,
            download_config_url: None,
            boot_nodes: backend_config.node_url,
            max_gas_burnt_view: None,
        };

        let home_dir_clone = home_dir.clone();
        tokio::task::spawn_blocking(move || {
            if let Err(err) = near_indexer::indexer_init_configs(&home_dir_clone, init_config) {
                tracing::error!("Failed to initialize near state: {}", err);
            }
        })
        .await?;

        tracing::info!("Near state initialized");
    }

    // If there are any transactions in the database, we should start from the interruption.
    let sync_mode = if starting_block_height.is_some() {
        near_indexer::SyncModeEnum::FromInterruption
    } else {
        near_indexer::SyncModeEnum::BlockHeight(backend_config.block_height)
    };

    let indexer_config = near_indexer::IndexerConfig {
        home_dir: home_dir.clone(),
        sync_mode,
        await_for_node_synced: near_indexer::AwaitForNodeSyncedEnum::WaitForFullSync,
    };

    let indexer = near_indexer::Indexer::new(indexer_config)?;
    let stream = indexer.streamer();

    tracing::debug!("Spawning indexer task");
    let handle = tokio::spawn(listen_blocks(stream, backend_config.contract_address, send));

    Ok(handle)
}

async fn listen_blocks(
    mut stream: mpsc::Receiver<near_indexer::StreamerMessage>,
    contract_address: String,
    send: mpsc::Sender<Tx>,
) -> Result<()> {
    tracing::info!("Listening for blocks");
    while let Some(message) = stream.recv().await {
        tracing::debug!("New block at {:?}", message.block.header.height);

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
                        tracing::trace!("Skipping tx with wrong receiver");
                        continue;
                    }

                    for action in t.transaction.actions {
                        if let ActionView::FunctionCall {
                            method_name, args, ..
                        } = action
                        {
                            if method_name != "transact" {
                                tracing::trace!("Skipping tx with wrong method name");
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

                            send.send(tx).await?;
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
