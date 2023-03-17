use std::sync::Arc;

use anyhow::Result;
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

// TODO: Check for race conditions. At the moment, there should
pub async fn process_job(job: Job<Payload>, ctx: Arc<AppState>) -> Result<()> {
    let tx = job.data;

    // let (_, _, index, _) = parse_delta(tx.delta);

    let root_before = ctx.tree.root()?;

    const OUTPLUSONE: u64 = constants::OUT as u64 + 1;

    let next_commit_index = ctx.get_optimistic_index();
    let prev_commit_index = next_commit_index.saturating_sub(1);

    // Update the tree and tx storage
    ctx.tree.add_leaf(tx.out_commit)?;

    ctx.tx_storage.set(
        *ctx.pool_index.read().await as u32,
        tx.out_commit,
        &vec![0; 32],
        &tx.memo,
    )?;

    let root_after = ctx.tree.root()?;
    // TODO: Implement get_virtual_node
    // let root_after = tree.get_virtual_node(...)?;

    let proof_filled = ctx.tree.zp_merkle_proof(prev_commit_index)?;
    let proof_free = ctx.tree.zp_merkle_proof(next_commit_index)?;

    let prev_leaf = ctx
        .tree
        .get_node(constants::OUTPLUSONELOG as u64, prev_commit_index)?
        .ok_or_else(|| anyhow::anyhow!("No previous leaf"))?;

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
    let (_, tree_proof) = prove_tree(&ctx.tree_params, &*POOL_PARAMS, tree_pub, tree_sec);

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

    let tx_hash = ctx.backend.send_tx(full_tx).await?;
    ctx.tx_storage.set(
        *ctx.pool_index.read().await as u32,
        tx.out_commit,
        &tx_hash,
        &tx.memo,
    )?;

    tracing::info!("Sent tx with hash: {tx_hash:#?}");

    ctx.job_queue.set_extra(job.id, TxMeta { tx_hash }).await?;

    Ok(())
}
