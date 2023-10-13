use std::sync::Arc;

use anyhow::anyhow;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use byteorder::{BigEndian, ReadBytesExt};
use fawkes_crypto::{backend::bellman_groth16::verifier::verify, engines::U256, ff_uint::Uint};
use libzeropool_rs::libzeropool::native::tx::parse_delta;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::trace::TraceLayer;
use zeropool_tx::TxType;

use crate::{
    job_queue::JobStatus,
    state::AppState,
    tx::{ParsedTxData, ProofWithInputs, TxValidationError},
    tx_worker::prepare_job,
};

pub fn routes(ctx: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/transactions",
            get(get_transactions).post(create_transaction),
        )
        // For compatibility with old API
        .route("/sendTransactions", post(create_transaction_legacy))
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTransactionResponse {
    pub job_id: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxDataRequest {
    pub tx_type: TxType,
    pub proof: ProofWithInputs,
    #[serde(with = "hex")]
    pub memo: Vec<u8>,
    #[serde(with = "hex")]
    pub extra_data: Vec<u8>,
    // #[serde(default)]
    // pub sync: bool,
}

async fn create_transaction(
    State(state): State<Arc<AppState>>,
    Json(tx_data): Json<TxDataRequest>,
) -> AppResult<Json<CreateTransactionResponse>> {
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

    // TODO: Modify state before creating a job
    // let job_data = prepare_job(tx);

    let payload = prepare_job(tx, state.clone()).await?;
    let job_id = state.job_queue.push(payload).await?;

    Ok(Json(CreateTransactionResponse { job_id }))
}

#[derive(Serialize, Deserialize)]
struct TxDataRequestLegacy(Vec<TxDataRequest>);

/// Legacy API compatibility
async fn create_transaction_legacy(
    state: State<Arc<AppState>>,
    Json(tx_data): Json<TxDataRequestLegacy>,
) -> AppResult<Json<CreateTransactionResponse>> {
    if tx_data.0.len() > 1 {
        return Err(AppError::BadRequest(anyhow!(
            "Can only process one transaction at a time"
        )));
    }

    let tx_data = tx_data
        .0
        .into_iter()
        .next()
        .ok_or(AppError::BadRequest(anyhow!(
            "No transaction data provided"
        )))?;

    create_transaction(state, Json(tx_data)).await
}

async fn validate_tx(tx: &TxDataRequest, state: &AppState) -> Vec<TxValidationError> {
    let mut errors = Vec::new();

    // TODO: Cache nullifiers

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

    if transfer_index.to_uint().0 > U256::from(*state.pool_index.read().await) {
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
        .transactions
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JobStatusResponse {
    state: JobStatus, // tx_hash: Option<String>,
}

async fn job(
    State(state): State<Arc<AppState>>,
    Path(id): Path<u64>,
) -> AppResult<Json<JobStatusResponse>> {
    let state = state.job_queue.job_status(id).await?;

    let Some(state) = state else {
        return Err(AppError::NotFound);
    };

    Ok(Json(JobStatusResponse { state }))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InfoResponse {
    api_version: String,
    root: String,
    optimistic_root: String,
    delta_index: String,
    optimistic_delta_index: String,
}

async fn info(State(state): State<Arc<AppState>>) -> AppResult<Json<InfoResponse>> {
    let pool_index = *state.pool_index.read().await;

    let root = state.pool_root.read().await.to_string();
    let optimistic_root = state.tree.lock().await.root()?.to_string();
    let optimistic_delta_index = state.tree.lock().await.num_leaves() * 128; // FIXME: use the constant

    Ok(Json(InfoResponse {
        api_version: "2".to_owned(),
        root,
        optimistic_root,
        delta_index: pool_index.to_string(),
        optimistic_delta_index: optimistic_delta_index.to_string(),
    }))
}

type AppResult<T> = Result<T, AppError>;

enum AppError {
    NotFound,
    BadRequest(anyhow::Error),
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
            Self::BadRequest(err) => {
                tracing::warn!("Bad request: {err}");
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
                    Json(json!({
                        "error": err.to_string(),
                    })),
                )
                    .into_response()
            }
        }
    }
}
