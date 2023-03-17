use std::str::FromStr;

use anyhow::Result;
use serde::{de::DeserializeOwned, Deserialize};

#[derive(Debug, Clone)]
pub enum BackendKind {
    #[cfg(feature = "evm_backend")]
    Evm(crate::backend::evm::Config),
    #[cfg(feature = "near")]
    Near(crate::backend::near::Config),
    #[cfg(feature = "waves")]
    Waves(crate::backend::waves::Config),
}

#[derive(Debug)]
pub struct Config {
    pub port: u16,
    pub backend: BackendKind,
    pub redis_url: String,
    pub indexer_url: String,
    pub fee: u64,
}

impl Config {
    pub fn init() -> Result<Self> {
        let backend_name = std::env::var("BACKEND")?;

        let backend = match backend_name.as_str() {
            #[cfg(feature = "evm_backend")]
            "evm" => BackendKind::Evm(prefixed_config("EVM")?),
            #[cfg(feature = "near_backend")]
            "near" => BackendKind::Near(prefixed_config("NEAR")?),
            #[cfg(feature = "waves_backend")]
            "waves" => BackendKind::Near(prefixed_config("NEAR")?),
            _ => panic!("Unknown backend: {backend_name}"),
        };

        Ok(Config {
            port: std::env::var("PORT")?.parse()?,
            redis_url: std::env::var("REDIS_URL")?,
            indexer_url: std::env::var("INDEXER_URL")?,
            fee: std::env::var("FEE")?.parse()?,
            backend,
        })
    }
}

fn prefixed_config<T: DeserializeOwned>(prefix: &str) -> Result<T> {
    Ok(envy::prefixed(format!("{prefix}_")).from_env()?)
}
