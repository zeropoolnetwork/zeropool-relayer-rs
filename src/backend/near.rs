use std::time::Duration;

use anyhow::Result;
use axum::async_trait;
use borsh::BorshDeserialize;
use itertools::Itertools;
use libzeropool_rs::libzeropool::fawkes_crypto::{engines::U256, ff_uint::Uint};
use near_crypto::InMemorySigner;
use near_jsonrpc_client::{methods, JsonRpcClient};
use near_jsonrpc_primitives::types::query::QueryResponseKind;
use near_primitives::{
    transaction::{Action, FunctionCallAction, Transaction},
    types::{AccountId, BlockReference, Finality, FunctionArgs},
    views::{ActionView, FinalExecutionOutcomeView, FinalExecutionStatus, QueryRequest},
};
use reqwest::Url;
use serde::Deserialize;
use tokio::time::sleep;
use zeropool_tx::{TxData, TxType};

use crate::{
    backend::{BlockchainBackend, TxCalldata, TxHash},
    tx::{ParsedTxData, TxValidationError},
    Fr, Proof,
};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub network: String,
    pub rpc_url: String,
    pub archive_rpc_url: String,
    pub sk: String,
    pub pool_address: AccountId,
    pub relayer_account_id: AccountId,
    pub token_id: AccountId,
}

pub struct NearBackend {
    config: Config,
    client: JsonRpcClient,
    signer: InMemorySigner,
}

impl NearBackend {
    pub fn new(config: Config) -> Result<Self> {
        let client = JsonRpcClient::connect(&config.rpc_url);
        let signer =
            InMemorySigner::from_secret_key(config.relayer_account_id.clone(), config.sk.parse()?);

        Ok(Self {
            config,
            client,
            signer,
        })
    }
}

#[async_trait]
impl BlockchainBackend for NearBackend {
    fn name(&self) -> &'static str {
        "near"
    }

    async fn fetch_latest_transactions(&self) -> Result<Vec<TxCalldata>> {
        const PAGE_SIZE: u64 = 25;

        let client = NearblocksClient::new(&self.config.network, &self.config.pool_address)?;
        let tx_count = client.get_tx_count().await?;

        if tx_count == 0 {
            return Ok(vec![]);
        }

        let mut txs = Vec::new();
        for page in 1..=(tx_count / PAGE_SIZE + 1) {
            tracing::info!("Fetching page {} of {}", page, tx_count / PAGE_SIZE + 1);

            let pairs = client.get_zeropool_txns(page, PAGE_SIZE).await?;

            // Fetch transaction data from the archive node.
            for IndexerTx { hash, sender } in pairs {
                let client = reqwest::Client::new();
                let res: serde_json::Value = client
                    .post(&self.config.archive_rpc_url)
                    .json(&serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": "dontcare",
                        "method": "tx",
                        "params": [hash, sender]
                    }))
                    .send()
                    .await?
                    .json()
                    .await?;

                let tx =
                    serde_json::from_value::<FinalExecutionOutcomeView>(res["result"].clone())?;

                for action in tx.transaction.actions {
                    if let ActionView::FunctionCall {
                        method_name, args, ..
                    } = action
                    {
                        if method_name != "transact" {
                            tracing::info!("Skipping non-'transact' transaction");
                            continue;
                        }

                        let calldata = args.into();
                        let hash = tx.transaction.hash.0.to_vec();

                        let tx = TxCalldata { hash, calldata };

                        txs.push(tx);
                    }
                }
            }
        }

        Ok(txs)
    }

    async fn validate_tx(&self, _tx: &ParsedTxData) -> Vec<TxValidationError> {
        vec![]
    }

    /// Sign and send a transaction to the blockchain.
    async fn send_tx(&self, tx: TxData<Fr, Proof>) -> Result<TxHash> {
        let access_key_query_response = self
            .client
            .call(methods::query::RpcQueryRequest {
                block_reference: BlockReference::latest(),
                request: QueryRequest::ViewAccessKey {
                    account_id: self.signer.account_id.clone(),
                    public_key: self.signer.public_key.clone(),
                },
            })
            .await?;

        let current_nonce = match access_key_query_response.kind {
            QueryResponseKind::AccessKey(access_key) => access_key.nonce,
            _ => anyhow::bail!("Unexpected response from access key query"),
        };

        let mut args: Vec<u8> = Vec::new();
        zeropool_tx::near::write(&tx, &mut args)?;

        let transaction = Transaction {
            signer_id: self.signer.account_id.clone(),
            public_key: self.signer.public_key.clone(),
            nonce: current_nonce + 1,
            receiver_id: self.config.pool_address.clone(),
            block_hash: access_key_query_response.block_hash,
            actions: vec![Action::FunctionCall(FunctionCallAction {
                method_name: "transact".to_string(),
                args,
                gas: 300_000_000_000_000, // 300 TeraGas, TODO: estimate gas
                deposit: 0,
            })],
        };

        let request = methods::broadcast_tx_async::RpcBroadcastTxAsyncRequest {
            signed_transaction: transaction.sign(&self.signer),
        };

        // TODO: Check the status of the transaction
        let tx_hash = self.client.call(request).await?;

        tracing::debug!("Near transaction sent: {}", tx_hash);

        loop {
            tracing::info!("Checking transaction status");
            let status_req = methods::tx::RpcTransactionStatusRequest {
                transaction_info: methods::tx::TransactionInfo::TransactionId {
                    hash: tx_hash,
                    account_id: self.signer.account_id.clone(),
                },
            };

            let response = match self.client.call(status_req).await {
                Ok(res) => res,
                Err(err) => {
                    // TODO: Limit number of attempts?
                    tracing::warn!("Failed to fetch tx status: {:?}", err);
                    continue;
                }
            };

            match response.status {
                FinalExecutionStatus::Failure(err) => {
                    tracing::error!("Transaction failed");
                    anyhow::bail!("Transaction failed: {:?}", err);
                }
                FinalExecutionStatus::SuccessValue(_) => {
                    tracing::info!("Transaction succeeded");
                    break;
                }
                _ => {
                    tracing::info!("Transaction pending");
                    sleep(Duration::from_secs(1)).await; // TODO: exponential backoff
                }
            };
        }

        Ok(tx_hash.0.to_vec())
    }

    async fn get_pool_index(&self) -> Result<u64> {
        let request = methods::query::RpcQueryRequest {
            block_reference: BlockReference::Finality(Finality::Final),
            request: QueryRequest::CallFunction {
                account_id: self.config.pool_address.clone(),
                method_name: "pool_index".to_owned(),
                args: FunctionArgs::from(Vec::new()),
            },
        };

        let response = self.client.call(request).await?;

        if let QueryResponseKind::CallResult(result) = response.kind {
            let num = U256::from_little_endian(&result.result);
            Ok(num.as_u64())
        } else {
            Err(anyhow::anyhow!("get_pool_index: Unexpected response"))
        }
    }

    async fn get_merkle_root(&self, index: u64) -> Result<Option<U256>> {
        let index = U256::from(index);
        let args = FunctionArgs::from(borsh::to_vec(&index)?);
        let request = methods::query::RpcQueryRequest {
            block_reference: BlockReference::Finality(Finality::Final),
            request: QueryRequest::CallFunction {
                account_id: self.config.pool_address.clone(),
                method_name: "merkle_root".to_owned(),
                args,
            },
        };

        let response = self.client.call(request).await?;

        if let QueryResponseKind::CallResult(result) = response.kind {
            Ok(<Option<U256>>::try_from_slice(&result.result)?)
        } else {
            Err(anyhow::anyhow!("get_merkle_root: Unexpected response"))
        }
    }

    fn parse_calldata(&self, calldata: Vec<u8>) -> Result<TxData<Fr, Proof>> {
        let r = &mut calldata.as_slice();
        let tx = zeropool_tx::near::read(r)?;
        Ok(tx)
    }

    fn extract_ciphertext_from_memo<'a>(&self, memo: &'a [u8], tx_type: TxType) -> &'a [u8] {
        let offset: usize = match tx_type {
            TxType::Deposit | TxType::Transfer => 8,
            TxType::Withdraw => {
                let addr_len_bytes: [u8; 4] = memo[20..24].try_into().unwrap_or_default();
                let addr_len = u32::from_le_bytes(addr_len_bytes) as usize;

                16 + 4 + addr_len
            }
        };

        &memo[offset..]
    }

    fn parse_hash(&self, hash: &str) -> Result<Vec<u8>> {
        bs58::decode(hash).into_vec().map_err(Into::into)
    }

    fn format_hash(&self, hash: &[u8]) -> String {
        bs58::encode(hash).into_string()
    }
}

