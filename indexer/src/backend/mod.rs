#[cfg(feature = "evm")]
pub mod evm;
#[cfg(feature = "near")]
pub mod near;

#[cfg(feature = "evm")]
pub use evm::*;
#[cfg(feature = "near")]
pub use near::*;

// TODO: Create a Backend trait and bundle all backends in a single binary (?)
//    Need async method in traits for that though.
