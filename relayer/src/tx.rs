use fawkes_crypto::backend::bellman_groth16::group::{G1Point, G2Point};
use libzeropool_rs::libzeropool::fawkes_crypto::{
    backend::bellman_groth16::prover::Proof, ff_uint::Num,
};
use serde::{Deserialize, Serialize};
use zeropool_tx::TxType;

use crate::{Engine, Fr};

#[derive(Serialize, Deserialize)]
pub struct ProofWithInputs {
    pub proof: Proof<Engine>,
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
    pub proof: Proof<Engine>,
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
            proof: Proof {
                a: G1Point(self.proof.a.0, self.proof.a.1),
                b: G2Point(
                    (self.proof.b.0 .0, self.proof.b.0 .1),
                    (self.proof.b.1 .0, self.proof.b.1 .1),
                ),
                c: G1Point(self.proof.c.0, self.proof.c.1),
            },
            delta: self.delta.clone(),
            out_commit: self.out_commit.clone(),
            nullifier: self.nullifier.clone(),
            memo: self.memo.clone(),
            extra_data: self.extra_data.clone(),
        }
    }
}
