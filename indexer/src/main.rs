use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::{Extension, Path, Query},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::{storage::Storage, tx::Tx};

mod backend;
mod config;
mod indexer;
mod storage;
mod tx;

type SharedDb = Arc<Storage>;

const MAX_TX_LIMIT: u64 = 100;

// TODO: Split into two separate services: indexer and api

#[cfg(not(feature = "near-indexer-framework"))]
#[tokio::main]
async fn main() {
    start().await;
}

#[cfg(feature = "near-indexer-framework")]
#[actix::main]
async fn main() {
    start().await;
}

async fn start() {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    let config = config::Config::init();

    tracing::info!("{config:#?}");

    let (storage, indexer_worker, storage_worker) =
        indexer::start_indexer(config.clone()).await.unwrap();

    let app = Router::new()
        .route("/transactions", get(get_transactions))
        .route("/transactions/:tx_hash", get(get_transaction))
        .route("/info", get(info))
        .layer(Extension(storage));

    let addr = SocketAddr::from(([0, 0, 0, 0], config.server.port));

    tracing::info!("Starting server on {addr}");
    let server = axum::Server::bind(&addr).serve(app.into_make_service());

    tokio::select! {
        res = server => {
            tracing::error!("Server stopped: {:?}", res);
        }
        res = indexer_worker => {
            tracing::error!("Indexer worker stopped: {:?}", res);
        }
        res = storage_worker => {
            tracing::error!("Storage worker exited unexpectedly: {:?}", res);
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct TxPaginationQuery {
    block_height: Option<u64>,
    timestamp: Option<u64>,
    limit: Option<u64>,
}

async fn get_transactions(
    Extension(db): Extension<SharedDb>,
    Query(p): Query<TxPaginationQuery>,
) -> AppResult<Json<Vec<Tx>>> {
    let block_height = p.block_height.unwrap_or_default();
    let timestamp = p.timestamp.unwrap_or_default();
    let limit = p
        .limit
        .map(|l| l.clamp(0, MAX_TX_LIMIT))
        .unwrap_or(MAX_TX_LIMIT);

    let txs = db.get_txs(block_height, timestamp, limit).await?;

    Ok(Json(txs))
}

async fn get_transaction(
    Extension(db): Extension<SharedDb>,
    Path(tx_hash): Path<String>,
) -> AppResult<Json<Tx>> {
    let tx = db.get_tx_by_hash(&tx_hash).await?;
    match tx {
        Some(tx) => Ok(Json(tx)),
        None => Err(AppError::NotFound),
    }
}

#[derive(Serialize)]
struct InfoResponse {
    version: String,
    num_transactions: u64,
}

async fn info(Extension(db): Extension<SharedDb>) -> AppResult<Json<InfoResponse>> {
    Ok(Json(InfoResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        num_transactions: db.count().await?,
    }))
}

type AppResult<T> = Result<T, AppError>;

enum AppError {
    NotFound,
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
