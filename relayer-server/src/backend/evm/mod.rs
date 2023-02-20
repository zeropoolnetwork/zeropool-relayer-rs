use anyhow::Result;
use serde::Deserialize;
use web3::{
    api::{Eth, Namespace},
    contract::Contract,
    transports::Http,
    Web3,
};

use crate::{
    backend::{BlockchainBackend, TxHash, TxSender},
    tx::{ParsedTxData, TxDataRequest, TxValidationError},
};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub rpc_ur: String,
    pub pool_address: String,
}

pub struct EvmBackend {
    web3: Web3<Http>,
    contract: Contract<Http>,
}

impl EvmBackend {
    pub fn new(config: Config) -> Self {
        let transport = Http::new(&config.rpc_url)?;
        let web3 = Web3::new(transport.clone());
        let contract = Contract::from_json(
            Eth::new(transport),
            config.contract_address.parse()?,
            include_bytes!("./Pool.json"),
        )?;

        Self { web3, contract }
    }
}

impl BlockchainBackend for EvmBackend {
    fn parse_tx(&self, tx: &TxDataRequest) -> Result<ParsedTxData> {
        todo!()
    }
}

impl TxSender for EvmBackend {
    async fn send_tx(&self, tx: &ParsedTxData) -> Result<TxHash> {
        //   const selector: string = PoolInstance.methods.transact().encodeABI()
        //
        //   const transferIndex = numToHex(txData.delta.transferIndex, TRANSFER_INDEX_SIZE)
        //   const energyAmount = numToHex(txData.delta.energyAmount, ENERGY_SIZE)
        //   const tokenAmount = numToHex(txData.delta.tokenAmount, TOKEN_SIZE)
        //   logger.debug(`DELTA ${transferIndex} ${energyAmount} ${tokenAmount}`)
        //
        //   const txFlatProof = flattenProof(txData.txProof)
        //   const treeFlatProof = flattenProof(txData.treeProof)
        //
        //   const memoMessage = txData.memo
        //   const memoSize = numToHex(toBN(memoMessage.length).divn(2), 4)
        //
        //   const data = [
        //     selector,
        //     txData.nullifier,
        //     txData.outCommit,
        //     transferIndex,
        //     energyAmount,
        //     tokenAmount,
        //     txFlatProof,
        //     txData.rootAfter,
        //     treeFlatProof,
        //     txData.txType,
        //     memoSize,
        //     memoMessage,
        //   ]
        //
        //   if (txData.extraData) {
        //     const extraData = truncateHexPrefix(txData.extraData)
        //     data.push(extraData)
        //   }
        //
        //   return data.join('')

        todo!()
    }
}
