use std::sync::Arc;

use anyhow::Result;
use fawkes_crypto::{
    backend::bellman_groth16::verifier::VK,
    ff_uint::{PrimeField, Uint},
};
use libzeropool_rs::libzeropool::fawkes_crypto::backend::bellman_groth16::Parameters;
use tokio::sync::RwLock;

use crate::{
    backend::BlockchainBackend,
    config::{BackendKind, Config},
    indexer::IndexerApi,
    job_queue::JobQueue,
    merkle_tree::MerkleTree,
    tx_storage::TxStorage,
    worker::{Payload, WorkerJobQueue},
    Engine,
};

const TX_INDEX_STRIDE: usize = libzeropool_rs::libzeropool::constants::OUT + 1;

pub struct AppState {
    pub tx_storage: TxStorage,
    pub job_queue: JobQueue<Payload, AppState>,
    pub backend: Arc<dyn BlockchainBackend>,
    pub tree: MerkleTree,
    pub pool_index: RwLock<u64>,
    pub indexer: IndexerApi,
    pub fee: u64,
    pub transfer_vk: VK<Engine>,
    pub tree_vk: VK<Engine>,
    pub tree_params: Parameters<Engine>,
}

impl AppState {
    pub async fn init(config: Config) -> Result<Self> {
        let backend = match config.backend {
            #[cfg(feature = "evm_backend")]
            BackendKind::Evm(evm_config) => {
                Arc::new(crate::backend::evm::EvmBackend::new(evm_config).unwrap())
            }
            _ => todo!("Backend unimplemented"),
        };

        let job_queue = WorkerJobQueue::new(&config.redis_url)?;
        let tx_storage = TxStorage::open("transactions.persy")?;
        let tree = MerkleTree::open("tree.persy")?;
        let pool_index = backend.get_pool_index().await?;
        let indexer = IndexerApi::new(config.indexer_url);
        let relayer_num_leaves = tree.num_leaves();
        let relayer_index = relayer_num_leaves * TX_INDEX_STRIDE as u64;

        // TODO: Optimize
        //     - Fetch only new transactions
        //     - Memory usage
        tracing::info!("Fetching all transactions from indexer");
        let all_txs = indexer.fetch_all().await?;
        for (i, tx) in all_txs.into_iter().enumerate() {
            let tx_index = i * TX_INDEX_STRIDE;
            if tx_index < relayer_index as usize {
                tracing::info!("Skipping tx {}", tx_index);
                continue;
            }

            let tx_data = backend.parse_calldata(tx.calldata)?;
            let tx_hash = backend.parse_hash(&tx.hash)?;

            tx_storage.set(tx_index as u32, tx_data.out_commit, &tx_hash, &tx_data.memo)?;
            tree.set_leaf(relayer_num_leaves + i as u64, tx_data.out_commit)?;
        }

        let transfer_vk = std::fs::read_to_string("params/transfer_verification_key.json")?;
        let transfer_vk: VK<_> = serde_json::from_str(&transfer_vk)?;

        let tree_vk = std::fs::read_to_string("params/tree_update_verification_key.json")?;
        let tree_vk: VK<_> = serde_json::from_str(&tree_vk)?;

        let tree_params_data = std::fs::read("params/tree_update_params.bin")?;
        let tree_params = Parameters::read(&mut tree_params_data.as_slice(), true, true)?;

        Ok(Self {
            tx_storage,
            job_queue,
            backend,
            tree,
            indexer,
            pool_index: RwLock::new(pool_index),
            fee: config.fee,
            transfer_vk,
            tree_vk,
            tree_params,
        })
    }

    pub async fn get_pool_index(&self) -> u64 {
        *self.pool_index.read().await
    }

    pub fn get_optimistic_index(&self) -> u64 {
        self.tree.num_leaves() * TX_INDEX_STRIDE as u64
    }
}
