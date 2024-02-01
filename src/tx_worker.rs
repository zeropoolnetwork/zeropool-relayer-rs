use std::sync::Arc;

use anyhow::{anyhow, Result};
#[cfg(feature = "groth16")]
use libzeropool_rs::libzeropool::fawkes_crypto::backend::bellman_groth16::{
    group::{G1Point, G2Point},
    prover::Proof,
};
#[cfg(feature = "plonk")]
use libzeropool_rs::libzeropool::fawkes_crypto::backend::plonk::prover::Proof;
use libzeropool_rs::libzeropool::{
    constants,
    native::tree::{TreePub, TreeSec},
    POOL_PARAMS,
};
#[cfg(feature = "groth16")]
use libzeropool_rs::proof_groth16::prove_tree;
#[cfg(feature = "plonk")]
use libzeropool_rs::proof_plonk::prove_tree;
use serde::{Deserialize, Serialize};
use zeropool_tx::TxData;

use crate::{
    job_queue::{Job, JobQueue},
    state::AppState,
    tx::ParsedTxData,
    Fr,
};

const TX_SIZE: u64 = constants::OUT as u64 + 1;

#[derive(Clone, Serialize, Deserialize)]
pub struct Payload {
    tx: ParsedTxData,
    tree_pub: TreePub<Fr>,
    tree_sec: TreeSec<Fr>,
    next_commit_index: u64,
    prev_commit_index: u64,
}

pub type WorkerJobQueue = JobQueue<Payload, AppState>;

/// Does as much as possible before creating a job in order to guarantee that the optimistic state
/// is updated by the time a user receives a response.
pub async fn prepare_job(tx: ParsedTxData, ctx: Arc<AppState>) -> Result<Payload> {
    let tree = ctx.tree.lock().await;
    let root_before = tree.root()?;
    let next_commit_index = tree.num_leaves();
    let prev_commit_index = next_commit_index.saturating_sub(1);

    // Modify state, if something goes wrong later, we'll rollback.
    tree.add_leaf(tx.out_commit)?;
    ctx.transactions.push(
        next_commit_index * TX_SIZE,
        tx.out_commit,
        &vec![0; 32],
        &tx.memo,
    )?;

    // Prepare the data for the prover.
    let root_after = tree.root()?;
    let proof_filled = tree.zp_merkle_proof(prev_commit_index)?;
    let proof_free = tree.zp_merkle_proof(next_commit_index)?;
    let prev_leaf = tree.leaf(prev_commit_index)?;

    let tree_pub = TreePub {
        root_before,
        root_after,
        leaf: tx.out_commit,
    };
    let tree_sec = TreeSec {
        proof_filled,
        proof_free,
        prev_leaf,
    };

    Ok(Payload {
        tx,
        tree_pub,
        tree_sec,
        next_commit_index,
        prev_commit_index,
    })
}

#[tracing::instrument(skip_all, fields(job_id = %job.id))]
pub async fn process_failure(job: Job<Payload>, ctx: Arc<AppState>) -> Result<()> {
    let prev_commit_index = job.data.prev_commit_index;

    tracing::info!("Rolling back tx storage to {prev_commit_index}");
    ctx.transactions.rollback(prev_commit_index)?;
    ctx.tree.lock().await.rollback(prev_commit_index)?;
    ctx.job_queue.cancel_jobs_after(job.id).await?;
    tracing::info!("Rollback complete");

    Ok(())
}

#[tracing::instrument(skip_all, fields(job_id = %job.id))]
pub async fn process_job(job: Job<Payload>, ctx: Arc<AppState>) -> Result<()> {
    let Payload {
        tx,
        tree_pub,
        tree_sec,
        next_commit_index,
        ..
    } = job.data;

    ctx.job_queue
        .add_job_mapping(job.id, next_commit_index)
        .await?;

    let root_after = tree_pub.root_after;

    let tree_proof = if ctx.config.mock_prover {
        tracing::debug!("Mocking tree proof");

        #[cfg(feature = "groth16")]
        {
            Proof {
                a: G1Point(Num::ZERO, Num::ZERO),
                b: G2Point((Num::ZERO, Num::ZERO), (Num::ZERO, Num::ZERO)),
                c: G1Point(Num::ZERO, Num::ZERO),
            }
        }

        #[cfg(feature = "plonk")]
        {
            Proof(vec![])
        }
    } else {
        tracing::debug!("Proving tree");

        #[cfg(feature = "groth16")]
        {
            let ctx = ctx.clone();
            let proof = tokio::task::spawn_blocking(move || {
                prove_tree(
                    &ctx.groth16_params.tree_params,
                    &*POOL_PARAMS,
                    tree_pub,
                    tree_sec,
                )
                .1
            })
            .await?;
            tracing::info!("Tree proof complete");
            proof
        }

        #[cfg(feature = "plonk")]
        {
            let ctx = ctx.clone();
            let proof = tokio::task::spawn_blocking(move || {
                prove_tree(
                    &ctx.plonk_params.params,
                    &ctx.plonk_params.tree_pk,
                    &*POOL_PARAMS,
                    tree_pub,
                    tree_sec,
                )
                .1
            })
            .await?;
            tracing::info!("Tree proof complete");
            proof
        }
    };

    let full_tx = TxData {
        tx_type: tx.tx_type,
        delta: tx.delta,
        token_id: ctx.config.backend.token_id(),
        out_commit: tx.out_commit,
        nullifier: tx.nullifier,
        proof: tx.proof,
        root_after,
        tree_proof,
        memo: tx.memo.clone(),
        extra_data: tx.extra_data,
    };

    // TODO: Proper human-readable Debug implementation for TxData
    tracing::debug!(
        r#"Transaction prepared:
    tx_type: {:?}
    delta: {}
    token_id: {}
    out_commit: {}
    nullifier: {}
    proof: ...
    root_after: {}
    tree_proof: ...
    memo: {:?}
    extra_data: {:?}
"#,
        full_tx.tx_type,
        full_tx.delta,
        full_tx.token_id,
        full_tx.out_commit,
        full_tx.nullifier,
        full_tx.root_after,
        hex::encode(&full_tx.memo),
        hex::encode(&full_tx.extra_data)
    );

    // TODO: Use a separate ordered queue for sending transactions?
    loop {
        if ctx.job_queue.is_job_cancelled(job.id).await? {
            tracing::info!("Job cancelled, skipping tx");
            return Err(anyhow!("Job cancelled"));
        }

        // Wait until the preceding transactions are executed.
        let pool_index = *ctx.pool_index.read().await;
        if pool_index == next_commit_index * TX_SIZE {
            break;
        } else {
            tracing::debug!(
                "Waiting for tx {} to be executed, current pool index is {}",
                next_commit_index * TX_SIZE,
                pool_index
            );
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    tracing::info!("Sending tx");

    let tx_hash = match ctx.backend.send_tx(full_tx).await {
        Ok(tx_hash) => tx_hash,
        Err(e) => {
            tracing::error!("Failed to send tx: {:#?}", e);
            return Err(e);
        }
    };

    tracing::info!(
        "Transaction successfully sent ({}). Updating permanent state...",
        ctx.backend.format_hash(&tx_hash)
    );

    // Update transaction with hash
    ctx.transactions.set(
        next_commit_index * TX_SIZE,
        tx.out_commit,
        &tx_hash,
        &tx.memo,
    )?;

    *ctx.pool_index.write().await += TX_SIZE;
    *ctx.pool_root.write().await = root_after.0.into();

    Ok(())
}
