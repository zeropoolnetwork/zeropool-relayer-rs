use zeropool_indexer_tx_storage::STORAGE_NAME;

use crate::backend::BACKEND_NAME;

#[derive(Debug, Clone)]
pub struct Config {
    pub backend: crate::backend::Config,
    pub storage: zeropool_indexer_tx_storage::Config,
}

impl Config {
    pub fn init() -> Self {
        Config {
            backend: envy::prefixed(format!("{}_", BACKEND_NAME))
                .from_env()
                .unwrap(),
            storage: envy::prefixed(format!("{}_", STORAGE_NAME))
                .from_env()
                .unwrap(),
        }
    }
}
