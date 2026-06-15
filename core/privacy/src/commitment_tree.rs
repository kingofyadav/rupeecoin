use rupeecoin_crypto::Hash;
use rupeecoin_zkproof::poseidon_compress_bytes;
use serde::{Deserialize, Serialize};

use crate::note::NoteCommitment;

/// Depth matches the STARK circuit's 20-level Merkle path.
/// Supports up to 2^20 ≈ 1M note commitments.
pub const TREE_DEPTH: usize = 20;

static EMPTY: std::sync::OnceLock<[Hash; TREE_DEPTH + 1]> = std::sync::OnceLock::new();

fn empty_hashes() -> &'static [Hash; TREE_DEPTH + 1] {
    EMPTY.get_or_init(|| {
        let mut e = [Hash::zero(); TREE_DEPTH + 1];
        e[0] = Hash::from_bytes(*b"RUPEECOIN_POSEIDON_EMPTY_LEAF_v2");
        for i in 1..=TREE_DEPTH {
            e[i] = Hash::from_bytes(poseidon_compress_bytes(
                e[i - 1].as_bytes(),
                e[i - 1].as_bytes(),
            ));
        }
        e
    })
}

/// Incremental Merkle commitment tree using Poseidon compression.
///
/// Parent = Poseidon(left_child, right_child) — standard positional binary tree.
/// The STARK circuit uses direction bits (is_right) from the auth_path to
/// compute the same root, so on-chain anchor and STARK anchor are identical.
#[derive(Clone, Serialize, Deserialize)]
pub struct CommitmentTree {
    pub leaves: Vec<NoteCommitment>,
}

impl CommitmentTree {
    pub fn new() -> Self {
        CommitmentTree { leaves: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.leaves.len()
    }

    pub fn append(&mut self, cm: NoteCommitment) -> usize {
        let idx = self.leaves.len();
        self.leaves.push(cm);
        idx
    }

    /// Compute the Poseidon Merkle root.
    pub fn root(&self) -> Hash {
        if self.leaves.is_empty() {
            return empty_hashes()[TREE_DEPTH];
        }

        let mut layer: Vec<Hash> = self.leaves.iter().map(|cm| cm.0).collect();

        for level in 0..TREE_DEPTH {
            let empty = empty_hashes()[level];
            let mut next = Vec::with_capacity((layer.len() + 1) / 2);
            let mut i = 0;
            while i < layer.len() {
                let left = layer[i];
                let right = if i + 1 < layer.len() { layer[i + 1] } else { empty };
                next.push(Hash::from_bytes(poseidon_compress_bytes(
                    left.as_bytes(),
                    right.as_bytes(),
                )));
                i += 2;
            }
            layer = next;
        }

        layer[0]
    }

    /// Return the 20 Poseidon siblings and direction bits for the STARK circuit.
    ///
    /// `is_right = true` means the leaf/running-node is the right child at that level,
    /// so the STARK trace should use `compress(sibling, node)` instead of `compress(node, sibling)`.
    pub fn auth_path(&self, index: usize) -> Option<MerklePath> {
        if index >= self.leaves.len() {
            return None;
        }

        let mut path = Vec::with_capacity(TREE_DEPTH);
        let mut layer: Vec<Hash> = self.leaves.iter().map(|cm| cm.0).collect();
        let mut idx = index;

        for level in 0..TREE_DEPTH {
            let empty = empty_hashes()[level];
            let is_right = idx % 2 == 1;
            let sibling_idx = idx ^ 1;
            let sibling =
                if sibling_idx < layer.len() { layer[sibling_idx] } else { empty };
            path.push((sibling, is_right));

            let mut next = Vec::with_capacity((layer.len() + 1) / 2);
            let mut i = 0;
            while i < layer.len() {
                let left = layer[i];
                let right = if i + 1 < layer.len() { layer[i + 1] } else { empty };
                next.push(Hash::from_bytes(poseidon_compress_bytes(
                    left.as_bytes(),
                    right.as_bytes(),
                )));
                i += 2;
            }
            layer = next;
            idx /= 2;
        }

        Some(MerklePath { path })
    }
}

impl Default for CommitmentTree {
    fn default() -> Self {
        Self::new()
    }
}

/// Merkle authentication path: 20 (sibling_hash, is_right_child) pairs.
///
/// `is_right_child = true` means the spending note is the RIGHT child at that level,
/// so the parent = Poseidon(sibling, note).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MerklePath {
    pub path: Vec<(Hash, bool)>,
}

impl MerklePath {
    /// Recompute the root using positional Poseidon: `compress(left, right)`.
    pub fn compute_root(&self, leaf: Hash) -> Hash {
        let mut cur = leaf;
        for &(sib, is_right) in &self.path {
            cur = if is_right {
                Hash::from_bytes(poseidon_compress_bytes(sib.as_bytes(), cur.as_bytes()))
            } else {
                Hash::from_bytes(poseidon_compress_bytes(cur.as_bytes(), sib.as_bytes()))
            };
        }
        cur
    }
}
