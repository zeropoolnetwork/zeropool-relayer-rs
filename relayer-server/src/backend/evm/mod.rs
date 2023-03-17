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
use web3::{
    api::{Eth, Namespace},
    contract::{Contract, Options},
    ethabi::Token,
    transports::Http,
    types::{TransactionParameters, U256},
    Web3,
};

use crate::{
    backend::{BlockchainBackend, TxHash},
    tx::{FullTxData, ParsedTxData, TxDataRequest, TxType, TxValidationError},
    Engine, Fr,
};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub rpc_url: String,
    pub pool_address: String,
    pub sk: String,
}

pub struct EvmBackend {
    web3: Web3<Http>,
    contract: Contract<Http>,
    sk: SecretKey,
}

impl EvmBackend {
    pub fn new(config: Config) -> Result<Self> {
        let transport = Http::new(&config.rpc_url)?;
        let web3 = Web3::new(transport.clone());
        let contract = Contract::from_json(
            Eth::new(transport),
            config.pool_address.parse()?,
            include_bytes!("./Pool.json"),
        )?;

        let sk = SecretKey::from_str(&config.sk)?;

        Ok(Self { web3, contract, sk })
    }

    fn encode_calldata(&self, tx: FullTxData) -> Result<Vec<u8>> {
        fn write_num<W: Write, P: PrimeField>(buf: &mut W, num: &Num<P>) {
            let mut bytes = [0u8; 32];
            num.to_mont_uint().0.put_big_endian(&mut bytes);
            buf.write_all(&bytes).unwrap();
        }

        fn write_proof<W: Write>(buf: &mut W, proof: &Proof<Engine>) {
            let mut bytes = [0u8; 32 * 8];

            {
                let w = &mut &mut bytes[..];
                write_num(w, &proof.a.0);
                write_num(w, &proof.a.1);

                write_num(w, &proof.b.0 .0);
                write_num(w, &proof.b.0 .1);
                write_num(w, &proof.b.1 .0);
                write_num(w, &proof.b.1 .1);

                write_num(w, &proof.c.0);
                write_num(w, &proof.c.1);
            }

            buf.write_all(&bytes).unwrap();
        }

        // Writing it manually is more efficient, but might be error-prone.
        // Try using ethabi?
        // Example:
        //     let calldata = web3::ethabi::encode(&[
        //         Token::FixedBytes(selector.to_vec()),
        //         Token::Uint(U256(tx.nullifier.to_mont_uint().0 .0)),
        //         Token::Uint(U256(tx.out_commit.to_mont_uint().0 .0)),
        //         Token::Uint(U256(tx.delta.to_mont_uint().0 .0)),
        //         ...
        //     ]);

        let mut buf = vec![];

        let selector = self.contract.abi().function("transact")?.short_signature();
        buf.write_all(&selector)?;
        write_num(&mut buf, &tx.nullifier);
        write_num(&mut buf, &tx.out_commit);
        write_num(&mut buf, &tx.delta);
        write_proof(&mut buf, &tx.proof);
        write_num(&mut buf, &tx.root_after);
        write_proof(&mut buf, &tx.tree_proof);
        buf.write_u16::<BigEndian>(tx.tx_type as u16)?;
        buf.write_u16::<BigEndian>(tx.memo.len() as u16)?;
        buf.write_all(&tx.memo)?;
        buf.write_all(&tx.extra_data)?;

        Ok(buf)
    }
}

#[async_trait]
impl BlockchainBackend for EvmBackend {
    fn validate_tx(&self, tx: &ParsedTxData) -> Vec<TxValidationError> {
        // TODO: Check the balance of the sender for deposits.
        vec![]
    }

    /// Sign and send a transaction to the blockchain.
    async fn send_tx(&self, tx: FullTxData) -> Result<TxHash> {
        let calldata = self.encode_calldata(tx)?;

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

    fn parse_calldata(&self, calldata: Vec<u8>) -> Result<FullTxData> {
        // skip selector
        let r = &mut &calldata[4..];

        fn read_num<R: Read, P: PrimeField>(r: &mut R) -> Num<P> {
            let mut bytes = [0u8; 32];
            r.read_exact(&mut bytes).unwrap();

            Num::from_uint_reduced(NumRepr(P::Inner::from_big_endian(&bytes)))
        }

        fn read_proof<R: Read>(r: &mut R) -> Proof<Engine> {
            let a = G1Point(read_num(r), read_num(r));
            let b = G2Point((read_num(r), read_num(r)), (read_num(r), read_num(r)));
            let c = G1Point(read_num(r), read_num(r));

            Proof { a, b, c }
        }

        let nullifier = read_num(r);
        let out_commit = read_num(r);
        let delta = read_num(r);
        let proof = read_proof(r);
        let root_after = read_num(r);
        let tree_proof = read_proof(r);
        let tx_type = r.read_u16::<BigEndian>()?;
        let memo_len = r.read_u16::<BigEndian>()?;
        let mut memo = vec![0u8; memo_len as usize];
        r.read_exact(&mut memo)?;
        let mut extra_data = vec![];
        r.read_to_end(&mut extra_data)?;

        // TODO: Consider using FromPrimitive
        let tx_type = match tx_type {
            0 => TxType::Deposit,
            1 => TxType::Transfer,
            2 => TxType::Withdraw,
            _ => bail!("Invalid tx type"),
        };

        Ok(FullTxData {
            nullifier,
            out_commit,
            delta,
            proof,
            root_after,
            tree_proof,
            tx_type,
            memo,
            extra_data,
        })
    }

    fn parse_hash(&self, hash: &str) -> Result<Vec<u8>> {
        let hash = hex::decode(hash)?;
        Ok(hash)
    }
}
