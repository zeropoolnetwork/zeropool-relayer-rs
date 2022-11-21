use std::collections::{HashMap, HashSet};

type Hash = [u8; 32];
type Index = u64;

pub trait Parameters {
    const DEFAULT_LEAF_DATA: &'static [u8] = &[0u8; 32];

    fn hash(data: &[u8]) -> Hash;
}

pub struct SparseMerkleTree<P: Parameters, const H: usize> {
    nodes: HashMap<Index, Hash>,
    default_nodes: [Hash; H],
    _parameters: std::marker::PhantomData<P>,
}

impl<P: Parameters, const H: usize> SparseMerkleTree<P, H> {
    pub fn new() -> Self {
        let mut default_nodes = [[0; 32]; H];
        let mut cur_hash = P::hash(P::DEFAULT_LEAF_DATA);
        for depth in (0..H).rev() {
            default_nodes[depth] = cur_hash;
            cur_hash = P::hash(&[cur_hash, cur_hash].concat());
        }

        Self {
            nodes: HashMap::new(),
            default_nodes,
            _parameters: std::marker::PhantomData,
        }
    }

    pub fn add_leaf(&mut self, index: Index, data: &[u8]) {
        let hash = P::hash(data);
        self.add_node(H as u64 - 1, index, hash);
    }

    pub fn add_node(&mut self, depth: u64, index: u64, hash: Hash) {
        let mut cur_hash = hash;
        let mut cur_index = index;
        for depth in (1..=depth).rev() {
            let mut data = {
                let sibling_index = Self::map_index(depth, cur_index ^ 1);
                let sibling_hash = self.nodes.get(&sibling_index).copied().unwrap_or_else(|| {
                    let default = self.default_nodes[depth as usize];
                    default
                });

                let mut buf = [0; core::mem::size_of::<Hash>() * 2];

                let (left, right) = buf.split_at_mut(core::mem::size_of::<Hash>());
                if cur_index & 1 == 0 {
                    left.copy_from_slice(&cur_hash);
                    right.copy_from_slice(&sibling_hash);
                } else {
                    left.copy_from_slice(&sibling_hash);
                    right.copy_from_slice(&cur_hash);
                };

                buf
            };

            cur_hash = P::hash(&data);
            cur_index /= 2;

            let parent_depth = depth as usize - 1;
            let parent_index = Self::map_index(parent_depth as u64, cur_index);

            if cur_hash != self.default_nodes[parent_depth] {
                self.nodes.insert(parent_index, cur_hash);
            } else {
                self.nodes.remove(&parent_index);
            }
        }
    }

    pub fn rollback_to_leaf(&mut self, index: Index) {
        let mut cur_index = index;
        for depth in (1..H).rev() {
            let parent_depth = depth - 1;
            let parent_index = Self::map_index(parent_depth as u64, cur_index / 2);

            self.nodes.remove(&parent_index);

            cur_index /= 2;
        }
    }

    pub fn remove_node(&mut self, depth: u64, index: u64) {
        self.add_node(depth, index, self.default_nodes[depth as usize]);
    }

    pub fn root(&self) -> Hash {
        self.nodes
            .get(&0)
            .cloned()
            .unwrap_or_else(|| self.default_nodes[0])
    }

    pub fn merkle_proof(&self, index: Index) -> Vec<Hash> {
        let mut proof = Vec::new();
        let mut cur_index = index;
        for depth in (1..H).rev() {
            let sibling_index = Self::map_index(depth as u64, cur_index ^ 1);
            let sibling_hash = self.nodes.get(&sibling_index).copied().unwrap_or_else(|| {
                let default = self.default_nodes[depth];
                default
            });

            proof.push(sibling_hash);

            cur_index /= 2;
        }

        proof
    }

    #[inline]
    pub fn map_index(depth: u64, index: u64) -> u64 {
        (1 << depth) - 1 + index
    }

    pub fn size(&self) -> usize {
        self.nodes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestParameters;

    const HEIGHT: usize = 5;

    impl Parameters for TestParameters {
        fn hash(data: &[u8]) -> Hash {
            use sha3::{Digest, Keccak256};
            let mut hasher = Keccak256::new();
            hasher.update(data);
            hasher.finalize().into()
        }
    }

    #[test]
    fn test_add_leaf_root_changes() {
        let mut tree = SparseMerkleTree::<TestParameters, HEIGHT>::new();
        let old_root = tree.root();
        tree.add_leaf(0, &[1, 3, 5]);
        assert_ne!(old_root, tree.root());
    }

    #[test]
    fn test_add_leaf_root_not_changing_on_repeat() {
        let mut tree = SparseMerkleTree::<TestParameters, HEIGHT>::new();
        tree.add_leaf(0, &[1, 3, 5]);
        let old_root = tree.root();
        tree.add_leaf(0, &[1, 3, 5]);
        assert_eq!(old_root, tree.root());
    }

    #[test]
    fn test_remove_node() {
        let mut tree = SparseMerkleTree::<TestParameters, HEIGHT>::new();
        let initial_root = tree.root();
        tree.add_leaf(0, &[1, 3, 5]);
        let new_root = tree.root();
        tree.remove_node(HEIGHT as u64 - 1, 0);
        assert_eq!(tree.root(), initial_root);
        assert_eq!(tree.size(), 0);
    }

    #[test]
    fn test_map_index() {
        assert_eq!(
            SparseMerkleTree::<TestParameters, HEIGHT>::map_index(0, 0),
            0
        );
        assert_eq!(
            SparseMerkleTree::<TestParameters, HEIGHT>::map_index(1, 1),
            2
        );
        assert_eq!(
            SparseMerkleTree::<TestParameters, HEIGHT>::map_index(2, 0),
            3
        );
        assert_eq!(
            SparseMerkleTree::<TestParameters, HEIGHT>::map_index(2, 2),
            5
        );
    }
}
