use anyhow::Result;
use serde::Deserialize;
use tokio::{sync::mpsc, task::JoinHandle};
use web3::{
    api::{Eth, Namespace},
    contract::Contract,
    futures::StreamExt,
    types::{BlockId as Web3BlockId, BlockNumber, FilterBuilder, H256, U64},
};
use zeropool_indexer_tx_storage::Tx;

use crate::backend::{Backend, BackendMethods};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub contract_address: String,
    pub rpc_url: String,
    pub starting_block: Option<u64>,
    pub request_interval: Option<u64>,
}

pub struct EvmBackend {
    config: Config,
    latest_tx_block_id: Option<u64>,
}

impl Backend for EvmBackend {
    type Config = Config;

    fn new(backend_config: Self::Config, latest_tx: Option<Tx>) -> Result<Self> {
        Ok(Self {
            config: backend_config,
            latest_tx_block_id: latest_tx.map(|tx| tx.block_height),
        })
    }
}

#[async_trait::async_trait]
impl BackendMethods for EvmBackend {
    async fn start(self, send: mpsc::Sender<Tx>) -> Result<JoinHandle<Result<()>>> {
        let transport = web3::transports::Http::new(&self.config.rpc_url)?;
        let web3 = web3::Web3::new(transport.clone());
        let contract = Contract::from_json(
            Eth::new(transport),
            self.config.contract_address.parse()?,
            include_bytes!("./Pool.json"),
        )?;

        let event_topic = contract.abi().event("Message")?.signature();

        let from_block = self
            .latest_tx_block_id
            .map(|n| BlockNumber::Number(U64::from(n)))
            .unwrap_or(BlockNumber::Earliest);

        let filter = FilterBuilder::default()
            .address(vec![contract.address()])
            .from_block(from_block)
            .topics(Some(vec![event_topic]), None, None, None)
            .build();

        let filter = web3.eth_filter().create_logs_filter(filter).await?;

        let handle = tokio::spawn(async move {
            let logs_stream = filter.stream(std::time::Duration::from_millis(
                self.config.request_interval.unwrap_or(1000),
            ));
            futures::pin_mut!(logs_stream);

            loop {
                let Some(Ok(log)) = logs_stream.next().await else {
                    continue;
                };

                tracing::info!("Found log: {:?}", log);

                let hash = log.transaction_hash.unwrap_or(H256::zero());
                let hash = format!("{hash:#x}");

                let block_hash = log.block_hash.unwrap_or(H256::zero());
                let block_hash = format!("{block_hash:#x}");

                let block_height = log.block_number.unwrap_or(U64::zero());
                let block_height = block_height.as_u64();

                let block = match web3
                    .eth()
                    .block_with_txs(Web3BlockId::Number(block_height.into()))
                    .await
                {
                    Ok(Some(block)) => block,
                    Ok(None) => {
                        tracing::warn!("Block not found: {}", block_height);
                        continue;
                    }
                    Err(err) => {
                        tracing::warn!("Failed to get block: {:?}", err);
                        continue;
                    }
                };

                let Some(tx) = block
                    .transactions
                    .into_iter()
                    .find(|tx| tx.hash == log.transaction_hash.unwrap()) else {
                    tracing::warn!("tx not found in block {}: {}", block_height, hash);
                    continue;
                };

                let sender_address = format!("{:#x}", tx.from.unwrap_or_default());
                let receiver_address = format!("{:#x}", tx.to.unwrap_or_default());

                let mut raw_signature = vec![0; 65];
                tx.r.unwrap_or_default()
                    .to_big_endian(&mut raw_signature[0..32]);
                tx.s.unwrap_or_default()
                    .to_big_endian(&mut raw_signature[32..64]);
                raw_signature[64] = tx.v.unwrap_or_default().as_u64() as u8;

                let signature = format!("0x{}", hex::encode(&raw_signature));

                let calldata = tx.input.0;

                let tx = Tx {
                    hash,
                    block_hash,
                    block_height,
                    timestamp: block.timestamp.as_u64(),
                    sender_address,
                    receiver_address,
                    signature,
                    calldata,
                };

                send.send(tx).await?;
            }

            #[allow(unreachable_code)]
            Ok(())
        });

        Ok(handle)
    }
}
