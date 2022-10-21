use serde::{Deserialize, Serialize};
use serde_with::{base64::Base64, serde_as};

#[serde_as]
#[derive(Debug, Serialize, Deserialize)]
pub struct Tx {
    pub hash: String,
    pub block_hash: String,
    pub block_height: u64,
    pub timestamp: u64,
    pub sender_address: String,
    pub receiver_address: String,
    pub signature: String,
    #[serde_as(as = "Base64")]
    pub calldata: Vec<u8>,
}
