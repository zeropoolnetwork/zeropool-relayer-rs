#[cfg(feature = "postgres_storage")]
mod postgres;
#[cfg(feature = "postgres_storage")]
pub use postgres::*;

// TODO: Cassandra or mongodb might be a better fit since we don't need to do joins
