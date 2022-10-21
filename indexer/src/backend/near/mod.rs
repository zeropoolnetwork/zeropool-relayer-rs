#[cfg(feature = "near-indexer-global")]
mod global_indexer;
#[cfg(feature = "near-indexer-framework")]
mod indexer_framework;

#[cfg(feature = "near-indexer-global")]
pub use self::global_indexer::*;
#[cfg(feature = "near-indexer-framework")]
pub use self::indexer_framework::*;

pub const BACKEND_NAME: &str = "NEAR";
