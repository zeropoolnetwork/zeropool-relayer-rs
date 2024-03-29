use anyhow::Result;
use serde::de::DeserializeOwned;

#[derive(Debug, Clone)]
pub enum BackendKind {
    Mock,
    #[cfg(feature = "evm_backend")]
    Evm(crate::backend::evm::Config),
    #[cfg(feature = "near_backend")]
    Near(crate::backend::near::Config),
    #[cfg(feature = "waves_backend")]
    Waves(crate::backend::waves::Config),
}

impl BackendKind {
    pub fn token_id(&self) -> String {
        match self {
            BackendKind::Mock => "mock".to_string(),
            #[cfg(feature = "evm_backend")]
            BackendKind::Evm(config) => config.token_address.clone(),
            #[cfg(feature = "near_backend")]
            BackendKind::Near(config) => config.token_id.to_string().clone(),
            #[cfg(feature = "waves_backend")]
            BackendKind::Waves(_config) => String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub backend: BackendKind,
    pub redis_url: String,
    pub fee: u64,
    pub mock_prover: bool,
}

impl Config {
    pub fn init() -> Result<Self> {
        let backend_name = std::env::var("BACKEND")?;

        let backend = match backend_name.as_str() {
            "mock" => BackendKind::Mock,
            #[cfg(feature = "evm_backend")]
            "evm" => BackendKind::Evm(prefixed_config("EVM")?),
            #[cfg(feature = "near_backend")]
            "near" => BackendKind::Near(prefixed_config("NEAR")?),
            #[cfg(feature = "waves_backend")]
            "waves" => BackendKind::Waves(prefixed_config("WAVES")?),
            _ => panic!("Unknown backend: {backend_name}"),
        };

        Ok(Config {
            port: std::env::var("PORT")?.parse()?,
            redis_url: std::env::var("REDIS_URL")?,
            fee: std::env::var("FEE")?.parse()?,
            mock_prover: std::env::var("MOCK_PROVER")
                .map(|var| var.parse::<bool>())
                .unwrap_or(Ok(false))?,
            backend,
        })
    }
}

fn prefixed_config<T: DeserializeOwned>(prefix: &str) -> Result<T> {
    Ok(envy::prefixed(format!("{prefix}_")).from_env()?)
}
