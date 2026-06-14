use rupeecoin_crypto::{leaf_hash, node_hash, Hash, Signature};
use rupeecoin_privacy::PrivacyTransaction;
use serde::{Deserialize, Serialize};

use crate::{attestation::Attestation, transaction::Transaction};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct BlockHeader {
    pub version: u32,
    pub prev_hash: Hash,
    pub merkle_root: Hash,
    /// Unix timestamp (seconds).
    pub timestamp: u64,
    pub height: u64,
    // ── Phase 3: consensus ────────────────────────────────────────────────────
    pub slot: u64,
    pub proposer_index: u32,
    // ── Phase 4: privacy pool ─────────────────────────────────────────────────
    /// Merkle root over privacy-transaction hashes (`Hash::zero()` when empty).
    pub privacy_root: Hash,
    // ── Phase 6: EIP-1559 fee market ─────────────────────────────────────────
    pub base_fee: u64,
    pub gas_used: u64,
    // ── Phase 7: state commitment ─────────────────────────────────────────────
    /// Sparse Merkle root over the UTXO set after this block.
    pub state_root: Hash,
    // ── Phase 8: VRF proposer selection ──────────────────────────────────────
    /// VRF output for this slot.  The next block's proposer index is derived from
    /// `BLAKE3(vrf_output || next_slot)`, chaining unpredictability forward.
    /// `Hash::zero()` for genesis.
    pub vrf_output: Hash,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
    // ── Phase 3 ──────────────────────────────────────────────────────────────
    pub attestations: Vec<Attestation>,
    /// Proposer's Dilithium3 signature over the block hash.
    pub proposer_signature: Option<Signature>,
    // ── Phase 4 ──────────────────────────────────────────────────────────────
    pub privacy_txs: Vec<PrivacyTransaction>,
    // ── Phase 8: VRF proof ───────────────────────────────────────────────────
    /// Dilithium3 signature that proves the proposer produced `header.vrf_output`
    /// using their registered key.  `None` for genesis and bootstrap blocks.
    pub vrf_proof: Option<Signature>,
}

impl Block {
    pub fn hash(&self) -> Hash {
        rupeecoin_crypto::hash_data(
            &bincode::serialize(&self.header).expect("header serialization"),
        )
    }

    pub fn compute_merkle_root(&self) -> Hash {
        merkle_root(&self.transactions)
    }

    pub fn compute_privacy_root(&self) -> Hash {
        privacy_merkle_root(&self.privacy_txs)
    }
}

/// Domain-separated binary Merkle tree over transaction IDs.
///
/// Leaf:     BLAKE3("rpc_leaf" || txid)
/// Internal: BLAKE3("rpc_node" || left || right)
/// Odd:      duplicate last leaf
pub fn merkle_root(txs: &[Transaction]) -> Hash {
    if txs.is_empty() {
        return Hash::zero();
    }
    let mut layer: Vec<Hash> = txs
        .iter()
        .map(|tx| leaf_hash(tx.txid().as_bytes()))
        .collect();
    while layer.len() > 1 {
        let mut next = Vec::with_capacity((layer.len() + 1) / 2);
        for pair in layer.chunks(2) {
            let right = if pair.len() == 2 { &pair[1] } else { &pair[0] };
            next.push(node_hash(&pair[0], right));
        }
        layer = next;
    }
    layer[0]
}

pub fn privacy_merkle_root(txs: &[PrivacyTransaction]) -> Hash {
    if txs.is_empty() {
        return Hash::zero();
    }
    let mut layer: Vec<Hash> = txs.iter().map(|tx| tx.hash()).collect();
    while layer.len() > 1 {
        let mut next = Vec::with_capacity((layer.len() + 1) / 2);
        for pair in layer.chunks(2) {
            let right = if pair.len() == 2 { &pair[1] } else { &pair[0] };
            next.push(rupeecoin_crypto::hash_concat(&pair[0], right));
        }
        layer = next;
    }
    layer[0]
}
