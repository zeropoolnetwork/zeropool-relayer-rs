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
};

pub type PoolParams = PoolBN256;
pub type Fr = <PoolParams as PoolParamsTrait>::Fr;
pub type Fs = <PoolParams as PoolParamsTrait>::Fs;
pub type Engine = Bn256;

mod backend;
mod config;
mod indexer;
mod job_queue;
mod json_api;
mod merkle_tree;
mod state;
mod tx;
mod tx_storage;
mod worker;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    let config = Config::init().unwrap();
    tracing::info!("{config:#?}");

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));

    let ctx = Arc::new(AppState::init(config).await.unwrap());
    let worker_handle = ctx
        .job_queue
        .start(ctx.clone(), worker::process_job)
        .unwrap();

    tracing::info!("Starting server on {addr}");

    let routes = json_api::routes(ctx);
    let server_handle = axum::Server::bind(&addr).serve(routes.into_make_service());

    tokio::select! {
        err = server_handle => {
            tracing::error!("JSON API critical error: {err:?}");
        }
        err = worker_handle => {
            tracing::error!("Worker critical error: {err:?}");
        }
    }
}
