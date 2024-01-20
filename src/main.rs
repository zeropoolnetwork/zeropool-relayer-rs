use std::{net::SocketAddr, sync::Arc};

#[cfg(feature = "groth16")]
use libzeropool_rs::libzeropool::fawkes_crypto::backend::bellman_groth16::{
    engines::Bn256, prover::Proof as Groth16Proof, verifier::VK as VerifyingKey,
    Parameters as Groth16Parameters,
};
#[cfg(feature = "plonk")]
use libzeropool_rs::libzeropool::fawkes_crypto::backend::plonk::{
    engines::Bn256, prover::Proof as PlonkProof, setup::VerifyingKey, Parameters as PlonkParameters,
};
use libzeropool_rs::libzeropool::native::params::{PoolBN256, PoolParams as PoolParamsTrait};

use crate::{config::*, state::AppState};

pub type PoolParams = PoolBN256;
pub type Fr = <PoolParams as PoolParamsTrait>::Fr;
pub type Fs = <PoolParams as PoolParamsTrait>::Fs;
pub type Engine = Bn256;
#[cfg(feature = "groth16")]
pub type Proof = Groth16Proof<Engine>;
#[cfg(feature = "plonk")]
pub type Proof = PlonkProof;
pub type VK = VerifyingKey<Bn256>;
#[cfg(feature = "groth16")]
pub type Parameters = Groth16Parameters<Engine>;
#[cfg(feature = "plonk")]
pub type Parameters = PlonkParameters<Engine>;

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

    let config = Config::init().expect("Failed to load config");
    tracing::info!("{config:#?}");

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));

    let ctx = Arc::new(
        AppState::init(config)
            .await
            .expect("Failed to initialize app state"),
    );

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
