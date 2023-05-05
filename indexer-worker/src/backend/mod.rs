use anyhow::Result;
use tokio::{sync::mpsc::Sender, task::JoinHandle};
use zeropool_indexer_tx_storage::Tx;

#[cfg(feature = "evm")]
pub mod evm;
#[cfg(any(
    feature = "near-lake-framework",
    feature = "near-indexer-framework",
    feature = "near-archive-node"
))]
pub mod near;

pub trait Backend: Sized + BackendMethods {
    type Config;

    fn new(backend_config: Self::Config, latest_tx: Option<Tx>) -> Result<Self>;
}

#[async_trait::async_trait]
pub trait BackendMethods {
    async fn start(self, send: Sender<Tx>) -> Result<JoinHandle<Result<()>>>;
}
