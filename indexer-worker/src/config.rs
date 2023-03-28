use anyhow::Result;
use serde::de::DeserializeOwned;
use zeropool_indexer_tx_storage::STORAGE_NAME;

#[derive(Debug, Clone)]
pub enum BackendKind {
    #[cfg(feature = "evm")]
    Evm(crate::backend::evm::Config),
    #[cfg(feature = "near-indexer-explorer")]
    NearIndexerExplorer(crate::backend::near::explorer_indexer::Config),
    #[cfg(feature = "near-indexer-framework")]
    NearIndexerFramework(crate::backend::near::indexer_framework::Config),
    #[cfg(feature = "near-lake-framework")]
    NearLakeFramework(crate::backend::near::lake_framework::Config),
}

#[derive(Debug, Clone)]
pub struct Config {
    pub backend: BackendKind,
    pub storage: zeropool_indexer_tx_storage::Config,
}

impl Config {
    pub fn init() -> Result<Self> {
        let backend_name = std::env::var("BACKEND")?;

        let backend = match backend_name.as_str() {
            #[cfg(feature = "evm")]
            "evm" => BackendKind::Evm(prefixed_config("EVM")?),
            #[cfg(feature = "near-indexer-explorer")]
            "near-indexer-explorer" => BackendKind::NearIndexerExplorer(prefixed_config("NEAR")?),
            #[cfg(feature = "near-indexer-framework")]
            "near-indexer-framework" => BackendKind::NearIndexerFramework(prefixed_config("NEAR")?),
            #[cfg(feature = "near-lake-framework")]
            "near-lake-framework" => BackendKind::NearLakeFramework(prefixed_config("NEAR")?),
            _ => panic!("Unknown backend: {backend_name}"),
        };

        Ok(Config {
            backend,
            storage: prefixed_config(STORAGE_NAME)?,
        })
    }
}

fn prefixed_config<T: DeserializeOwned>(prefix: &str) -> Result<T> {
    Ok(envy::prefixed(format!("{prefix}_")).from_env()?)
}