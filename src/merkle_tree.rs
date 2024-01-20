use std::str::FromStr;

use anyhow::{anyhow, bail, Result};
use borsh::BorshDeserialize;
use libzeropool_rs::libzeropool::{
    constants,
    fawkes_crypto::{
        ff_uint::Num,
        native::poseidon::{poseidon, MerkleProof},
    },
    native::params::PoolParams,
    POOL_PARAMS,
};
use persy::{ByteVec, Persy, Transaction, ValueMode};

use crate::Fr;

type Hash = Num<Fr>;
type Index = u64;

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

            if !tx.exists_index("roots")? {
                tx.create_index::<Index, String>("roots", ValueMode::Replace)?;
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
        tx.drop_index("roots")?;
        tx.create_index::<Index, ByteVec>("data_index", ValueMode::Replace)?;
        tx.create_index::<String, Index>("meta_index", ValueMode::Replace)?;
        tx.create_index::<Index, String>("roots", ValueMode::Replace)?;
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

    fn add_root(&self, index: Index, root: Hash) -> Result<()> {
        let mut tx = self.db.begin()?;

        tx.put::<Index, String>("roots", index, root.to_string())?;

        tx.prepare()?.commit()?;

        Ok(())
    }

    fn get_root(&self, index: Index) -> Result<Option<Hash>> {
        let res = if let Some(data) = self.db.one::<Index, String>("roots", &index)? {
            Some(Hash::from_str(&data).map_err(|_| anyhow!("Invalid hash"))?)
        } else {
            None
        };

        Ok(res)
    }

    fn delete_root_tx(&self, tx: &mut Transaction, index: Index) -> Result<()> {
        tx.remove::<Index, String>("roots", index, None)?;

        Ok(())
    }

    fn delete_roots_tx<I>(&self, tx: &mut Transaction, values: I) -> Result<()>
    where
        I: IntoIterator<Item = Index>,
    {
        for index in values {
            tx.remove::<Index, String>("roots", index, None)?
        }

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

        if nodes.get_root(0)?.is_none() {
            nodes.add_root(0, default_nodes[0])?;
        }

        Ok(Self {
            nodes,
            default_nodes,
        })
    }

    pub fn clear_and_open(path: &str) -> Result<Self> {
        std::fs::remove_file(&path)?;
        Self::open(path)
    }

    fn set_node(&self, depth: u64, index: u64, hash: Hash) -> Result<()> {
        let mut tx = self.nodes.begin()?;

        self.nodes.set_tx(&mut tx, depth, index, hash)?;

        let mut cur_hash = hash;
        for (i, depth) in (1..=depth).rev().enumerate() {
            let cur_index = index >> i;

            let data = {
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
                self.nodes
                    .set_tx(&mut tx, parent_depth, parent_index, cur_hash)?;
            } else {
                self.nodes.delete_tx(&mut tx, parent_depth, parent_index)?; // TODO: Move cleaning up into a separate function?
            }
        }

        self.nodes.commit(tx)?;

        Ok(())
    }

    // fn set_leaf(&self, index: Index, hash: Hash) -> Result<()> {
    //     self.set_node(H as Index, index, hash)?;
    //
    //     if self.get_node(H as Index, index)?.is_none() {
    //         self.nodes.set_num_leaves(index + 1)?;
    //     }
    //
    //     self.nodes.add_root(index, hash)?;
    //
    //     Ok(())
    // }

    pub fn add_leaf(&self, hash: Hash) -> Result<()> {
        let index = self.nodes.get_num_leaves()?;
        self.set_node(H as Index, index, hash)?;
        self.nodes.set_num_leaves(index + 1)?;

        let root = self.root()?;
        self.nodes.add_root(index + 1, root)?;

        Ok(())
    }

    // /// Provides a more efficient way to add multiple leaves at once. Not used anywhere yet.
    // pub fn add_leaves_at<I: IntoIterator<Item = Hash>>(
    //     &self,
    //     index: Index,
    //     leaves: I,
    // ) -> Result<()> {
    //     let mut tx = self.nodes.begin()?;
    //
    //     let leaves = leaves.into_iter();
    //     let mut num_leaves = 0;
    //     for (i, hash) in leaves.into_iter().enumerate() {
    //         self.nodes
    //             .set_tx(&mut tx, H as Index, index + i as Index, hash)?;
    //         num_leaves += 1;
    //     }
    //
    //     if num_leaves == 0 {
    //         return Ok(());
    //     }
    //
    //     for (i, depth) in (1..=H as u64).rev().enumerate() {
    //         let mut cur_index = index >> i;
    //         if cur_index & 1 == 1 {
    //             cur_index -= 1;
    //         }
    //
    //         let num_nodes = (num_leaves as u64 >> i).max(1);
    //
    //         for lhs_index in (cur_index..=(cur_index + num_nodes)).step_by(2) {
    //             let rhs_index = lhs_index + 1;
    //
    //             let parent_hash = {
    //                 let lhs_hash = self
    //                     .nodes
    //                     .get_tx(&mut tx, depth, lhs_index)?
    //                     .unwrap_or(self.default_nodes[depth as usize]);
    //
    //                 let rhs_hash = self
    //                     .nodes
    //                     .get_tx(&mut tx, depth, rhs_index)?
    //                     .unwrap_or(self.default_nodes[depth as usize]);
    //
    //                 poseidon(&[lhs_hash, rhs_hash], POOL_PARAMS.compress())
    //             };
    //
    //             let parent_depth = depth - 1;
    //             let parent_index = lhs_index / 2;
    //
    //             if parent_hash == self.default_nodes[parent_depth as usize] {
    //                 self.nodes.delete_tx(&mut tx, parent_depth, parent_index)?;
    //             } else {
    //                 self.nodes
    //                     .set_tx(&mut tx, parent_depth, parent_index, parent_hash)?;
    //             }
    //         }
    //     }
    //
    //     let old_num_leaves = self.nodes.get_num_leaves()?;
    //     let new_num_leaves = old_num_leaves + num_leaves;
    //     self.nodes.set_num_leaves_tx(&mut tx, new_num_leaves)?;
    //
    //     self.nodes.commit(tx)?;
    //
    //     Ok(())
    // }

    /// Deletes all leaves from the tree with i >= index, recalculating the parents.
    pub fn rollback(&self, index: Index) -> Result<()> {
        if index == 0 {
            self.nodes.clear()?;
            self.nodes.set_num_leaves(0)?;
            return Ok(());
        }

        let old_num_leaves = self.nodes.get_num_leaves()?;

        if index >= old_num_leaves {
            bail!("Cannot rollback to a higher index than the latest leaf");
        }

        let mut tx = self.nodes.begin()?;
        self.nodes.delete_roots_tx(&mut tx, index..old_num_leaves)?;
        self.nodes.set_num_leaves_tx(&mut tx, index)?;
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

    // pub fn remove_node(&self, depth: u64, index: u64) -> Result<()> {
    //     self.set_node(depth, index, self.default_nodes[depth as usize])
    // }

    pub fn root(&self) -> Result<Hash> {
        let root = self
            .nodes
            .get(0, 0)?
            .unwrap_or_else(|| self.default_nodes[0]);

        Ok(root)
    }

    pub fn leaf(&self, index: Index) -> Result<Hash> {
        self.nodes
            .get(H as u64, index)
            .map(|val| val.unwrap_or_else(|| self.default_nodes[H as usize]))
    }

    pub fn historic_root(&self, index: Index) -> Result<Option<Hash>> {
        self.nodes.get_root(index)
    }

    // fn get_node(&self, depth: u64, index: u64) -> Result<Option<Hash>> {
    //     self.nodes.get(depth, index)
    // }
    //
    // pub fn get_node_with_default(&self, depth: u64, index: u64) -> Result<Hash> {
    //     self.nodes
    //         .get(depth, index)
    //         .map(|val| val.unwrap_or_else(|| self.default_nodes[depth as usize]))
    // }

    pub fn merkle_proof(&self, index: Index) -> impl Iterator<Item = Result<Hash>> + '_ {
        (0..H as u64).rev().enumerate().map(move |(i, depth)| {
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
        self.nodes.get_num_leaves().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use std::{str::FromStr, sync::atomic::AtomicU64};

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
        let (_, tree) = tree();

        for hash in hashes {
            tree.add_leaf(Hash::from_str(hash).unwrap()).unwrap();
        }

        // tree.add_leaves_at(0, hashes.iter().map(|s| Hash::from_str(s).unwrap()))
        //     .unwrap();

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
        let (_, tree) = tree();

        for hash in hashes {
            tree.add_leaf(Hash::from_str(hash).unwrap()).unwrap();
        }

        // tree.add_leaves_at(0, hashes.iter().map(|s| Hash::from_str(s).unwrap()))
        //     .unwrap();

        tree.rollback(rollback).unwrap();

        assert_eq!(tree.root().unwrap().to_string(), root);
        assert_eq!(tree.num_leaves(), rollback);
    }

    #[test]
    fn test_tree_historic_roots() {
        let (_, tree) = tree();

        let commitments = [
            "21758523569841126314748171871054218043006161291554819416231684046987851067498",
            "16724444468010964400839022626144977285825616058853472708913481597582644700596",
        ];
        let hashes = commitments
            .iter()
            .map(|s| Hash::from_str(s).unwrap())
            .collect::<Vec<_>>();

        for hash in hashes {
            tree.add_leaf(hash).unwrap();
        }

        assert_eq!(
            tree.historic_root(0).unwrap().unwrap(),
            Hash::from_str(
                "11469701942666298368112882412133877458305516134926649826543144744382391691533"
            )
            .unwrap()
        );
        assert_eq!(
            tree.historic_root(1).unwrap().unwrap(),
            Hash::from_str(
                "18217180360268434444631987097418959453267068925801925323197576743495176441694"
            )
            .unwrap()
        );
        assert_eq!(
            tree.historic_root(2).unwrap().unwrap(),
            Hash::from_str(
                "6099403096036521144404881526691887255167647210674316057097812068882884236686"
            )
            .unwrap()
        );
    }

    // TODO: Generate test cases on the fly
    #[test]
    #[ignore]
    fn generate_test_cases() {
        let mut tree = libzeropool_rs::merkle::MerkleTree::new_test(POOL_PARAMS.clone());

        println!("root 0: {}", tree.get_root());

        tree.add_hash(0, Hash::from(1), false);
        println!("root 1: {}", tree.get_root());
        println!(
            "commitment 0: {}",
            tree.get(constants::OUTPLUSONELOG as u32, 0)
        );

        tree.add_hash(128, Hash::from(2), false);
        println!("root 2: {}", tree.get_root());
        println!(
            "commitment 1: {}",
            tree.get(constants::OUTPLUSONELOG as u32, 1)
        );
    }
}
