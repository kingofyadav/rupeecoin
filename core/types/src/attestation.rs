use rupeecoin_crypto::{hash_data, Hash, PublicKey, Signature};
use serde::{Deserialize, Serialize};

/// A validator's signed vote on a block.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct Attestation {
    pub slot: u64,
    pub block_hash: Hash,
    pub validator_pubkey: PublicKey,
    pub signature: Signature,
}

impl Attestation {
    /// The data every validator signs: BLAKE3("ATTEST" ++ slot_le ++ block_hash).
    /// The "ATTEST" prefix distinguishes attestation signatures from announcement signatures.
    pub fn sig_data(slot: u64, block_hash: &Hash) -> Hash {
        let mut buf = Vec::with_capacity(6 + 8 + 32);
        buf.extend_from_slice(b"ATTEST");
        buf.extend_from_slice(&slot.to_le_bytes());
        buf.extend_from_slice(block_hash.as_bytes());
        hash_data(&buf)
    }
}

/// Broadcast by a node when it wants to join the validator set.
/// Self-declared stake is accepted in Phase 3; on-chain verification comes later.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct ValidatorAnnouncement {
    pub pubkey: PublicKey,
    /// Self-declared stake in micro-RPC.
    pub stake: u64,
    /// Signature over BLAKE3(pubkey_bytes ++ stake_le_bytes).
    pub signature: Signature,
}

impl ValidatorAnnouncement {
    /// The data a validator signs when announcing: BLAKE3("VANN" ++ pubkey_bytes ++ stake_le).
    /// The "VANN" prefix distinguishes announcement signatures from attestation signatures.
    pub fn sig_data(pubkey: &PublicKey, stake: u64) -> Hash {
        let mut buf = Vec::with_capacity(4 + pubkey.as_bytes().len() + 8);
        buf.extend_from_slice(b"VANN");
        buf.extend_from_slice(pubkey.as_bytes());
        buf.extend_from_slice(&stake.to_le_bytes());
        hash_data(&buf)
    }
}

/// Evidence of a slashable offence.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum SlashingEvidence {
    /// Validator proposed two different blocks for the same slot.
    DoubleProposal {
        slot: u64,
        block_hash_a: Hash,
        sig_a: Signature,
        block_hash_b: Hash,
        sig_b: Signature,
        proposer_pubkey: PublicKey,
    },
    /// Validator signed two conflicting attestations for the same slot.
    DoubleVote {
        attestation_a: Attestation,
        attestation_b: Attestation,
    },
}
