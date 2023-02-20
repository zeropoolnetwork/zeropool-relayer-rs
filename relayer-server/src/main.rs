use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use libzeropool_rs::libzeropool::{
    fawkes_crypto::backend::bellman_groth16::engines::Bn256,
    native::params::{PoolBN256, PoolParams as PoolParamsTrait},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::instrument;
use uuid::Uuid;

use crate::{
    config::*,
    job_queue::{JobQueue, JobStatus},
    state::AppState,
    tx::{TxDataRequest, TxValidationError},
    tx_storage::TxStorage,
    worker::*,
};

pub type PoolParams = PoolBN256;
pub type Fr = <PoolParams as PoolParamsTrait>::Fr;
pub type Fs = <PoolParams as PoolParamsTrait>::Fs;
pub type Engine = Bn256;

mod backend;
mod config;
mod job_queue;
mod merkle_tree;
mod state;
mod tx;
mod tx_storage;
mod worker;

#[derive(Deserialize)]
pub struct TxPaginationQuery {
    pub offset: Option<u32>,
    pub limit: Option<u32>,
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    let config = Config::init().unwrap();

    let backend = match config.backend {
        #[cfg(feature = "evm_backend")]
        BackendKind::Evm(evm_config) => Arc::new(backend::evm::EvmBackend::new(evm_config)),
        _ => todo!("Backend unimplemented"),
    };

    tracing::info!("{config:#?}");

    let tx_storage = TxStorage::open("transactions.persy").unwrap();
    let job_queue = Arc::new(WorkerJobQueue::new(&config.redis_url).unwrap());
    let ctx = Arc::new(AppState::new(tx_storage, job_queue.clone(), backend));
    let worker_handle = job_queue.start(ctx.clone(), worker::process_job).unwrap();

    let app = Router::new()
        .route(
            "/transactions",
            get(get_transactions).post(create_transaction),
        )
        .route("/job/:id", get(job))
        .route("/info", get(info))
        .with_state(ctx);

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));

    tracing::info!("Starting server on {addr}");

    let server_handle = axum::Server::bind(&addr).serve(app.into_make_service());

    tokio::select! {
        _ = server_handle => {}
        _ = worker_handle => {}
    }
}

async fn create_transaction(
    Json(tx_data): Json<TxDataRequest>,
    State(state): State<Arc<AppState>>,
) -> AppResult<Uuid> {
    let raw_tx = tx_data.parse()?;
    let tx = state.backend.parse_tx(raw_tx)?;
    let job_id = state.job_queue.push(tx);

    Ok(job_id)
}

async fn get_transactions(
    State(state): State<Arc<AppState>>,
    Query(pagination): Query<TxPaginationQuery>,
) -> impl IntoResponse {
    let limit = pagination.limit.unwrap_or(100);
    let offset = pagination.offset.unwrap_or(0);

    let txs = state
        .tx_storage
        .iter_range(offset..(offset + limit * 128))
        .map(|(_, data)| data)
        .collect();

    Json(txs)
}

async fn job(Path(id): Path<Uuid>, State(state): State<Arc<AppState>>) -> AppResult<JobStatus> {
    let status = state.job_queue.job_status(id).await?;

    let Some(status) = status else {
        return Err(AppError::NotFound);
    };

    Ok(status)
}

#[instrument]
async fn info() -> impl IntoResponse {
    #[derive(Serialize)]
    struct InfoResponse {
        name: String,
        version: String,
    }

    Json(InfoResponse {
        name: "relayer".to_string(),
        version: "0.1.0".to_string(),
    })
}

type AppResult<T> = Result<T, AppError>;

enum AppError {
    NotFound,
    TxValidationError(TxValidationError),
    InternalServerError(anyhow::Error),
}

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self::InternalServerError(err.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            Self::NotFound => StatusCode::NOT_FOUND.into_response(),
            Self::TxValidationError(err) => {
                tracing::warn!("Tx validation error: {err}");
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": err.to_string(),
                    })),
                )
                    .into_response()
            }
            Self::InternalServerError(err) => {
                tracing::warn!("Internal server error: {err}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": err.to_string(),
                    })),
                )
                    .into_response()
            }
        }
    }
}
