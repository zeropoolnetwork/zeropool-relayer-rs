use std::{
    io::{Read, Write},
    str::FromStr,
};

use anyhow::{bail, Result};
use axum::{async_trait, body::Full};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use fawkes_crypto::{
    backend::bellman_groth16::group::{G1Point, G2Point},
    ff_uint::NumRepr,
};
use libzeropool_rs::libzeropool::{
    fawkes_crypto::{
        backend::bellman_groth16::prover::Proof,
        ff_uint::{Num, PrimeField, Uint},
    },
    native::tx::parse_delta,
};
use secp256k1::SecretKey;
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::{
    backend::{BlockchainBackend, TxHash},
    tx::{FullTxData, ParsedTxData, TxDataRequest, TxType, TxValidationError},
    Engine, Fr,
};

pub struct MockBackend {
    pool_index: Mutex<u64>,
}

impl MockBackend {
    pub fn new() -> Self {
        Self {
            pool_index: Mutex::new(0),
        }
    }
}

#[async_trait]
impl BlockchainBackend for MockBackend {
    fn validate_tx(&self, tx: &ParsedTxData) -> Vec<TxValidationError> {
        // let address = recover(&tx.signature, &tx.hash).unwrap();
        // let balance = self
        //     .token
        //     .query("balanceOf", tx.sender, None, Options::default(), None);
        // TODO: Check the balance of the sender for deposits.
        vec![]
    }

    /// Sign and send a transaction to the blockchain.
    async fn send_tx(&self, _tx: FullTxData) -> Result<TxHash> {
        let mut pool_index = self.pool_index.lock().await;
        *pool_index += 1;
        Ok(pool_index.to_be_bytes().to_vec())
    }

    async fn get_pool_index(&self) -> Result<u64> {
        Ok(*self.pool_index.lock().await)
    }

    fn parse_calldata(&self, calldata: Vec<u8>) -> Result<FullTxData> {
        Ok(bincode::deserialize(&calldata)?)
    }

    fn parse_hash(&self, hash: &str) -> Result<Vec<u8>> {
        let hash = hex::decode(hash)?;
        Ok(hash)
    }

    fn format_hash(&self, hash: &[u8]) -> String {
        hex::encode(hash)
    }
}
