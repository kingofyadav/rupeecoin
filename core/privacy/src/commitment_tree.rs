use rupeecoin_crypto::Hash;
use rupeecoin_zkproof::poseidon_compress_bytes;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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
///
/// # Incremental root (Phase 11.3)
///
/// `frontier[i]` holds the hash of the complete subtree of height `i` that
/// forms the leftmost "waiting" node at level `i`, or `None` if no such
/// subtree exists.  Bit `i` of `leaves.len()` is 1 iff `frontier[i]` is
/// `Some`.  This allows `root()` and `append()` to run in O(TREE_DEPTH)
/// instead of O(n).
///
/// The `frontier` field is recomputed from `leaves` on deserialization so
/// the serialised form is identical to the pre-11.3 format (just `leaves`).
#[derive(Clone)]
pub struct CommitmentTree {
    pub leaves: Vec<NoteCommitment>,
    frontier: [Option<[u8; 32]>; TREE_DEPTH],
}

// ── Serde: persist only `leaves`; rebuild frontier on load ───────────────────

impl Serialize for CommitmentTree {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            leaves: &'a Vec<NoteCommitment>,
        }
        Wire {
            leaves: &self.leaves,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for CommitmentTree {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            leaves: Vec<NoteCommitment>,
        }
        let w = Wire::deserialize(d)?;
        // Replay appends to rebuild frontier in O(n · TREE_DEPTH).
        let mut tree = CommitmentTree::new();
        for leaf in w.leaves {
            tree.append(leaf);
        }
        Ok(tree)
    }
}

// ── Core implementation ───────────────────────────────────────────────────────

impl CommitmentTree {
    pub fn new() -> Self {
        CommitmentTree {
            leaves: Vec::new(),
            frontier: [None; TREE_DEPTH],
        }
    }

    pub fn len(&self) -> usize {
        self.leaves.len()
    }

    pub fn is_empty(&self) -> bool {
        self.leaves.is_empty()
    }

    /// Append a note commitment to the tree.
    ///
    /// Updates `frontier` in O(TREE_DEPTH) and pushes to `leaves` in O(1)
    /// amortised.  Returns the leaf index.
    pub fn append(&mut self, cm: NoteCommitment) -> usize {
        let idx = self.leaves.len();

        // Maintain frontier: carry = current hash, merge upward as long as
        // frontier[i] is occupied (left subtree waiting for a right partner).
        let mut carry: [u8; 32] = *cm.0.as_bytes();
        for i in 0..TREE_DEPTH {
            match self.frontier[i] {
                Some(left) => {
                    carry = poseidon_compress_bytes(&left, &carry);
                    self.frontier[i] = None;
                }
                None => {
                    self.frontier[i] = Some(carry);
                    break;
                }
            }
        }

        self.leaves.push(cm);
        idx
    }

    /// Compute the Poseidon Merkle root in O(TREE_DEPTH).
    ///
    /// Uses the incremental `frontier` to avoid rebuilding from all leaves.
    /// Correctness: `frontier[i] = Some(h)` iff bit `i` of `leaves.len()` is 1.
    /// Starting from `empty[0]` (the "current right accumulator"), merging up
    /// level by level reproduces the positional Poseidon root.
    pub fn root(&self) -> Hash {
        let empties = empty_hashes();
        let mut cur: [u8; 32] = *empties[0].as_bytes();
        for (slot, empty) in self.frontier.iter().zip(empties.iter()) {
            cur = if let Some(left) = slot {
                poseidon_compress_bytes(left, &cur)
            } else {
                poseidon_compress_bytes(&cur, empty.as_bytes())
            };
        }
        Hash::from_bytes(cur)
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
            let sibling = if sibling_idx < layer.len() {
                layer[sibling_idx]
            } else {
                empty
            };
            path.push((sibling, is_right));

            let mut next = Vec::with_capacity(layer.len().div_ceil(2));
            let mut i = 0;
            while i < layer.len() {
                let left = layer[i];
                let right = if i + 1 < layer.len() {
                    layer[i + 1]
                } else {
                    empty
                };
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
        assert_eq!(
            self.path.len(),
            TREE_DEPTH,
            "MerklePath must have exactly TREE_DEPTH ({TREE_DEPTH}) entries, got {}",
            self.path.len()
        );
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_leaf(b: u8) -> NoteCommitment {
        NoteCommitment(Hash::from_bytes([b; 32]))
    }

    /// Scratch-rebuild root using the old O(n) algorithm — used as ground truth.
    fn root_scratch(leaves: &[NoteCommitment]) -> Hash {
        if leaves.is_empty() {
            return empty_hashes()[TREE_DEPTH];
        }
        let mut layer: Vec<Hash> = leaves.iter().map(|cm| cm.0).collect();
        for level in 0..TREE_DEPTH {
            let empty = empty_hashes()[level];
            let mut next = Vec::with_capacity(layer.len().div_ceil(2));
            let mut i = 0;
            while i < layer.len() {
                let left = layer[i];
                let right = if i + 1 < layer.len() {
                    layer[i + 1]
                } else {
                    empty
                };
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

    #[test]
    fn empty_tree_root_matches() {
        let tree = CommitmentTree::new();
        assert_eq!(tree.root(), root_scratch(&[]));
    }

    #[test]
    fn single_leaf_root_matches() {
        let mut tree = CommitmentTree::new();
        let leaf = mk_leaf(1);
        tree.append(leaf);
        assert_eq!(tree.root(), root_scratch(&tree.leaves));
    }

    #[test]
    fn incremental_root_matches_scratch_for_1_to_10_leaves() {
        let mut tree = CommitmentTree::new();
        for n in 1u8..=10 {
            tree.append(mk_leaf(n));
            assert_eq!(tree.root(), root_scratch(&tree.leaves), "mismatch at n={n}");
        }
    }

    #[test]
    fn power_of_two_leaf_counts() {
        let mut tree = CommitmentTree::new();
        for n in 0u8..=8 {
            tree.append(mk_leaf(n));
        }
        assert_eq!(tree.root(), root_scratch(&tree.leaves));
    }

    #[test]
    fn serde_roundtrip_preserves_root() {
        let mut tree = CommitmentTree::new();
        for n in 1u8..=5 {
            tree.append(mk_leaf(n));
        }
        let root_before = tree.root();
        let encoded = bincode::serialize(&tree).unwrap();
        let decoded: CommitmentTree = bincode::deserialize(&encoded).unwrap();
        assert_eq!(decoded.root(), root_before);
        assert_eq!(decoded.leaves.len(), 5);
    }

    #[test]
    fn auth_path_root_consistent_with_incremental_root() {
        let mut tree = CommitmentTree::new();
        for n in 1u8..=7 {
            tree.append(mk_leaf(n));
        }
        let root = tree.root();
        for idx in 0..tree.len() {
            let path = tree.auth_path(idx).unwrap();
            assert_eq!(
                path.compute_root(tree.leaves[idx].0),
                root,
                "auth_path root mismatch for leaf {idx}"
            );
        }
    }
}
