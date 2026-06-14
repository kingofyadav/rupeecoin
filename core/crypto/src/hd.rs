//! BLAKE3-based hierarchical deterministic key derivation for Dilithium3.
//!
//! Path convention:
//!   master            — m
//!   receive account   — m / 0 / i   (i = address index)
//!   validator key     — m / 1 / 0
use crate::signature::KeyPair;

/// Receive-address account index.
pub const HD_RECEIVE: u32 = 0;
/// Validator / change account index.
pub const HD_VALIDATOR: u32 = 1;

/// A node in the BIP-32-analogue HD derivation tree.
///
/// NOT serializable by design — private key material must never be written to
/// untrusted channels.  Persist the original entropy seed (via the keystore)
/// and re-derive on load.
#[derive(Clone)]
pub struct HdNode {
    /// Depth from root (0 = master).
    pub depth: u8,
    /// Index at this level.
    pub index: u32,
    /// 32-byte key material — used as seed for Dilithium keypair.
    key: [u8; 32],
    /// 32-byte chain code — mixed into child derivation.
    chain_code: [u8; 32],
}

impl HdNode {
    /// Derive the master node from an arbitrary-length seed (e.g. 32 random bytes
    /// or the output of PBKDF2 over a BIP-39 mnemonic).
    pub fn master(seed: &[u8]) -> Self {
        let (key, chain_code) = split(derive("rupeecoin-hd-master-v1", seed));
        HdNode {
            depth: 0,
            index: 0,
            key,
            chain_code,
        }
    }

    /// Derive the child node at `index`.
    pub fn child(&self, index: u32) -> Self {
        let mut input = [0u8; 68]; // key(32) || chain_code(32) || index(4)
        input[..32].copy_from_slice(&self.key);
        input[32..64].copy_from_slice(&self.chain_code);
        input[64..].copy_from_slice(&index.to_le_bytes());
        let (key, chain_code) = split(derive("rupeecoin-hd-child-v1", &input));
        HdNode {
            depth: self.depth.saturating_add(1),
            index,
            key,
            chain_code,
        }
    }

    /// Convert this node's key material to a Dilithium3 keypair.
    ///
    /// The derivation is deterministic: the same `HdNode` always produces
    /// the same keypair.
    pub fn to_keypair(&self) -> KeyPair {
        KeyPair::from_seed(&self.key)
    }

    /// Expose the 32-byte key seed (for storage / serialisation).
    pub fn key_seed(&self) -> &[u8; 32] {
        &self.key
    }
}

// ── Internals ─────────────────────────────────────────────────────────────────

/// Produce 64 bytes of derived material using BLAKE3 `derive_key` mode.
fn derive(context: &str, data: &[u8]) -> [u8; 64] {
    let mut out = [0u8; 64];
    blake3::Hasher::new_derive_key(context)
        .update(data)
        .finalize_xof()
        .fill(&mut out);
    out
}

fn split(bytes: [u8; 64]) -> ([u8; 32], [u8; 32]) {
    let mut key = [0u8; 32];
    let mut chain = [0u8; 32];
    key.copy_from_slice(&bytes[..32]);
    chain.copy_from_slice(&bytes[32..]);
    (key, chain)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ── Determinism ───────────────────────────────────────────────────────────

    proptest! {
        #[test]
        fn derivation_is_deterministic(
            seed in proptest::array::uniform32(any::<u8>()),
            account in 0u32..4,
            index  in 0u32..1_000,
        ) {
            let n1 = HdNode::master(&seed).child(account).child(index);
            let n2 = HdNode::master(&seed).child(account).child(index);
            prop_assert_eq!(n1.key_seed(), n2.key_seed());
            prop_assert_eq!(n1.depth,      n2.depth);
            prop_assert_eq!(n1.index,      n2.index);
        }

        #[test]
        fn different_seeds_produce_different_master_keys(
            s1 in proptest::array::uniform32(any::<u8>()),
            s2 in proptest::array::uniform32(any::<u8>()),
        ) {
            prop_assume!(s1 != s2);
            let k1 = HdNode::master(&s1).key_seed().to_vec();
            let k2 = HdNode::master(&s2).key_seed().to_vec();
            prop_assert_ne!(k1, k2);
        }

        #[test]
        fn different_indices_produce_different_child_keys(
            seed in proptest::array::uniform32(any::<u8>()),
            i1 in 0u32..500,
            i2 in 0u32..500,
        ) {
            prop_assume!(i1 != i2);
            let parent = HdNode::master(&seed);
            let k1 = parent.child(i1).key_seed().to_vec();
            let k2 = parent.child(i2).key_seed().to_vec();
            prop_assert_ne!(k1, k2);
        }

        #[test]
        fn account_separation_holds(
            seed in proptest::array::uniform32(any::<u8>()),
            index in 0u32..100,
        ) {
            // Same seed + same index but different account must differ.
            let receive   = HdNode::master(&seed).child(HD_RECEIVE).child(index);
            let validator = HdNode::master(&seed).child(HD_VALIDATOR).child(index);
            prop_assert_ne!(receive.key_seed(), validator.key_seed());
        }

        #[test]
        fn depth_is_correct(
            seed in proptest::array::uniform32(any::<u8>()),
            a in 0u32..4,
            b in 0u32..100,
        ) {
            let node = HdNode::master(&seed).child(a).child(b);
            prop_assert_eq!(node.depth, 2u8);
        }
    }

    // ── Basic unit tests ──────────────────────────────────────────────────────

    #[test]
    fn master_depth_is_zero() {
        let node = HdNode::master(&[0u8; 32]);
        assert_eq!(node.depth, 0);
        assert_eq!(node.index, 0);
    }

    #[test]
    fn child_index_stored_correctly() {
        let node = HdNode::master(&[1u8; 32]).child(42);
        assert_eq!(node.index, 42);
        assert_eq!(node.depth, 1);
    }

    #[test]
    fn to_keypair_is_deterministic() {
        let seed = [7u8; 32];
        let kp1 = HdNode::master(&seed).child(HD_RECEIVE).child(0).to_keypair();
        let kp2 = HdNode::master(&seed).child(HD_RECEIVE).child(0).to_keypair();
        assert_eq!(kp1.public_key, kp2.public_key);
    }
}
