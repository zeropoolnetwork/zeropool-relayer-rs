use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{
    backend::BlockchainBackend,
    job_queue::{Job, JobQueue},
    state::AppState,
    tx::ParsedTxData,
    tx_storage::{self, TxStorage},
};

pub type Payload = ParsedTxData;

pub type WorkerJobQueue = JobQueue<Payload, AppState>;

pub async fn process_job(data: Job<Payload>, ctx: Arc<AppState>) -> Result<()> {
    Ok(())
}