struct IndexerTx {
    hash: String,
    sender: String,
}

struct NearblocksClient {
    url: Url,
    account: String,
}

impl NearblocksClient {
    fn new(network: &str, account: &str) -> Result<Self> {
        let url = match network {
            "mainnet" => format!("https://api.nearblocks.io/v1/account/{}", account),
            "testnet" => format!("https://api-testnet.nearblocks.io/v1/account/{}", account),
            _ => anyhow::bail!("Unknown network"),
        };

        let url = Url::parse(&url)?;

        Ok(Self {
            url,
            account: account.to_string(),
        })
    }

    pub async fn get_tx_count(&self) -> Result<u64> {
        #[derive(Deserialize)]
        struct Response {
            txns: Vec<Count>,
        }

        #[derive(Deserialize)]
        struct Count {
            count: String,
        }

        let mut url = self.url.clone();
        url.path_segments_mut().unwrap().push("txns").push("count");

        let response = reqwest::get(url).await?.json::<Response>().await?;
        let count = response
            .txns
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No tx count present"))?
            .count
            .parse()?;

        Ok(count)
    }

    pub async fn get_zeropool_txns(&self, page: u64, per_page: u64) -> Result<Vec<IndexerTx>> {
        #[derive(Deserialize)]
        struct Response {
            txns: Vec<Transaction>,
        }

        #[derive(Deserialize)]
        struct Transaction {
            transaction_hash: String,
            predecessor_account_id: String,
            receiver_account_id: String,
            actions: Vec<Action>,
            outcomes: Outcome,
        }

        #[derive(Deserialize)]
        struct Outcome {
            status: bool,
        }

        #[derive(Deserialize)]
        struct Action {
            action: String,
            method: Option<String>,
        }

        let mut url = self.url.clone();
        url.path_segments_mut().unwrap().push("txns");

        url.query_pairs_mut()
            .append_pair("order", "asc")
            .append_pair("page", &page.to_string())
            .append_pair("per_page", &per_page.to_string());

        tracing::debug!("Fetching transaction hashes from {}", url);

        let mut response = reqwest::get(url).await?.json::<Response>().await?;

        let relevant_txs = response.txns.drain(..).filter_map(|tx| {
            if tx.receiver_account_id != self.account.as_str() || !tx.outcomes.status {
                return None;
            }

            tx.actions.into_iter().find(|action| {
                action.action == "FUNCTION_CALL" && action.method.as_deref() == Some("transact")
            })?;

            Some(IndexerTx {
                hash: tx.transaction_hash,
                sender: tx.predecessor_account_id,
            })
        });

        Ok(relevant_txs.collect())
    }
}
