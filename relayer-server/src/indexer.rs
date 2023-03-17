use anyhow::Result;
use reqwest::Url;
use zeropool_indexer_tx_storage::Tx;

const LIMIT: usize = 100;

pub struct IndexerApi {
    url: String,
}

impl IndexerApi {
    pub fn new(url: String) -> Self {
        IndexerApi { url }
    }

    pub async fn fetch_all(&self) -> Result<Vec<Tx>> {
        let mut txs = vec![];
        let mut block_height = 0;
        let mut url = Url::parse(&self.url)?;
        url.path_segments_mut().unwrap().push("transactions");

        loop {
            url.query_pairs_mut().clear().extend_pairs([
                ("block_height", block_height.to_string()),
                ("limit", LIMIT.to_string()),
            ]);
            let res = reqwest::get(url.clone()).await?;
            let mut new_txs: Vec<Tx> = res.json().await?;
            block_height = new_txs
                .last()
                .map(|tx| tx.block_height)
                .unwrap_or(block_height);

            txs.append(&mut new_txs);

            if new_txs.len() < LIMIT {
                break;
            }
        }

        Ok(txs)
    }
}
