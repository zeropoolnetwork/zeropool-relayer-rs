use std::ops::RangeBounds;

use anyhow::Result;
use fawkes_crypto::ff_uint::{PrimeField, Uint};
use libzeropool_rs::libzeropool::fawkes_crypto::ff_uint::Num;
use persy::{Persy, PersyId, ValueMode};

use crate::Fr;

pub struct TxStorage {
    db: Persy,
}

impl TxStorage {
    pub fn open(path: &str) -> Result<Self> {
        let db = Persy::open_or_create_with(path, Default::default(), |db| {
            let mut tx = db.begin()?;
            tx.create_segment("data")?;
            tx.create_index::<u32, PersyId>("keys", ValueMode::Replace)?;
            tx.prepare()?.commit()?;

            Ok(())
        })?;

        Ok(Self { db })
    }

    pub fn set(&self, index: u32, out_commit: Num<Fr>, tx_hash: &[u8], memo: &[u8]) -> Result<()> {
        let mut tx = self.db.begin()?;

        let mut buf =
            Vec::with_capacity(std::mem::size_of_val(&out_commit) + tx_hash.len() + memo.len());
        buf.extend_from_slice(&out_commit.0.to_uint().to_big_endian());
        buf.extend_from_slice(tx_hash);
        buf.extend_from_slice(memo);

        let id = tx.insert("data", &buf)?;
        tx.put::<u32, PersyId>("keys", index, id)?;
        tx.prepare()?.commit()?;

        Ok(())
    }

    pub fn get(&self, index: u32) -> Result<Option<Vec<u8>>> {
        let Some(id) = self.db.one("keys", &index)? else {
            return Ok(None);
        };

        Ok(self.db.read("data", &id)?)
    }

    pub fn rollback(&self, index: u32) -> Result<()> {
        let indices = self.db.range::<u32, PersyId, _>("keys", index..)?;

        let mut tx = self.db.begin()?;

        for (index, mut id) in indices {
            let id = id.next().unwrap();
            tx.remove::<u32, PersyId>("keys", index, None)?;
            tx.delete("data", &id)?;
        }

        tx.prepare()?.commit()?;

        Ok(())
    }

    pub fn iter<'a>(&'a self) -> Result<impl Iterator<Item = Result<(u32, Vec<u8>)>> + 'a> {
        self.iter_range(..)
    }

    pub fn iter_range<'a, R>(
        &'a self,
        range: R,
    ) -> Result<impl Iterator<Item = Result<(u32, Vec<u8>)>> + 'a>
    where
        R: RangeBounds<u32>,
    {
        let indices = self.db.range::<u32, PersyId, _>("keys", range)?;

        let iter = indices.map(|(index, mut id)| {
            let id = id.next().unwrap();
            let data = self.db.read("data", &id)?.unwrap();

            Ok((index, data))
        });

        Ok(iter)
    }
}

#[cfg(test)]
mod tests {
    use scopeguard::defer;

    use super::*;

    // #[test]
    // fn test_tx_storage() {
    //     let storage = TxStorage::open("tx_storage_test.persy").unwrap();
    //     defer! {
    //         std::fs::remove_file("tx_storage_test.persy").unwrap();
    //     }
    //
    //     storage.set(2, b"one").unwrap();
    //     storage.set(4, b"two").unwrap();
    //     storage.set(6, b"three").unwrap();
    //
    //     assert_eq!(storage.get(2).unwrap(), Some(b"one".to_vec()));
    //     assert_eq!(storage.get(4).unwrap(), Some(b"two".to_vec()));
    //     assert_eq!(storage.get(6).unwrap(), Some(b"three".to_vec()));
    //
    //     let mut iter = storage.iter().unwrap();
    //     assert_eq!(iter.next(), Some((2, b"one".to_vec())));
    //     assert_eq!(iter.next(), Some((4, b"two".to_vec())));
    //     assert_eq!(iter.next(), Some((6, b"three".to_vec())));
    //     assert_eq!(iter.next(), None);
    //
    //     let mut iter = storage.iter_range(4..).unwrap();
    //     assert_eq!(iter.next(), Some((4, b"two".to_vec())));
    //     assert_eq!(iter.next(), Some((6, b"three".to_vec())));
    //     assert_eq!(iter.next(), None);
    // }
}
