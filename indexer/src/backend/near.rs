use anyhow::Result;
use near_indexer::{
    near_primitives::views::{ActionView, ExecutionStatusView},
    InitConfigArgs,
};
use serde::Deserialize;
// use sqlx::{postgres::PgPoolOptions, types::BigDecimal, PgPool};
use tokio::sync::mpsc;

use crate::tx::Tx;

pub const BACKEND_NAME: &str = "NEAR";

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub contract_address: String,
    pub chain_id: String,
    pub indexer_url: String,
    pub indexer_start_height: Option<u64>,
}

pub async fn start(
    backend_config: Config,
    starting_block_height: Option<u64>,
    send: mpsc::Sender<Tx>,
) -> Result<()> {
    tracing::info!("Starting indexer");

    let mut home_dir = std::env::current_dir()?;
    home_dir.push(".near");

    let init_config = InitConfigArgs {
        chain_id: Some(backend_config.chain_id.clone()),
        account_id: None,
        test_seed: None,
        num_shards: 0,
        fast: false,
        genesis: None,
        download_genesis: false,
        download_genesis_url: None,
        download_config: false,
        download_config_url: None,
        boot_nodes: None,
        max_gas_burnt_view: None,
    };

    tracing::info!("Initializing near state");
    let home_dir_clone = home_dir.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = near_indexer::indexer_init_configs(&home_dir_clone, init_config) {
            tracing::warn!("Failed to initialize near state: {e}");
        }
    })
    .await?;
    tracing::info!("Near state initialized");

    let sync_mode = if let Some(starting_block_height) = starting_block_height {
        near_indexer::SyncModeEnum::BlockHeight(starting_block_height)
    } else {
        near_indexer::SyncModeEnum::LatestSynced
    };

    let indexer_config = near_indexer::IndexerConfig {
        home_dir: home_dir.clone(),
        sync_mode,
        await_for_node_synced: near_indexer::AwaitForNodeSyncedEnum::WaitForFullSync,
    };

    let indexer = near_indexer::Indexer::new(indexer_config)?;
    let stream = indexer.streamer();

    tokio::spawn(listen_blocks(stream, backend_config.contract_address, send));

    Ok(())
}

async fn listen_blocks(
    mut stream: mpsc::Receiver<near_indexer::StreamerMessage>,
    contract_address: String,
    send: mpsc::Sender<Tx>,
) {
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

                            send.send(tx)
                                .await
                                .expect("Failed to send tx to the channel");
                        }
                    }
                }
            }
        }
    }
}

// // Using block timestamp instead of block height to avoid an extra join
// /// Used for pre-initializing the database.
// pub async fn fetch_transactions(&self, from_block_timestamp: u64) -> Result<Vec<Tx>> {
//     let pg = PgPoolOptions::new()
//         .max_connections(1)
//         .connect(&config.near_indexer_url)
//         .await?;
//     #[derive(sqlx::FromRow)]
//     struct Record {
//         transaction_hash: String,
//         block_timestamp: BigDecimal,
//         included_in_block_hash: String,
//         signer_account_id: String,
//         receiver_account_id: String,
//         signature: String,
//         args: JsonValue,
//         block_height: BigDecimal,
//     }
//
//     let recs = sqlx::query_as::<_, Record>("
//         SELECT tx.transaction_hash, tx.block_timestamp, tx.signer_account_id,
//                tx.receiver_account_id, tx.signature, tx.included_in_block_hash,
//                b.block_height,
//                a.args
//         FROM transactions AS tx
//         JOIN transaction_actions AS a ON tx.transaction_hash = a.transaction_hash
//         JOIN blocks AS b ON tx.included_in_block_hash = b.block_hash
//         WHERE tx.receiver_account_id = ? AND a.action_kind = 'FUNCTION_CALL' AND tx.block_timestamp > ?
//         ORDER BY tx.block_timestamp ASC
//     ",)
//         .bind(&self.pool_address)
//         .bind(from_block_timestamp as i64)
//         .fetch_all(&self.pg)
//         .await?;
//
//     let mut txs = Vec::new();
//
//     for rec in recs {
//         if rec.args["method_name"] == "transact" {
//             let args = rec.args["args_base64"]
//                 .as_str()
//                 .ok_or_else(|| anyhow::anyhow!("args_base64 is missing"))?;
//             let calldata = base64::decode(args)?;
//
//             let tx = Tx {
//                 hash: rec.transaction_hash,
//                 block_hash: rec.included_in_block_hash,
//                 block_height: rec.block_height.to_u64().unwrap(),
//                 timestamp: rec.block_timestamp.to_u64().unwrap(),
//                 sender_address: rec.signer_account_id,
//                 receiver_address: rec.receiver_account_id,
//                 signature: rec.signature,
//                 calldata,
//             };
//
//             txs.push(tx);
//         }
//     }
//
//     Ok(txs)
// }
