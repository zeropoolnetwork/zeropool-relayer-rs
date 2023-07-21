use anyhow::Result;
use axum::async_trait;
use near_crypto::InMemorySigner;
use near_jsonrpc_client::{methods, JsonRpcClient};
use near_jsonrpc_primitives::types::query::QueryResponseKind;
use near_primitives::{
    transaction::{Action, FunctionCallAction, Transaction},
    types::{AccountId, BlockReference, Finality, FunctionArgs},
    views::{ActionView, FinalExecutionOutcomeView, QueryRequest},
};
use reqwest::Url;
use serde::Deserialize;
use serde_json::from_slice;
use zeropool_tx::TxData;

use crate::{
    backend::{BlockchainBackend, TxCalldata, TxHash},
    tx::{ParsedTxData, TxValidationError},
    Engine,
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
    async fn init_state(&self) -> Result<Vec<TxCalldata>> {
        const PAGE_SIZE: u64 = 25;

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
        }

        #[derive(Deserialize)]
        struct Action {
            action: String,
            method: String,
        }

        // TODO: Support different indexer services.

        let indexer_url = match self.config.network.as_str() {
            "mainnet" => format!(
                "https://api.nearblocks.io/v1/account/{}/txns",
                &self.config.pool_address
            ),
            "testnet" => format!(
                "https://api-testnet.nearblocks.io/v1/account/{}/txns",
                &self.config.pool_address
            ),
            _ => anyhow::bail!("Unknown network"),
        };

        let mut indexer_url = Url::parse_with_params(
            &indexer_url,
            &[("order", "asc"), ("page", "1"), ("per_page", "25")],
        )?;
        let mut current_page = 1;

        // Receive (tx hash, sender account id) pairs from the indexer.
        let mut pairs = Vec::new();
        loop {
            let mut response = reqwest::get(indexer_url.clone())
                .await?
                .json::<Response>()
                .await?;

            if response.txns.is_empty() {
                break;
            }

            let relevant_txs = response.txns.drain(..).filter_map(|tx| {
                if tx.receiver_account_id != self.config.pool_address.as_str() {
                    return None;
                }

                tx.actions.into_iter().find(|action| {
                    action.action == "FUNCTION_CALL" && action.method == "transact"
                })?;

                Some((tx.transaction_hash, tx.predecessor_account_id))
            });

            pairs.extend(relevant_txs);

            current_page += 1;
            indexer_url
                .query_pairs_mut()
                .clear()
                .append_pair("order", "asc")
                .append_pair("page", &current_page.to_string())
                .append_pair("per_page", &PAGE_SIZE.to_string());
        }

        // Fetch transaction data from the archive node.
        let mut txs = Vec::new();
        for (hash, sender_id) in pairs {
            let client = reqwest::Client::new();
            let res: serde_json::Value = client
                .post(&self.config.archive_rpc_url)
                .json(&serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": "dontcare",
                    "method": "tx",
                    "params": [hash, sender_id]
                }))
                .send()
                .await?
                .json()
                .await?;

            let tx = serde_json::from_value::<FinalExecutionOutcomeView>(res["result"].clone())?;

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

        Ok(txs)
    }

    fn validate_tx(&self, _tx: &ParsedTxData) -> Vec<TxValidationError> {
        vec![]
    }

    /// Sign and send a transaction to the blockchain.
    async fn send_tx(&self, tx: TxData<Engine>) -> Result<TxHash> {
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

        Ok(tx_hash.0.to_vec())
    }

    async fn get_pool_index(&self) -> Result<u64> {
        let request = methods::query::RpcQueryRequest {
            block_reference: BlockReference::Finality(Finality::Final),
            request: QueryRequest::CallFunction {
                account_id: self.config.token_id.clone(),
                method_name: "pool_index".to_owned(),
                args: FunctionArgs::from(Vec::new()),
            },
        };

        let response = self.client.call(request).await?;

        if let QueryResponseKind::CallResult(result) = response.kind {
            Ok(from_slice::<u64>(&result.result)?)
        } else {
            Err(anyhow::anyhow!("get_pool_index: Unexpected response"))
        }
    }

    fn parse_calldata(&self, calldata: Vec<u8>) -> Result<TxData<Engine>> {
        let r = &mut calldata.as_slice();
        let tx = zeropool_tx::near::read(r)?;
        Ok(tx)
    }

    fn parse_hash(&self, hash: &str) -> Result<Vec<u8>> {
        bs58::decode(hash).into_vec().map_err(Into::into)
    }

    fn format_hash(&self, hash: &[u8]) -> String {
        bs58::encode(hash).into_string()
    }
}
