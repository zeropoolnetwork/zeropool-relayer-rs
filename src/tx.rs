#[cfg(feature = "groth16")]
use libzeropool_rs::libzeropool::fawkes_crypto::backend::bellman_groth16::group::{
    G1Point, G2Point,
};
use libzeropool_rs::libzeropool::fawkes_crypto::ff_uint::Num;
use serde::{Deserialize, Serialize};
use zeropool_tx::{proof::Proof as _, TxType};

use crate::{Fr, Proof};

#[derive(Serialize, Deserialize)]
pub struct ProofWithInputs {
    pub proof: Proof,
    pub inputs: Vec<Num<Fr>>,
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
    pub proof: Proof,
    pub delta: Num<Fr>,
    pub out_commit: Num<Fr>,
    pub nullifier: Num<Fr>,
    pub memo: Vec<u8>,
    pub extra_data: Vec<u8>,
}

impl Clone for ParsedTxData {
    fn clone(&self) -> Self {
        Self {
            tx_type: self.tx_type,
            proof: self.proof.my_clone(),
            delta: self.delta.clone(),
            out_commit: self.out_commit.clone(),
            nullifier: self.nullifier.clone(),
            memo: self.memo.clone(),
            extra_data: self.extra_data.clone(),
        }
    }
}
