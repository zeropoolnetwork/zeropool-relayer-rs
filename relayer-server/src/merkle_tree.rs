use std::collections::{HashMap, HashSet};

use anyhow::{bail, Result};
use borsh::BorshDeserialize;
use itertools::Itertools;
use libzeropool_rs::{
    libzeropool::{
        constants,
        fawkes_crypto::{
            core::sizedvec::SizedVec,
            ff_uint::Num,
            native::poseidon::{poseidon, MerkleProof},
        },
        native::params::PoolParams,
        POOL_PARAMS,
    },
    utils::zero_note,
};
use persy::{ByteVec, Persy, PersyId, Transaction, ValueMode};

use crate::Fr;

type Hash = Num<Fr>;
type Index = u64;

type StoredHash = [u8; std::mem::size_of::<Hash>()];

struct Storage {
    db: Persy,
}

impl Storage {
    fn open(path: &str) -> Result<Self> {
        let db = Persy::open_or_create_with(path, Default::default(), |db| {
            let mut tx = db.begin()?;

            if !tx.exists_index("data_index")? {
                tx.create_index::<Index, ByteVec>("data_index", ValueMode::Replace)?;
            }

            if !tx.exists_index("meta_index")? {
                tx.create_index::<String, Index>("meta_index", ValueMode::Replace)?;
                tx.put::<String, Index>("meta_index", "num_leaves".to_owned(), 0)?;
            }

            tx.prepare().unwrap().commit().unwrap();

            Ok(())
        })
        .unwrap();

        Ok(Self { db })
    }

    fn clear(&self) -> Result<()> {
        let mut tx = self.db.begin()?;

        tx.drop_index("data_index")?;
        tx.drop_index("meta_index")?;
        tx.create_index::<Index, ByteVec>("data_index", ValueMode::Replace)?;
        tx.create_index::<String, Index>("meta_index", ValueMode::Replace)?;
        tx.put::<String, Index>("meta_index", "num_leaves".to_owned(), 0)?;

        tx.prepare()?.commit()?;

        Ok(())
    }

    fn begin(&self) -> Result<Transaction> {
        Ok(self.db.begin()?)
    }

    fn commit(&self, tx: Transaction) -> Result<()> {
        tx.prepare()?.commit()?;
        Ok(())
    }

    fn set_num_leaves(&self, index: Index) -> Result<()> {
        let mut tx = self.db.begin()?;
        tx.put("meta_index", "num_leaves".to_owned(), index)?;
        tx.prepare()?.commit()?;

        Ok(())
    }

    fn set_num_leaves_tx(&self, tx: &mut Transaction, index: Index) -> Result<()> {
        tx.put("meta_index", "num_leaves".to_owned(), index)?;

        Ok(())
    }

    fn get_num_leaves(&self) -> Result<Index> {
        Ok(self
            .db
            .one("meta_index", &"num_leaves".to_owned())?
            .expect("No latest_leaf_index key in the database"))
    }

    fn set(&self, depth: Index, index: Index, value: Hash) -> Result<()> {
        let mut tx = self.db.begin()?;
        self.set_tx(&mut tx, depth, index, value)?;
        tx.prepare()?.commit()?;

        Ok(())
    }

    fn set_tx(&self, tx: &mut Transaction, depth: Index, index: Index, value: Hash) -> Result<()> {
        let key = Self::key(depth, index);

        tx.put::<Index, ByteVec>("data_index", key, ByteVec::new(borsh::to_vec(&value)?))?;

        Ok(())
    }

    fn get(&self, depth: Index, index: Index) -> Result<Option<Hash>> {
        let res = if let Some(data) = self
            .db
            .one::<Index, ByteVec>("data_index", &Self::key(depth, index))?
        {
            Some(Hash::try_from_slice(&data)?)
        } else {
            None
        };

        Ok(res)
    }

    fn get_tx(&self, tx: &mut Transaction, depth: Index, index: Index) -> Result<Option<Hash>> {
        let res =
            if let Some(data) = tx.one::<Index, ByteVec>("data_index", &Self::key(depth, index))? {
                Some(Hash::try_from_slice(&data)?)
            } else {
                None
            };

        Ok(res)
    }

    fn delete(&self, depth: Index, index: Index) -> Result<()> {
        let mut tx = self.db.begin()?;

        let key = Self::key(depth, index);
        tx.remove::<Index, ByteVec>("data_index", key, None)?;

        tx.prepare()?.commit()?;

        Ok(())
    }

