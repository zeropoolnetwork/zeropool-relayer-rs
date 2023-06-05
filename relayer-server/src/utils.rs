use std::io::{Read, Write};

use anyhow::{anyhow, Result};
use byteorder::ByteOrder;
use libzeropool_rs::libzeropool::fawkes_crypto::{
    backend::bellman_groth16::{
        group::{G1Point, G2Point},
        prover::Proof,
    },
    ff_uint::{Num, NumRepr, PrimeField, Uint},
};

use crate::Engine;

pub fn read_num<E: ByteOrder, R: Read, P: PrimeField>(r: &mut R) -> Result<Num<P>> {
    let mut bytes = [0u8; 32];
    r.read_exact(&mut bytes)?;

    Num::from_uint(NumRepr(P::Inner::from_big_endian(&bytes)))
        .ok_or_else(|| anyhow!("invalid field element"))
}

pub fn read_proof<E: ByteOrder, R: Read>(r: &mut R) -> Result<Proof<Engine>> {
    let a = G1Point(read_num::<E, _, _>(r)?, read_num::<E, _, _>(r)?);
    let b = G2Point(
        (read_num::<E, _, _>(r)?, read_num::<E, _, _>(r)?),
        (read_num::<E, _, _>(r)?, read_num::<E, _, _>(r)?),
    );
    let c = G1Point(read_num::<E, _, _>(r)?, read_num::<E, _, _>(r)?);

    Ok(Proof { a, b, c })
}

pub fn write_num<E: ByteOrder, W: Write, P: PrimeField>(buf: &mut W, num: &Num<P>) {
    let mut bytes = [0u8; 32];
    num.to_mont_uint().0.put_big_endian(&mut bytes);
    buf.write_all(&bytes).unwrap();
}

pub fn write_proof<E: ByteOrder, W: Write>(buf: &mut W, proof: &Proof<Engine>) {
    let mut bytes = [0u8; 32 * 8];

    {
        let w = &mut &mut bytes[..];
        write_num::<E, _, _>(w, &proof.a.0);
        write_num::<E, _, _>(w, &proof.a.1);

        write_num::<E, _, _>(w, &proof.b.0 .0);
        write_num::<E, _, _>(w, &proof.b.0 .1);
        write_num::<E, _, _>(w, &proof.b.1 .0);
        write_num::<E, _, _>(w, &proof.b.1 .1);

        write_num::<E, _, _>(w, &proof.c.0);
        write_num::<E, _, _>(w, &proof.c.1);
    }

    buf.write_all(&bytes).unwrap();
}
