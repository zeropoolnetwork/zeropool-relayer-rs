use std::sync::Arc;

use crate::{
    backend::BlockchainBackend,
    job_queue::JobQueue,
    tx_storage::TxStorage,
    worker::{Payload, WorkerJobQueue},
};

pub struct AppState {
    pub tx_storage: TxStorage,
    pub job_queue: Arc<JobQueue<Payload, AppState>>,
    pub backend: Arc<dyn BlockchainBackend>,
}

impl AppState {
    pub fn new(
        tx_storage: TxStorage,
        job_queue: Arc<WorkerJobQueue>,
        backend: Arc<dyn BlockchainBackend>,
    ) -> Self {
        Self {
            tx_storage,
            job_queue,
            backend,
        }
    }
}
