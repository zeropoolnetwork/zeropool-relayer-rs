use anyhow::Result;
use libzeropool_rs::libzeropool::fawkes_crypto::{
    backend::bellman_groth16::prover::Proof, ff_uint::Num,
};
use serde::{Deserialize, Serialize};
use serde_repr::*;

use crate::{Engine, Fr};

#[derive(Debug, Clone, Copy, Serialize_repr, Deserialize_repr)]
#[repr(u16)]
enum TxType {
    Deposit = 0,
    Transfer = 1,
    Withdraw = 2,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxDataRequest {
    pub tx_type: TxType,
    #[serde(with = "hex")]
    pub proof: Vec<u8>,
    #[serde(with = "hex")]
    pub memo: Vec<u8>,
    #[serde(with = "hex")]
    pub extra_data: Vec<u8>,
}

impl TxDataRequest {
    pub fn parse(self) -> Result<RawTxData, TxValidationError::InvalidProof> {
        let proof =
            bincode::deserialize(&self.proof).map_err(|_| TxValidationError::InvalidProof)?;
        Ok(RawTxData {
            tx_type: self.tx_type,
            proof,
            memo: self.memo,
            extra_data: self.extra_data,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TxValidationError {
    InvalidProof,
}

#[derive(Debug, Clone, Copy)]
pub struct RawTxData {
    tx_type: TxType,
    proof: Proof<Engine>,
    memo: Vec<u8>,
    extra_data: Vec<u8>,
}

/// Intermediate transaction data ready to be sent to the worker.
#[derive(Debug, Clone, Copy)]
pub struct ParsedTxData {
    raw: RawTxData,
    out_commit: Num<Fr>,
    commit_index: Num<Fr>,
    tx_hash: Num<Fr>,
    tx_data: Vec<u8>,
    nullifier: Num<Fr>,
}

/// Full transaction data ready to be sent to be prepared and send to the blockchain.
#[derive(Debug, Clone, Copy)]
pub struct FullTxData {
    raw: RawTxData,
    root_after: Num<Fr>,
    delta: Num<Fr>,
    out_commit: Num<Fr>,
    commit_index: Num<Fr>,
    tx_hash: Num<Fr>,
    tx_data: Vec<u8>,
    nullifier: Num<Fr>,
    tree_proof: Proof<Engine>,
}
