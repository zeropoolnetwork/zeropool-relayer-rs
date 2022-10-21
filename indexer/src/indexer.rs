use std::sync::Arc;

use anyhow::Result;
use tokio::{sync::mpsc, task::JoinHandle};

use crate::{backend::start, config::Config, storage::Storage};

pub async fn start_indexer(
    config: Config,
) -> Result<(Arc<Storage>, JoinHandle<Result<()>>, JoinHandle<Result<()>>)> {
    let storage = Arc::new(Storage::open(config.storage).await?);

    let from_block_height = storage.latest_tx().await?.map(|tx| tx.block_height);
    let (send, mut recv) = mpsc::channel(100);

    let indexer_worker =
        tokio::spawn(async move { start(config.backend, from_block_height, send).await });

    let db = storage.clone();
    let storage_worker = tokio::spawn(async move {
        tracing::info!("Storage worker listening for new transactions");

        while let Some(tx) = recv.recv().await {
            tracing::debug!("Storing new transaction {}", tx.hash);
            if let Err(e) = db.store_tx(tx).await {
                tracing::error!("Failed to store transaction: {e}");
            }
        }

        Err(anyhow::anyhow!("Storage worker stopped"))
    });

    Ok((storage, indexer_worker, storage_worker))
}