    fn delete_tx(&self, tx: &mut Transaction, depth: Index, index: Index) -> Result<()> {
        let key = Self::key(depth, index);

        tx.remove::<Index, ByteVec>("data_index", key, None)?;

        Ok(())
    }

    fn set_multiple<I>(&self, values: I) -> Result<()>
    where
        I: IntoIterator<Item = (Index, Index, Hash)>,
    {
        let mut tx = self.db.begin()?;
        for (depth, index, value) in values {
            self.set_tx(&mut tx, depth, index, value)?;
        }
        tx.prepare()?.commit()?;

        Ok(())
    }

    fn delete_multiple<I>(&self, values: I) -> Result<()>
    where
        I: IntoIterator<Item = (Index, Index)>,
    {
        let mut tx = self.db.begin()?;

        for (depth, index) in values {
            let key = Self::key(depth, index);
            tx.remove::<Index, ByteVec>("data_index", key, None)?
        }

        tx.prepare()?.commit()?;

        Ok(())
    }

    fn key(depth: Index, index: Index) -> Index {
        (1 << depth) - 1 + index
    }
}

const H: usize = constants::HEIGHT - constants::OUTPLUSONELOG;

/// A merkle tree for storing commitment hashes as leaves. Won't work for transaction hashes.
pub struct MerkleTree {
    nodes: Storage,
    /// For empty nodes with index >= length
    default_nodes: Vec<Hash>,
    num_leaves: Index,
}

impl MerkleTree {
    pub fn open(path: &str) -> Result<Self> {
        let nodes = Storage::open(path)?;

        let mut full_default_nodes = vec![Hash::ZERO; constants::HEIGHT + 1];
        for i in (0..full_default_nodes.len() - 1).rev() {
            let t = full_default_nodes[i + 1];
            full_default_nodes[i] = poseidon([t, t].as_ref(), POOL_PARAMS.compress());
        }

        let default_nodes = full_default_nodes[..=H].to_vec();

        let num_leaves = nodes.get_num_leaves()?;

        Ok(Self {
            nodes,
            default_nodes,
            num_leaves,
        })
    }

    pub fn set_node(&mut self, depth: u64, index: u64, hash: Hash) -> Result<()> {
        let mut tx = self.nodes.begin()?;

        self.nodes.set_tx(&mut tx, depth, index, hash)?;

        let mut cur_hash = hash;
        for (i, depth) in (1..=depth).rev().enumerate() {
            let cur_index = index >> i;

            let mut data = {
                let sibling_index = cur_index ^ 1;
                let sibling_hash = self
                    .nodes
                    .get_tx(&mut tx, depth, sibling_index)?
                    .unwrap_or(self.default_nodes[depth as usize]);

                if cur_index & 1 == 0 {
                    [cur_hash, sibling_hash]
                } else {
                    [sibling_hash, cur_hash]
                }
            };

            cur_hash = poseidon(&data, POOL_PARAMS.compress());

            let parent_depth = depth - 1;
            let parent_index = cur_index / 2;

            if cur_hash != self.default_nodes[parent_depth as usize] {
                if parent_depth == 0 {
                    println!("Root: {}", cur_hash);
                }

                self.nodes
                    .set_tx(&mut tx, parent_depth, parent_index, cur_hash)?;
            } else {
                self.nodes.delete_tx(&mut tx, parent_depth, parent_index)?; // TODO: Move cleaning up into a separate function?
            }
        }

        self.nodes.commit(tx)?;

        Ok(())
    }

    pub fn set_leaf(&mut self, index: Index, hash: Hash) -> Result<()> {
        self.set_node(H as Index, index, hash)?;
        self.nodes.set_num_leaves(index + 1)?;
        self.num_leaves = index + 1;

        Ok(())
    }

    pub fn add_leaf(&mut self, hash: Hash) -> Result<()> {
        let index = self.nodes.get_num_leaves()?;
        self.set_node(H as Index, index, hash)?;
        self.nodes.set_num_leaves(index + 1)?;
        self.num_leaves = index + 1;

        Ok(())
    }

    // TODO: Optimize
    pub fn add_leaves_at<I: IntoIterator<Item = Hash>>(
        &mut self,
        index: Index,
        leaves: I,
    ) -> Result<()> {
        for (i, hash) in leaves.into_iter().enumerate() {
            self.set_leaf(index + i as Index, hash)?;
        }

        Ok(())
    }

