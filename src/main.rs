use std::{net::SocketAddr, sync::Arc};

use libzeropool_rs::libzeropool::{
    fawkes_crypto::backend::bellman_groth16::engines::Bn256,
    native::params::{PoolBN256, PoolParams as PoolParamsTrait},
};

use crate::{config::*, state::AppState};

pub type PoolParams = PoolBN256;
pub type Fr = <PoolParams as PoolParamsTrait>::Fr;
pub type Fs = <PoolParams as PoolParamsTrait>::Fs;
pub type Engine = Bn256;

mod backend;
mod config;
mod job_queue;
mod json_api;
mod merkle_tree;
mod state;
mod tx;
mod tx_storage;
mod tx_worker;

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
        .start(
            ctx.clone(),
            tx_worker::process_job,
            tx_worker::process_failure,
        )
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
