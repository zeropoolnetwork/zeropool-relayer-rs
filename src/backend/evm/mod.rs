use std::str::FromStr;

use anyhow::Result;
use axum::async_trait;
use secp256k1::SecretKey;
use serde::Deserialize;
use web3::{
    contract::{Contract, Options},
    transports::Http,
    types::{TransactionParameters, U256},
    Web3,
};
use zeropool_tx::TxData;

use crate::{
    backend::{BlockchainBackend, TxCalldata, TxHash},
    tx::{ParsedTxData, TxValidationError},
    Engine,
};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub rpc_url: String,
    pub pool_address: String,
    pub token_address: String,
    pub sk: String,
}

pub struct EvmBackend {
    web3: Web3<Http>,
    contract: Contract<Http>,
    token: Contract<Http>,
    sk: SecretKey,
}

impl EvmBackend {
    pub fn new(config: Config) -> Result<Self> {
        let transport = Http::new(&config.rpc_url)?;
        let web3 = Web3::new(transport.clone());
        let contract = Contract::from_json(
            web3.eth(),
            config.pool_address.parse()?,
            include_bytes!("pool.json"),
        )?;
        let token = Contract::from_json(
            web3.eth(),
            config.token_address.parse()?,
            include_bytes!("token.json"),
        )?;

        let sk = SecretKey::from_str(&config.sk)?;

        Ok(Self {
            web3,
            contract,
            sk,
            token,
        })
    }
}

#[async_trait]
impl BlockchainBackend for EvmBackend {
    async fn fetch_latest_transactions(&self) -> Result<Vec<TxCalldata>> {
        Ok(vec![])
    }

    fn validate_tx(&self, _tx: &ParsedTxData) -> Vec<TxValidationError> {
        // let address = recover(&tx.signature, &tx.hash).unwrap();
        // let balance = self
        //     .token
        //     .query("balanceOf", tx.sender, None, Options::default(), None);
        // TODO: Check the balance of the sender for deposits.
        vec![]
    }

    /// Sign and send a transaction to the blockchain.
    async fn send_tx(&self, tx: TxData<Engine>) -> Result<TxHash> {
        let mut calldata = Vec::new();
        zeropool_tx::evm::write(&tx, &mut calldata)?;

        let tx_object = TransactionParameters {
            to: Some(self.contract.address()),
            data: calldata.into(),
            ..Default::default()
        };

        let signed = self
            .web3
            .accounts()
            .sign_transaction(tx_object, &self.sk)
            .await?;

        // TODO: Calculate gas
        let result = self
            .web3
            .eth()
            .send_raw_transaction(signed.raw_transaction)
            .await?;

        Ok(result.to_fixed_bytes().to_vec())
    }

    async fn get_pool_index(&self) -> Result<u64> {
        let pool_index: U256 = self
            .contract
            .query("pool_index", (), None, Options::default(), None)
            .await?;

        Ok(pool_index.as_u64())
    }

    async fn get_merkle_root(&self, index: u64) -> Result<Option<fawkes_crypto::engines::U256>> {
        let root: U256 = self
            .contract
            .query("roots", index, None, Options::default(), None)
            .await?;

        let root = fawkes_crypto::engines::U256::new(root.0);

        Ok(Some(root))
    }

    fn parse_calldata(&self, calldata: Vec<u8>) -> Result<TxData<Engine>> {
        let r = &mut calldata.as_slice();
        let tx = zeropool_tx::evm::read(r)?;
        Ok(tx)
    }

    fn parse_hash(&self, hash: &str) -> Result<Vec<u8>> {
        let hash = hex::decode(hash)?;
        Ok(hash)
    }

    fn format_hash(&self, hash: &[u8]) -> String {
        hex::encode(hash)
    }
}
