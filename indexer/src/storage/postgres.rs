use anyhow::Result;
use serde::Deserialize;
use sqlx::{postgres::PgPoolOptions, types::time::OffsetDateTime, PgPool};

use crate::tx::Tx;

// TODO: Proper row to Tx mapping

pub const STORAGE_NAME: &str = "PG";

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    url: String,
    max_connections: u32,
}

pub struct Storage {
    pool: PgPool,
}

impl Storage {
    pub async fn open(config: Config) -> Result<Self> {
        tracing::info!("Initializing postgres connection pool");
        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .connect(&config.url)
            .await?;

        // TODO: Implement migrations
        // TODO: Store hashes in binary format?
        tracing::info!("Creating/checking transactions table");
        sqlx::query!(
            "CREATE TABLE IF NOT EXISTS transactions (
                hash TEXT PRIMARY KEY,
                block_hash TEXT NOT NULL,
                block_height BIGINT NOT NULL,
                timestamp TIMESTAMPTZ NOT NULL,
                sender_address TEXT NOT NULL,
                receiver_address TEXT NOT NULL,
                signature TEXT NOT NULL,
                calldata BYTEA NOT NULL
            )",
        )
        .execute(&pool)
        .await?;

        tracing::info!("Creating/checking transactions table indices");
        sqlx::query!(
            "CREATE INDEX IF NOT EXISTS transactions_timestamp ON transactions (timestamp)",
        )
        .execute(&pool)
        .await?;
        sqlx::query!(
            "CREATE INDEX IF NOT EXISTS transactions_block_height ON transactions (block_height)",
        )
        .execute(&pool)
        .await?;

        Ok(Self { pool })
    }

    pub async fn latest_tx(&self) -> Result<Option<Tx>> {
        let tx = sqlx::query!("SELECT * FROM transactions ORDER BY timestamp DESC LIMIT 1")
            .fetch_optional(&self.pool)
            .await?
            .map(|rec| Tx {
                hash: rec.hash,
                block_hash: rec.block_hash,
                block_height: rec.block_height as u64,
                timestamp: rec.timestamp.unix_timestamp_nanos() as u64, // Should be fine for about 200 years or so
                sender_address: rec.sender_address,
                receiver_address: rec.receiver_address,
                signature: rec.signature,
                calldata: rec.calldata,
            });

        Ok(tx)
    }

    pub async fn store_tx(&self, tx: Tx) -> Result<()> {
        tracing::info!("Storing transaction {}", tx.hash);

        let timestamp = OffsetDateTime::from_unix_timestamp_nanos(tx.timestamp as i128)?;

        sqlx::query!(
            "INSERT INTO transactions (hash, block_hash, block_height, timestamp, sender_address, receiver_address, signature, calldata)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            tx.hash,
            tx.block_hash,
            tx.block_height as i64,
            timestamp,
            tx.sender_address,
            tx.receiver_address,
            tx.signature,
            tx.calldata,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_txs_by_timestamp(&self, from_timestamp: u64, limit: u64) -> Result<Vec<Tx>> {
        let txs = sqlx::query!(
            "SELECT * FROM transactions
            WHERE timestamp >= $1
            ORDER BY timestamp ASC
            LIMIT $2",
            from_timestamp as i64,
            limit as i64,
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|rec| Tx {
            hash: rec.hash,
            block_hash: rec.block_hash,
            block_height: rec.block_height as u64,
            timestamp: rec.timestamp.unix_timestamp_nanos() as u64,
            sender_address: rec.sender_address,
            receiver_address: rec.receiver_address,
            signature: rec.signature,
            calldata: rec.calldata,
        })
        .collect();

        Ok(txs)
    }

    pub async fn get_txs_by_block_height(
        &self,
        from_block_height: u64,
        limit: u64,
    ) -> Result<Vec<Tx>> {
        let txs = sqlx::query!(
            "SELECT * FROM transactions
            WHERE block_height >= $1
            ORDER BY timestamp ASC
            LIMIT $2",
            from_block_height as i64,
            limit as i64,
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|rec| Tx {
            hash: rec.hash,
            block_hash: rec.block_hash,
            block_height: rec.block_height as u64,
            timestamp: rec.timestamp.unix_timestamp_nanos() as u64,
            sender_address: rec.sender_address,
            receiver_address: rec.receiver_address,
            signature: rec.signature,
            calldata: rec.calldata,
        })
        .collect();

        Ok(txs)
    }

    pub async fn get_tx_by_hash(&self, hash: &str) -> Result<Option<Tx>> {
        let tx = sqlx::query!("SELECT * FROM transactions WHERE hash = $1", hash)
            .fetch_optional(&self.pool)
            .await?
            .map(|rec| Tx {
                hash: rec.hash,
                block_hash: rec.block_hash,
                block_height: rec.block_height as u64,
                timestamp: rec.timestamp.unix_timestamp_nanos() as u64, // Should be fine for about 200 years or so
                sender_address: rec.sender_address,
                receiver_address: rec.receiver_address,
                signature: rec.signature,
                calldata: rec.calldata,
            });

        Ok(tx)
    }

    pub async fn count(&self) -> Result<u64> {
        let count = sqlx::query!("SELECT COUNT(*) FROM transactions")
            .fetch_one(&self.pool)
            .await?
            .count
            .unwrap() as u64;

        Ok(count)
    }
}