    pub fn add_leaves_at_optimized<I: IntoIterator<Item = Hash>>(
        &mut self,
        index: Index,
        leaves: I,
    ) -> Result<()> {
        let mut tx = self.nodes.begin()?;

        let leaves = leaves.into_iter();
        let mut num_leaves = 0;
        for (i, hash) in leaves.into_iter().enumerate() {
            self.nodes
                .set_tx(&mut tx, H as Index, index + i as Index, hash)?;
            num_leaves += 1;
        }

        if num_leaves == 0 {
            return Ok(());
        }

        for (i, depth) in (1..=H as u64).rev().enumerate() {
            let mut cur_index = index >> i;
            if cur_index & 1 == 1 {
                cur_index -= 1;
            }

            let num_nodes = (num_leaves as u64 >> i).max(1);

            for mut lhs_index in (cur_index..=(cur_index + num_nodes)).step_by(2) {
                let rhs_index = lhs_index + 1;

                let parent_hash = {
                    let lhs_hash = self
                        .nodes
                        .get_tx(&mut tx, depth, lhs_index)?
                        .unwrap_or(self.default_nodes[depth as usize]);

                    let rhs_hash = self
                        .nodes
                        .get_tx(&mut tx, depth, rhs_index)?
                        .unwrap_or(self.default_nodes[depth as usize]);

                    poseidon(&[lhs_hash, rhs_hash], POOL_PARAMS.compress())
                };

                let parent_depth = depth - 1;
                let parent_index = lhs_index / 2;

                if parent_hash == self.default_nodes[parent_depth as usize] {
                    self.nodes.delete_tx(&mut tx, parent_depth, parent_index)?;
                } else {
                    self.nodes
                        .set_tx(&mut tx, parent_depth, parent_index, parent_hash)?;
                }
            }
        }

        let new_num_leaves = self.num_leaves + num_leaves;
        self.nodes.set_num_leaves_tx(&mut tx, new_num_leaves)?;
        self.num_leaves = new_num_leaves;

        self.nodes.commit(tx)?;

        Ok(())
    }

    /// Deletes all leaves from the tree with i >= index, recalculating the parents.
    pub fn rollback(&mut self, index: Index) -> Result<()> {
        if index == 0 {
            self.nodes.clear()?;
            self.num_leaves = 0;
            return Ok(());
        }

        if index >= self.num_leaves {
            bail!("Cannot rollback to a higher index than the latest leaf");
        }

        let mut tx = self.nodes.begin()?;

        let old_num_leaves = self.num_leaves;
        self.nodes.set_num_leaves_tx(&mut tx, index)?;
        self.num_leaves = index;

        self.nodes.delete_tx(&mut tx, H as Index, index)?;

        for (h, depth) in (1..=H as Index).rev().enumerate() {
            let cur_index = index >> h;
            let parent_index = cur_index / 2;
            let cur_num_leaves = old_num_leaves >> h;
            let parent_depth = depth - 1;

            // Remove all unneeded nodes at the current depth
            for i in (cur_index + 1)..cur_num_leaves + 1 {
                self.nodes.delete_tx(&mut tx, depth, i)?;
            }

            // Recalculate parent for the current index
            let parent_hash = {
                let sibling_index = cur_index ^ 1;

                let current = self
                    .nodes
                    .get_tx(&mut tx, depth, cur_index)?
                    .unwrap_or(self.default_nodes[depth as usize]);

                let sibling = self
                    .nodes
                    .get_tx(&mut tx, depth, sibling_index)?
                    .unwrap_or(self.default_nodes[depth as usize]);

                let pair = if cur_index & 1 == 1 {
                    [sibling, current]
                } else {
                    [current, sibling]
                };

                poseidon(&pair, POOL_PARAMS.compress())
            };

            if parent_hash == self.default_nodes[parent_depth as usize] {
                self.nodes.delete_tx(&mut tx, parent_depth, parent_index)?;
            } else {
                self.nodes
                    .set_tx(&mut tx, parent_depth, parent_index, parent_hash)?;
            }
        }

        self.nodes.commit(tx)?;

        Ok(())
    }

    pub fn remove_node(&mut self, depth: u64, index: u64) -> Result<()> {
        self.set_node(depth, index, self.default_nodes[depth as usize])
    }

    pub fn root(&self) -> Result<Hash> {
        let root = self
            .nodes
            .get(0, 0)?
            .unwrap_or_else(|| self.default_nodes[0]);

        Ok(root)
    }

