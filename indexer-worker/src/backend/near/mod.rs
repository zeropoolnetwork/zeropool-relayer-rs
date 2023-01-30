use zeropool_indexer_tx_storage::Tx;

#[cfg(feature = "near-indexer-explorer")]
pub use self::explorer_indexer::*;
#[cfg(feature = "near-indexer-framework")]
pub use self::indexer_framework::*;
#[cfg(feature = "near-lake-framework")]
pub use self::lake_framework::*;

#[cfg(feature = "near-indexer-explorer")]
mod explorer_indexer;
#[cfg(feature = "near-indexer-framework")]
mod indexer_framework;
#[cfg(feature = "near-lake-framework")]
mod lake_framework;

pub const BACKEND_NAME: &str = "NEAR";

pub fn block_id(tx: &Tx) -> BlockId {
    tx.block_height
}
