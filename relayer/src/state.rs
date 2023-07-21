use std::sync::Arc;

use anyhow::Result;
use fawkes_crypto::backend::bellman_groth16::verifier::VK;
use libzeropool_rs::libzeropool::fawkes_crypto::backend::bellman_groth16::Parameters;
use tokio::sync::RwLock;

use crate::{
    backend::BlockchainBackend,
    config::{BackendKind, Config},
    job_queue::JobQueue,
    merkle_tree::MerkleTree,
    tx_storage::TxStorage,
    worker::{Payload, WorkerJobQueue},
    Engine,
};

const TX_INDEX_STRIDE: usize = libzeropool_rs::libzeropool::constants::OUT + 1;

pub struct AppState {
    pub config: Config,
    pub transactions: TxStorage,
    pub tree: RwLock<MerkleTree>,
    pub job_queue: JobQueue<Payload, AppState>,
    pub backend: Arc<dyn BlockchainBackend>,
    pub pool_index: RwLock<u64>,
    pub fee: u64,
    pub transfer_vk: VK<Engine>,
    pub tree_vk: VK<Engine>,
    pub tree_params: Parameters<Engine>,
}

impl AppState {
    pub async fn init(config: Config) -> Result<Self> {
        let backend: Arc<dyn BlockchainBackend> = match config.backend.clone() {
            BackendKind::Mock => Arc::new(crate::backend::mock::MockBackend::new()),
            #[cfg(feature = "evm_backend")]
            BackendKind::Evm(config) => Arc::new(crate::backend::evm::EvmBackend::new(config)?),
            #[cfg(feature = "near_backend")]
            BackendKind::Near(config) => Arc::new(crate::backend::near::NearBackend::new(config)?),
            _ => todo!("Backend unimplemented"),
        };

        let job_queue = WorkerJobQueue::new(&config.redis_url)?;
        let mut transactions = TxStorage::open("transactions.persy")?;
        let mut tree = MerkleTree::open("tree.persy")?;
        let pool_index = backend.get_pool_index().await?;
        let relayer_num_leaves = tree.num_leaves();
        let relayer_index = relayer_num_leaves * TX_INDEX_STRIDE as u64;
        let fee = config.fee;

        // TODO: More criteria for state corruption.
        let is_state_corrupted = relayer_index > pool_index;

        if is_state_corrupted {
            tracing::error!("Relayer state is corrupted. Reinitializing...");

            transactions = TxStorage::clear_and_open("transactions.persy")?;
            tree = MerkleTree::clear_and_open("tree.persy")?;
        }

        if relayer_index < pool_index {
            tracing::info!("Fetching transactions...");
            let all_txs = backend.init_state().await?;
            tracing::info!(
                "Fetched {} transactions, initializing state...",
                all_txs.len()
            );

            for (i, tx) in all_txs.into_iter().enumerate() {
                let tx_index = i * TX_INDEX_STRIDE;
                if tx_index < relayer_index as usize {
                    tracing::info!("Skipping tx {}", tx_index);
                    continue;
                }

                let tx_data = backend.parse_calldata(tx.calldata)?;
                let tx_hash = tx.hash;

                tree.add_leaf(tx_data.out_commit)?;
                transactions.set(tx_index as u64, tx_data.out_commit, &tx_hash, &tx_data.memo)?;
            }
        }

        let transfer_vk = std::fs::read_to_string("params/transfer_verification_key.json")?;
        let transfer_vk: VK<_> = serde_json::from_str(&transfer_vk)?;

        let tree_vk = std::fs::read_to_string("params/tree_update_verification_key.json")?;
        let tree_vk: VK<_> = serde_json::from_str(&tree_vk)?;

        let tree_params_data = std::fs::read("params/tree_update_params.bin")?;
        let tree_params = Parameters::read(&mut tree_params_data.as_slice(), true, true)?;

        Ok(Self {
            config,
            transactions,
            job_queue,
            backend,
            tree: RwLock::new(tree),
            pool_index: RwLock::new(pool_index),
            fee,
            transfer_vk,
            tree_vk,
            tree_params,
        })
    }

    pub async fn get_pool_index(&self) -> u64 {
        *self.pool_index.read().await
    }
}