    pub fn merkle_proof(&self, index: Index) -> impl Iterator<Item = Result<Hash>> + '_ {
        (1..H as u64).rev().enumerate().map(move |(i, depth)| {
            let cur_index = index >> i;
            let sibling_index = cur_index ^ 1;
            let sibling_hash_res = self
                .nodes
                .get(depth, sibling_index)
                .map(|val| val.unwrap_or_else(|| self.default_nodes[depth as usize]));

            sibling_hash_res
        })
    }

    pub fn zp_merkle_proof(&self, index: Index) -> Result<MerkleProof<Fr, { H }>> {
        let leaves = self.merkle_proof(index).collect::<Result<_>>()?;
        let path = (0..H).rev().map(|i| (index >> i) & 1 == 0).collect();

        Ok(MerkleProof {
            sibling: leaves,
            path,
        })
    }

    pub fn num_leaves(&self) -> Index {
        self.num_leaves
    }
}

#[cfg(test)]
mod tests {
    use std::{str::FromStr, sync::atomic::AtomicU64};

    use scopeguard::defer;
    use test_case::test_case;

    use super::*;

    struct TempFile {
        path: String,
    }

    impl TempFile {
        fn new() -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let index = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let path = format!("temp_{}.persy", index);
            Self { path }
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            std::fs::remove_file(&self.path).unwrap();
        }
    }

    fn tree() -> (TempFile, MerkleTree) {
        let tmp = TempFile::new();
        let tree = MerkleTree::open(&tmp.path).unwrap();

        (tmp, tree)
    }

    // Pre-generated commitments
    #[test_case(
        &[],
        "11469701942666298368112882412133877458305516134926649826543144744382391691533";
        "empty tree"
    )]
    // 1
    #[test_case(
        &["21758523569841126314748171871054218043006161291554819416231684046987851067498"],
        "18217180360268434444631987097418959453267068925801925323197576743495176441694"
    )]
    // 1, 2
    #[test_case(
        &["16420276852541026600344033825207676569867936608872881181836367702530922827407"],
        "251605550209499043336848956117016181831224059551090160999458894430847550555"
    )]
    // 1..128
    #[test_case(
        &["11724007625716546835200693109273052718668215301673253982172959849883715209623"],
        "4148563631467949416743437885157339434364374946027595321945343539817512254601"
    )]
    // 1..129
    #[test_case(
        &[
            "11724007625716546835200693109273052718668215301673253982172959849883715209623",
            "19610086605328701226820788612686074752152186098634199524426215658185107698579"
        ],
        "21405206392816009270791415764229930987086761294527961786896913105350324305770"
    )]
    fn test_tree_add_leaves(hashes: &[&str], expected_root: &str) {
        let (_, mut tree) = tree();

        tree.add_leaves_at_optimized(0, hashes.iter().map(|s| Hash::from_str(s).unwrap()))
            .unwrap();

        assert_eq!(tree.root().unwrap().to_string(), expected_root);
        assert_eq!(tree.num_leaves() as usize, hashes.len());
    }

    #[test_case(
        &["21758523569841126314748171871054218043006161291554819416231684046987851067498"],
        0,
        "11469701942666298368112882412133877458305516134926649826543144744382391691533";
        "to 0"
    )]
    #[test_case(
        &[
            "11724007625716546835200693109273052718668215301673253982172959849883715209623",
            "19610086605328701226820788612686074752152186098634199524426215658185107698579"
        ],
        1,
        "4148563631467949416743437885157339434364374946027595321945343539817512254601";
        "to 1"
    )]
    fn test_tree_rollback_to(hashes: &[&str], rollback: u64, root: &str) {
        let (_, mut tree) = tree();

        tree.add_leaves_at_optimized(0, hashes.iter().map(|s| Hash::from_str(s).unwrap()))
            .unwrap();

        tree.rollback(rollback).unwrap();

        assert_eq!(tree.root().unwrap().to_string(), root);
        assert_eq!(tree.num_leaves(), rollback);
    }

    // TODO: Generate test cases on the fly
    #[test]
    #[ignore]
    fn generate_test_cases() {
        let mut tree = libzeropool_rs::merkle::MerkleTree::new_test(POOL_PARAMS.clone());

        println!("root 0: {}", tree.get_root());

        tree.add_hashes(0, (1..=128).map(Hash::from));
        tree.add_hashes(128, (129..=129).map(Hash::from));

        for i in 0..5 {
            let commitment = tree.get(constants::OUTPLUSONELOG as u32, i);
            println!("commitment {}: {}", i, commitment);
        }

        let root = tree.get_root();
        println!("root: {}", root);
    }
}
