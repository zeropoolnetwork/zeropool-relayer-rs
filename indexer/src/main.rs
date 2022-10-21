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

#[derive(Deserialize)]
#[serde(untagged)]
pub enum TxPaginationQuery {
    Timestamp { timestamp: u64, limit: u64 },
    BlockHeight { block_height: u64, limit: u64 },
}

impl Default for TxPaginationQuery {
    fn default() -> Self {
        Self::BlockHeight {
            block_height: 0,
            limit: 100,
        }
    }
}

#[derive(Serialize)]
struct InfoResponse {
    version: String,
    num_transactions: u64,
}

// TODO: Split into two separate services: indexer and api
#[tokio::main]
async fn main() {
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

async fn get_transactions(
    Extension(db): Extension<SharedDb>,
    pagination: Option<Query<TxPaginationQuery>>,
) -> AppResult<Json<Vec<Tx>>> {
    let Query(pagination) = pagination.unwrap_or_default();
    let txs = match pagination {
        TxPaginationQuery::Timestamp { timestamp, limit } => {
            db.get_txs_by_timestamp(timestamp, limit).await?
        }
        TxPaginationQuery::BlockHeight {
            block_height,
            limit,
        } => db.get_txs_by_block_height(block_height, limit).await?,
    };

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

async fn info(Extension(db): Extension<SharedDb>) -> AppResult<Json<InfoResponse>> {
    Ok(Json(InfoResponse {
        version: "0.1.0".to_string(),
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
