use std::{net::SocketAddr, sync::Arc};

use anyhow::Result;
use axum::{
    extract::{Extension, Path, Query},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use zeropool_indexer_tx_storage::{Storage, Tx, STORAGE_NAME};

type SharedDb = Arc<Storage>;

const MAX_TX_LIMIT: u64 = 100;

#[derive(Debug, Clone)]
pub struct Config {
    port: u16,
    storage: zeropool_indexer_tx_storage::Config,
}

impl Config {
    pub fn init() -> Self {
        Config {
            port: std::env::var("PORT")
                .ok()
                .and_then(|port| port.parse().ok())
                .unwrap_or(3000),
            storage: envy::prefixed(format!("{}_", STORAGE_NAME))
                .from_env()
                .unwrap(),
        }
    }
}

#[tokio::main]
async fn main() {
    start().await.unwrap();
}

async fn start() -> Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    let config = Config::init();
    tracing::info!("{config:#?}");

    let storage = Arc::new(Storage::open(config.storage).await?);

    let app = Router::new()
        .route("/transactions", get(get_transactions))
        .route("/transactions/:tx_hash", get(get_transaction))
        .route("/info", get(info))
        .layer(Extension(storage));

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));

    tracing::info!("Starting server on {addr}");
    let server = axum::Server::bind(&addr).serve(app.into_make_service());
    server.await?;

    Ok(())
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
