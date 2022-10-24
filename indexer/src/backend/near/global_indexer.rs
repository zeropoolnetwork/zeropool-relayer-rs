use std::time::Duration;

use anyhow::Result;
use num_traits::ToPrimitive;
use sqlx::{
    postgres::PgPoolOptions,
    types::{BigDecimal, JsonValue},
    FromRow, PgPool,
};
use tokio::sync::mpsc;

use crate::{Deserialize, Tx};

const RETRY_DELAY: Duration = Duration::from_secs(1);
const DEFAULT_REQUEST_INTERVAL_MS: u64 = 3000;
const ACQUIRE_CONNECTION_TIMEOUT: Duration = Duration::from_secs(60 * 10);

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub contract_address: String,
    pub indexer_pg_url: String,
    pub indexer_start_height: Option<u64>,
    pub indexer_request_interval: Option<u64>,
}

pub async fn start(
    backend_config: Config,
    starting_block_height: Option<u64>,
    send: mpsc::Sender<Tx>,
) -> Result<()> {
    tracing::info!("Initializing NEAR Indexer for Explorer connection pool");
    let pg = PgPoolOptions::new()
        .acquire_timeout(ACQUIRE_CONNECTION_TIMEOUT)
        .max_connections(1)
        .connect(&backend_config.indexer_pg_url)
        .await?;

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(
            backend_config
                .indexer_request_interval
                .unwrap_or(DEFAULT_REQUEST_INTERVAL_MS),
        ));
        let mut last_block_height = starting_block_height
            .or(backend_config.indexer_start_height)
            .unwrap_or(0);

        tracing::info!("Listening for new transactions");
        loop {
            interval.tick().await;

            tracing::debug!("Checking for new transactions");
            let res =
                fetch_transactions(&pg, &backend_config.contract_address, last_block_height).await;

            let txs = match res {
                Ok(txs) => txs,
                Err(e) => {
                    tracing::error!("Failed to fetch transactions: {}", e);
                    continue;
                }
            };

            for tx in txs {
                tracing::debug!("Sending transaction {} to worker", tx.hash);
                last_block_height = tx.block_height;
                send.send(tx).await.unwrap_or_else(|err| {
                    tracing::error!("Failed to send transaction to storage: {}", err);
                });
            }
        }
    })
    .await?;

    Ok(())
}

// Using block timestamp instead of block height to avoid an extra join
/// Used for pre-initializing the database.
async fn fetch_transactions(
    conn: &PgPool,
    contract_address: &str,
    from_block: u64,
) -> Result<Vec<Tx>> {
    #[derive(sqlx::FromRow)]
    struct Record {
        transaction_hash: String,
        block_timestamp: BigDecimal,
        included_in_block_hash: String,
        signer_account_id: String,
        receiver_account_id: String,
        signature: String,
        args: JsonValue,
        block_height: BigDecimal,
    }

    // Check with a simpler query
    #[derive(FromRow)]
    struct Count {
        count: i64,
    }

    let res = sqlx::query_as::<_, Count>(
        "
        SELECT count(*)
        FROM transactions as tx
            JOIN blocks AS b ON tx.included_in_block_hash = b.block_hash
        WHERE
            tx.receiver_account_id = $1
            AND b.block_height > $2
        ",
    )
    .bind(contract_address)
    .bind(from_block as i64)
    .fetch_one(conn)
    .await?;

    if res.count == 0 {
        tracing::trace!("No new transactions");
        return Ok(vec![]);
    } else {
        tracing::debug!("Found {} potential transactions", res.count);
    }

    // If the query is successful, continue with the more complex one
    let recs = sqlx::query_as::<_, Record>(
        "
        SELECT
            tx.transaction_hash,
            tx.block_timestamp,
            tx.signer_account_id,
            tx.receiver_account_id,
            tx.signature,
            tx.included_in_block_hash,
            b.block_height,
            a.args
        FROM transactions AS tx
            JOIN transaction_actions AS a ON tx.transaction_hash = a.transaction_hash
            JOIN blocks AS b ON tx.included_in_block_hash = b.block_hash
            JOIN execution_outcomes AS eo ON tx.converted_into_receipt_id = eo.receipt_id
        WHERE
            tx.receiver_account_id = $1
            AND eo.status != 'FAILURE'
            AND a.action_kind = 'FUNCTION_CALL'
            AND b.block_height > $2
            AND a.args->>'method_name' = 'transact'
        ORDER BY tx.block_timestamp ASC
    ",
    )
    .bind(contract_address)
    .bind(from_block as i64)
    .fetch_all(conn)
    .await?;

    let mut txs = Vec::new();

    for rec in recs {
        tracing::trace!("Processing tx {}", rec.transaction_hash);

        let args = rec.args["args_base64"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("args_base64 is missing"))?;
        let calldata = base64::decode(args)?;

        let tx = Tx {
            hash: rec.transaction_hash,
            block_hash: rec.included_in_block_hash,
            block_height: rec.block_height.to_u64().unwrap(),
            timestamp: rec.block_timestamp.to_u64().unwrap(),
            sender_address: rec.signer_account_id,
            receiver_address: rec.receiver_account_id,
            signature: rec.signature,
            calldata,
        };

        txs.push(tx);
    }

    Ok(txs)
}
