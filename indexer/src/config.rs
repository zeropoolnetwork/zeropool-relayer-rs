use serde::Deserialize;

use crate::{backend::BACKEND_NAME, storage::STORAGE_NAME};

#[derive(Debug, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub backend: crate::backend::Config,
    pub storage: crate::storage::Config,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub port: u16,
}

impl Config {
    pub fn init() -> Self {
        Config {
            server: envy::from_env().unwrap(),
            backend: envy::prefixed(format!("{}_", BACKEND_NAME))
                .from_env()
                .unwrap(),
            storage: envy::prefixed(format!("{}_", STORAGE_NAME))
                .from_env()
                .unwrap(),
        }
    }
}
