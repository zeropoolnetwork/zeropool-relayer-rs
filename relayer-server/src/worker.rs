use std::sync::Arc;

use anyhow::Result;
use fawkes_crypto::backend::bellman_groth16::{
    group::{G1Point, G2Point},
    prover::Proof,
};
use libzeropool_rs::{
    libzeropool::{
        constants,
        fawkes_crypto::ff_uint::{Num, Uint},
        native::{
            tree::{TreePub, TreeSec},
            tx::parse_delta,
        },
        POOL_PARAMS,
    },
    proof::prove_tree,
};
use serde::{Deserialize, Serialize};

use crate::{
    backend::BlockchainBackend,
    job_queue::{Job, JobQueue},
    merkle_tree::MerkleTree,
    state::AppState,
    tx::{FullTxData, ParsedTxData},
    tx_storage::{self, TxStorage},
    Fr,
};

pub type Payload = ParsedTxData;

pub type WorkerJobQueue = JobQueue<Payload, AppState>;

#[derive(Serialize, Deserialize)]
pub struct TxMeta {
    tx_hash: Vec<u8>,
}

// TODO: Thoroughly check for race conditions. This might be a mine field, considering that
//       process_job runs in parallel.
#[tracing::instrument(skip_all, fields(job_id = %job.id))]
pub async fn process_job(job: Job<Payload>, ctx: Arc<AppState>) -> Result<()> {
    const OUTPLUSONE: u64 = constants::OUT as u64 + 1;

    let tx = job.data;
    let tree = ctx.tree.write().await;
    let root_before = tree.root()?;
    let next_commit_index = tree.num_leaves() * OUTPLUSONE;
    let prev_commit_index = next_commit_index.saturating_sub(OUTPLUSONE);

    // Update the tree and tx storage
    tree.add_leaf(tx.out_commit)?;

    ctx.tx_storage.set(
        *ctx.pool_index.read().await,
        tx.out_commit,
        &vec![0; 32],
        &tx.memo,
    )?;

    let root_after = tree.root()?;
    let proof_filled = tree.zp_merkle_proof(prev_commit_index)?;
    let proof_free = tree.zp_merkle_proof(next_commit_index)?;

    let prev_leaf = tree
        .get_node(constants::OUTPLUSONELOG as u64, prev_commit_index)?
        .ok_or_else(|| anyhow::anyhow!("No previous leaf"))?;

    // Let the other tasks start
    drop(tree);

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

    let root_after = tree_pub.root_after;

    let tree_proof = if ctx.config.mock_prover {
        tracing::debug!("Mocking tree proof");
        Proof {
            a: G1Point(Num::ZERO, Num::ZERO),
            b: G2Point((Num::ZERO, Num::ZERO), (Num::ZERO, Num::ZERO)),
            c: G1Point(Num::ZERO, Num::ZERO),
        }
    } else {
        tracing::debug!("Proving tree");
        prove_tree(&ctx.tree_params, &*POOL_PARAMS, tree_pub, tree_sec).1
    };

    let full_tx = FullTxData {
        tx_type: tx.tx_type,
        delta: tx.delta,
        out_commit: tx.out_commit,
        nullifier: tx.nullifier,
        memo: tx.memo.clone(),
        root_after,
        tree_proof,
        proof: tx.proof,
        extra_data: tx.extra_data,
    };

    tracing::info!("Sending tx");
    let tx_hash = match ctx.backend.send_tx(full_tx).await {
        Ok(tx_hash) => tx_hash,
        Err(e) => {
            tracing::error!("Failed to send tx: {:#?}", e);

            // TODO: Rollback to prev_commit_index or pool_index? pool_index might be easier.
            // let pool_index = *ctx.pool_index.read().await;

            tracing::debug!("Rolling back tx storage to {prev_commit_index}");
            ctx.tx_storage.rollback(prev_commit_index)?;
            ctx.tree.write().await.rollback(prev_commit_index)?;
            tracing::debug!("Rollback complete");

            return Err(e);
        }
    };

    ctx.tx_storage.set(
        *ctx.pool_index.read().await,
        tx.out_commit,
        &tx_hash,
        &tx.memo,
    )?;

    tracing::info!("Sent tx with hash: {tx_hash:#?}");

    ctx.job_queue.set_extra(job.id, TxMeta { tx_hash }).await?;
    *ctx.pool_index.write().await = next_commit_index;

    Ok(())
}
