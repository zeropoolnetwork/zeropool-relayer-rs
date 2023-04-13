use anyhow::Result;
use libzeropool_rs::libzeropool::fawkes_crypto::{
    backend::bellman_groth16::prover::Proof, ff_uint::Num,
};
use serde::{Deserialize, Serialize};
use serde_repr::*;

use crate::{Engine, Fr};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[repr(u16)]
pub enum TxType {
    #[serde(rename = "0000")]
    Deposit = 0,
    #[serde(rename = "0001")]
    Transfer = 1,
    #[serde(rename = "0002")]
    Withdraw = 2,
}

#[derive(Serialize, Deserialize)]
pub struct ProofWithInputs {
    pub proof: Proof<Engine>,
    pub inputs: Vec<Num<Fr>>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxDataRequest {
    pub tx_type: TxType,
    pub proof: ProofWithInputs,
    #[serde(with = "hex")]
    pub memo: Vec<u8>,
    #[serde(with = "hex")]
    pub extra_data: Vec<u8>,
    // #[serde(default)]
    // pub sync: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum TxValidationError {
    #[error("Empty memo")]
    EmptyMemo,
    #[error("Invalid transfer proof")]
    InvalidTransferProof,
    #[error("Insufficient balance for deposit")]
    InsufficientBalance,
    #[error("Fee too low")]
    FeeTooLow,
    #[error("Invalid values")]
    InvalidValues,
    #[error("Invalid tx index")]
    InvalidTxIndex,
}

/// Intermediate transaction data ready to be sent to the worker.
#[derive(Serialize, Deserialize)]
pub struct ParsedTxData {
    pub tx_type: TxType,
    pub proof: Proof<Engine>,
    pub delta: Num<Fr>,
    pub out_commit: Num<Fr>,
    pub nullifier: Num<Fr>,
    pub memo: Vec<u8>,
    pub extra_data: Vec<u8>,
}

/// Full data needed to create a blockchain transaction.
#[derive(Serialize, Deserialize)]
pub struct FullTxData {
    pub tx_type: TxType,
    pub proof: Proof<Engine>,
    pub tree_proof: Proof<Engine>,
    pub root_after: Num<Fr>,
    pub delta: Num<Fr>,
    pub out_commit: Num<Fr>,
    pub nullifier: Num<Fr>,
    pub memo: Vec<u8>,
    pub extra_data: Vec<u8>,
}
