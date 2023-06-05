use anyhow::Result;
use axum::async_trait;
use near_crypto::{InMemorySigner, SecretKey};
use near_jsonrpc_client::{methods, JsonRpcClient};
use near_jsonrpc_primitives::types::query::QueryResponseKind;
use near_primitives::{
    transaction::{Action, FunctionCallAction, Transaction},
    types::{AccountId, BlockReference, Finality, FunctionArgs},
    views::QueryRequest,
};
use serde::Deserialize;
use serde_json::from_slice;
use zeropool_tx::TxData;

use crate::{
    backend::{BlockchainBackend, TxHash},
    tx::{ParsedTxData, TxValidationError},
    Engine,
};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub rpc_url: String,
    pub sk: String,
    pub pool_address: AccountId,
    pub relayer_account_id: AccountId,
    pub token_id: AccountId,
}

pub struct NearBackend {
    config: Config,
    client: JsonRpcClient,
    signer: near_crypto::InMemorySigner,
    // sk: SecretKey,
}

impl NearBackend {
    pub fn new(config: Config) -> Result<Self> {
        // let sk = SecretKey::from_str(&config.sk)?;
        let client = JsonRpcClient::connect(&config.rpc_url);
        let signer =
            InMemorySigner::from_secret_key(config.relayer_account_id.clone(), config.sk.parse()?);

        Ok(Self {
            config,
            client,
            signer,
            // sk,
        })
    }
}

#[async_trait]
impl BlockchainBackend for NearBackend {
    fn validate_tx(&self, _tx: &ParsedTxData) -> Vec<TxValidationError> {
        vec![]
    }

    /// Sign and send a transaction to the blockchain.
    async fn send_tx(&self, tx: TxData<Engine>) -> Result<TxHash> {
        let access_key_query_response = self
            .client
            .call(methods::query::RpcQueryRequest {
                block_reference: BlockReference::latest(),
                request: near_primitives::views::QueryRequest::ViewAccessKey {
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
                gas: 100_000_000_000_000, // 100 TeraGas, TODO: estimate gas
                deposit: 0,
            })],
        };

        let request = methods::broadcast_tx_async::RpcBroadcastTxAsyncRequest {
            signed_transaction: transaction.sign(&self.signer),
        };

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
