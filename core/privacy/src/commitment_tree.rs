use rupeecoin_crypto::{hash_concat, hash_data, Hash};
use serde::{Deserialize, Serialize};

use crate::note::NoteCommitment;

/// Tree depth: can hold up to 2^DEPTH = 2^32 ≈ 4 billion notes.
pub const TREE_DEPTH: usize = 32;

/// Precomputed empty-node hashes at each level.
/// `EMPTY[0]` = hash of an empty leaf.
/// `EMPTY[l]` = hash of two empty nodes at level `l-1`.
static EMPTY: std::sync::OnceLock<[Hash; TREE_DEPTH + 1]> = std::sync::OnceLock::new();

fn empty_hashes() -> &'static [Hash; TREE_DEPTH + 1] {
    EMPTY.get_or_init(|| {
        let mut e = [Hash::zero(); TREE_DEPTH + 1];
        e[0] = hash_data(b"RUPEECOIN_EMPTY_LEAF_v1");
        for i in 1..=TREE_DEPTH {
            e[i] = hash_concat(&e[i - 1], &e[i - 1]);
        }
        e
    })
}

/// Sparse incremental Merkle tree backed by a flat list of note commitments.
///
/// The tree is rebuilt from its leaf list on load.  For Phase 4 this is
/// acceptable; a production system would persist the frontier.
#[derive(Clone, Serialize, Deserialize)]
pub struct CommitmentTree {
    /// All note commitments in insertion order (leaves, left-to-right).
    pub leaves: Vec<NoteCommitment>,
}

impl CommitmentTree {
    pub fn new() -> Self {
        CommitmentTree { leaves: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.leaves.len()
    }

    /// Append a new note commitment and return its leaf index.
    pub fn append(&mut self, cm: NoteCommitment) -> usize {
        let idx = self.leaves.len();
        self.leaves.push(cm);
        idx
    }

    /// Compute the Merkle root of the current tree state.
    pub fn root(&self) -> Hash {
        if self.leaves.is_empty() {
            return empty_hashes()[TREE_DEPTH];
        }

        // Build the tree bottom-up.
        let mut layer: Vec<Hash> = self.leaves.iter().map(|cm| cm.0).collect();

        for level in 0..TREE_DEPTH {
            let empty = empty_hashes()[level];
            let mut next = Vec::with_capacity((layer.len() + 1) / 2);
            let mut i = 0;
            while i < layer.len() {
                let left = layer[i];
                let right = if i + 1 < layer.len() {
                    layer[i + 1]
                } else {
                    empty
                };
                next.push(hash_concat(&left, &right));
                i += 2;
            }
            layer = next;
        }

        layer[0]
    }

    /// Generate a Merkle authentication path for leaf at `index`.
    pub fn auth_path(&self, index: usize) -> Option<MerklePath> {
        if index >= self.leaves.len() {
            return None;
        }

        let mut path = Vec::with_capacity(TREE_DEPTH);
        let mut layer: Vec<Hash> = self.leaves.iter().map(|cm| cm.0).collect();
        let mut idx = index;

        for level in 0..TREE_DEPTH {
            let empty = empty_hashes()[level];
            let sibling_idx = if idx % 2 == 0 { idx + 1 } else { idx - 1 };
            let sibling = if sibling_idx < layer.len() {
                layer[sibling_idx]
            } else {
                empty
            };
            let is_right = idx % 2 == 1; // true when we're the right child
            path.push((sibling, is_right));

            let mut next = Vec::with_capacity((layer.len() + 1) / 2);
            let mut i = 0;
            while i < layer.len() {
                let left = layer[i];
                let right = if i + 1 < layer.len() {
                    layer[i + 1]
                } else {
                    empty
                };
                next.push(hash_concat(&left, &right));
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

// ── Merkle path ──────────────────────────────────────────────────────────────

/// An authentication path from a leaf to the tree root.
/// Each element is `(sibling_hash, is_right_child)`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MerklePath {
    pub path: Vec<(Hash, bool)>,
}

impl MerklePath {
    /// Recompute the root from a leaf commitment and this path.
    pub fn compute_root(&self, leaf: Hash) -> Hash {
        let mut cur = leaf;
        for &(sibling, is_right) in &self.path {
            cur = if is_right {
                hash_concat(&sibling, &cur)
            } else {
                hash_concat(&cur, &sibling)
            };
        }
        cur
    }
}
