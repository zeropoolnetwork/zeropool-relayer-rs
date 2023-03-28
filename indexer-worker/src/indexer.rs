use std::sync::Arc;

use anyhow::Result;
use tokio::{sync::mpsc, task::JoinHandle};
use zeropool_indexer_tx_storage::Storage;

use crate::{
    backend::{self, Backend, BackendMethods},
    config::{BackendKind, Config},
};

pub async fn start_indexer(
    config: Config,
) -> Result<(Arc<Storage>, JoinHandle<Result<()>>, JoinHandle<Result<()>>)> {
    let storage = Arc::new(Storage::open(config.storage).await?);

    let latest_tx = storage.latest_tx().await?;
    let (send, mut recv) = mpsc::channel(100);

    let indexer_worker = match config.backend {
        #[cfg(feature = "evm")]
        BackendKind::Evm(evm_config) => {
            backend::evm::EvmBackend::new(evm_config, latest_tx)?
                .start(send)
                .await?
        }
        #[cfg(feature = "near-lake-framework")]
        BackendKind::NearLakeFramework(near_config) => {
            backend::near::lake_framework::NearLakeFrameworkBackend::new(near_config, latest_tx)?
                .start(send)
                .await?
        }
        #[cfg(feature = "near-indexer-explorer")]
        BackendKind::NearIndexerExplorer(near_config) => {
            backend::near::explorer_indexer::NearExplorerBackend::new(near_config, latest_tx)?
                .start(send)
                .await?
        }
        #[cfg(feature = "near-indexer-framework")]
        BackendKind::NearIndexerFramework(near_config) => {
            backend::near::indexer_framework::NearIndexerFrameworkBackend::new(
                near_config,
                latest_tx,
            )?
            .start(send)
            .await?
        }
    };

    let db = storage.clone();
    let storage_worker = tokio::spawn(async move {
        tracing::info!("Storage worker listening for new transactions");

        while let Some(tx) = recv.recv().await {
            tracing::info!("Storing new transaction {}", tx.hash);
            if let Err(e) = db.store_tx(tx).await {
                tracing::error!("Failed to store transaction: {e}");
            }
        }

        Err(anyhow::anyhow!("Storage worker stopped"))
    });

    Ok((storage, indexer_worker, storage_worker))
}
