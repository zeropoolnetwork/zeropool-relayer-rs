use std::ops::RangeBounds;

use anyhow::Result;
use libzeropool_rs::libzeropool::{
    constants,
    fawkes_crypto::ff_uint::{Num, PrimeField, Uint},
};
use persy::{Persy, PersyId, ValueMode};

use crate::Fr;

pub type Index = u64;

const STRIDE: u64 = constants::OUT as u64 + 1;

pub struct TxStorage {
    db: Persy,
}

impl TxStorage {
    pub fn open(path: &str) -> Result<Self> {
        let db = Persy::open_or_create_with(path, Default::default(), |db| {
            let mut tx = db.begin()?;
            tx.create_segment("data")?;
            tx.create_index::<Index, PersyId>("keys", ValueMode::Replace)?;
            tx.create_index::<String, u64>("meta", ValueMode::Replace)?;
            tx.put("meta", "next_index".to_owned(), 0u64)?;
            tx.prepare()?.commit()?;

            Ok(())
        })?;

        Ok(Self { db })
    }

    pub fn clear_and_open(path: &str) -> Result<Self> {
        std::fs::remove_file(&path)?;
        Self::open(path)
    }

    pub fn set(
        &self,
        index: Index,
        out_commit: Num<Fr>,
        tx_hash: &[u8],
        memo: &[u8],
    ) -> Result<()> {
        let next_index = self.next_index()?;

        if index > next_index {
            return Err(anyhow::anyhow!(
                "Invalid index: expected {}, got {}",
                next_index,
                index
            ));
        }

        let mut tx = self.db.begin()?;

        let mut buf =
            Vec::with_capacity(std::mem::size_of_val(&out_commit) + tx_hash.len() + memo.len());
        buf.extend_from_slice(&out_commit.0.to_uint().to_big_endian());
        buf.extend_from_slice(tx_hash);
        buf.extend_from_slice(memo);

        let id = tx.insert("data", &buf)?;
        tx.put::<Index, PersyId>("keys", index, id)?;

        tx.put("meta", "next_index".to_owned(), index + STRIDE)?;

        tx.prepare()?.commit()?;

        Ok(())
    }

    pub fn get(&self, index: Index) -> Result<Option<Vec<u8>>> {
        let Some(id) = self.db.one("keys", &index)? else {
            return Ok(None);
        };

        Ok(self.db.read("data", &id)?)
    }

    /// Remove all transactions with indices >= `index`.
    pub fn rollback(&self, index: Index) -> Result<()> {
        let indices = self.db.range::<Index, PersyId, _>("keys", index..)?;

        let mut tx = self.db.begin()?;

        for (index, mut id) in indices {
            let id = id.next().unwrap();
            tx.remove::<Index, PersyId>("keys", index, None)?;
            tx.delete("data", &id)?;
        }

        tx.put("meta", "next_index".to_owned(), index)?;

        tx.prepare()?.commit()?;

        Ok(())
    }

    pub fn next_index(&self) -> Result<Index> {
        Ok(self
            .db
            .one::<String, Index>("meta", &"next_index".to_string())
            .map(|val| val.unwrap_or(0))?)
    }

    pub fn len(&self) -> Result<Index> {
        self.next_index()
    }

    pub fn iter<'a>(&'a self) -> Result<impl Iterator<Item = Result<(u64, Vec<u8>)>> + 'a> {
        self.iter_range(..)
    }

    pub fn iter_range<'a, R>(
        &'a self,
        range: R,
    ) -> Result<impl Iterator<Item = Result<(Index, Vec<u8>)>> + 'a>
    where
        R: RangeBounds<Index>,
    {
        let indices = self.db.range::<Index, PersyId, _>("keys", range)?;

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

    #[test]
    fn test_tx_storage_set() {
        const FILE_NAME: &str = "tx_storage_test_invalid_index.persy";
        let storage = TxStorage::open(FILE_NAME).unwrap();
        defer! {
            std::fs::remove_file(FILE_NAME).unwrap();
        }

        storage.set(0, Num::ZERO, &[0, 1, 2], &[3, 4, 5]).unwrap();
        let res = storage.set(1, Num::ZERO, &[0, 1, 2], &[3, 4, 5]);
        assert!(res.is_err());

        let res = storage.set(128, Num::ZERO, &[0, 1, 2], &[3, 4, 5]);
        assert!(res.is_ok());
    }
}
