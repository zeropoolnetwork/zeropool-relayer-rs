use std::{future::Future, net::SocketAddr, sync::Arc};

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, IntoMakeService},
    Json, Router, Server,
};
use byteorder::{BigEndian, ReadBytesExt};
use fawkes_crypto::{
    backend::bellman_groth16::verifier::verify,
    engines::U256,
    ff_uint::{Num, Uint},
};
use libzeropool_rs::libzeropool::{
    fawkes_crypto::backend::bellman_groth16::engines::Bn256,
    native::{
        params::{PoolBN256, PoolParams as PoolParamsTrait},
        tx::parse_delta,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::trace::TraceLayer;
use tracing::instrument;
use uuid::Uuid;

use crate::{
    config::*,
    job_queue::{JobQueue, JobStatus},
    state::AppState,
    tx::{ParsedTxData, TxDataRequest, TxType, TxValidationError},
    tx_storage::TxStorage,
    worker::*,
    Fr,
};

pub fn routes(ctx: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/transactions",
            get(get_transactions).post(create_transaction),
        )
        .route("/job/:id", get(job))
        .route("/info", get(info))
        .layer(TraceLayer::new_for_http())
        .with_state(ctx)
}

#[derive(Deserialize)]
pub struct TxPaginationQuery {
    pub offset: Option<u64>,
    pub limit: Option<u64>,
}

async fn create_transaction(
    State(state): State<Arc<AppState>>,
    Json(tx_data): Json<TxDataRequest>,
) -> AppResult<Json<Uuid>> {
    tracing::info!("Received transaction");
    let mut validation_errors = Vec::new();

    validation_errors.extend(validate_tx(&tx_data, state.as_ref()).await);

    let tx = ParsedTxData {
        tx_type: tx_data.tx_type,
        proof: tx_data.proof.proof,
        delta: tx_data.proof.inputs[3],
        out_commit: tx_data.proof.inputs[2],
        nullifier: tx_data.proof.inputs[1],
        memo: tx_data.memo,
        extra_data: tx_data.extra_data,
    };

    validation_errors.extend(state.backend.validate_tx(&tx));

    if !validation_errors.is_empty() {
        return Err(AppError::TxValidationErrors(validation_errors));
    }

    let job_id = state.job_queue.push(tx).await?;

    Ok(Json(job_id))
}

async fn validate_tx(tx: &TxDataRequest, state: &AppState) -> Vec<TxValidationError> {
    let mut errors = Vec::new();

    if !verify(&state.transfer_vk, &tx.proof.proof, &tx.proof.inputs) {
        errors.push(TxValidationError::InvalidTransferProof);
    }

    // Should at least contain fee
    if tx.memo.len() < 8 {
        errors.push(TxValidationError::EmptyMemo);
    }

    let memo_reader = &mut &tx.memo[..];
    let fee = memo_reader.read_u64::<BigEndian>().unwrap();

    if fee < state.fee {
        errors.push(TxValidationError::FeeTooLow);
    }

    let delta = tx.proof.inputs[3];
    let (token_amount, energy_amount, transfer_index, _pool_id) = parse_delta(delta);

    if transfer_index.to_uint().0 < U256::from(*state.pool_index.read().await) {
        errors.push(TxValidationError::InvalidTxIndex);
    }

    let token_amount = token_amount.to_uint().0;
    let energy_amount = energy_amount.to_uint().0;

    let is_token_amount_negative = !token_amount.unchecked_shr(255).is_zero();
    let is_token_amount_positive = !is_token_amount_negative && !token_amount.is_zero();
    let is_energy_amount_positive =
        energy_amount.unchecked_shr(255).is_zero() && !energy_amount.is_zero();

    match tx.tx_type {
        TxType::Deposit => {
            if is_token_amount_negative || energy_amount != U256::ZERO {
                errors.push(TxValidationError::InvalidValues);
            }
        }
        TxType::Transfer => {
            if token_amount != U256::ZERO || energy_amount != U256::ZERO {
                errors.push(TxValidationError::InvalidValues);
            }
        }
        TxType::Withdraw => {
            if is_token_amount_positive || is_energy_amount_positive {
                errors.push(TxValidationError::InvalidValues);
            }
        }
    }

    errors
}

#[derive(Serialize)]
struct Hex(#[serde(with = "hex")] Vec<u8>);

async fn get_transactions(
    State(state): State<Arc<AppState>>,
    Query(pagination): Query<TxPaginationQuery>,
) -> AppResult<Json<Vec<Hex>>> {
    let limit = pagination.limit.unwrap_or(100);
    let offset = pagination.offset.unwrap_or(0);
    let pool_index = *state.pool_index.read().await;

    let txs = state
        .tx_storage
        .iter_range(offset..(offset + limit * 128))?
        .map(|res| {
            res.map(|(index, data)| {
                let is_mined = (index < pool_index) as u8;
                let data = [&[is_mined], data.as_slice()].concat();

                Hex(data)
            })
        })
        .collect::<Result<_, _>>()?;

    Ok(Json(txs))
}

async fn job(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<JobStatus>> {
    let status = state.job_queue.job_status(id).await?;

    let Some(status) = status else {
        return Err(AppError::NotFound);
    };

    Ok(Json(status))
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
    TxValidationErrors(Vec<TxValidationError>),
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
            Self::TxValidationErrors(errors) => {
                tracing::warn!("Tx validation error: {errors:#?}");
                let errors = errors
                    .into_iter()
                    .map(|err| json!({ "error": err.to_string(), "code": err }))
                    .collect::<Vec<_>>();

                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "Validation error",
                        "errors": errors,
                    })),
                )
                    .into_response()
            }
            Self::InternalServerError(err) => {
                tracing::warn!("Internal server error: {err}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": err.to_string(),
                    })),
                )
                    .into_response()
            }
        }
    }
}
