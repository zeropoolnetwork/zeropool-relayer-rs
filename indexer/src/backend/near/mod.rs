#[cfg(feature = "near-indexer-global")]
mod explorer_indexer;
#[cfg(feature = "near-indexer-framework")]
mod indexer_framework;

#[cfg(feature = "near-indexer-global")]
pub use self::explorer_indexer::*;
#[cfg(feature = "near-indexer-framework")]
pub use self::indexer_framework::*;

pub const BACKEND_NAME: &str = "NEAR";
