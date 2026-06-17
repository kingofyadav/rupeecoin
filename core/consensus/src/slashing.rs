use std::collections::HashMap;

use rupeecoin_crypto::{Hash, PublicKey, Signature};
use rupeecoin_types::{Attestation, SlashingEvidence};

/// Detects equivocation (double-proposal and double-vote) by tracking
/// first-seen proposals and attestations per slot.
pub struct SlashingDetector {
    /// slot → (block_hash, proposer_pubkey, signature)
    seen_proposals: HashMap<u64, (Hash, PublicKey, Signature)>,
    /// (slot, validator_address) → (block_hash, attestation)
    seen_attestations: HashMap<(u64, Hash), Attestation>,
}

impl SlashingDetector {
    pub fn new() -> Self {
        SlashingDetector {
            seen_proposals: HashMap::new(),
            seen_attestations: HashMap::new(),
        }
    }

    /// Record a block proposal.  Returns slashing evidence if the proposer
    /// has already proposed a different block in the same slot.
    pub fn record_proposal(
        &mut self,
        slot: u64,
        block_hash: Hash,
        proposer_pubkey: PublicKey,
        signature: Signature,
    ) -> Option<SlashingEvidence> {
        // Verify the signature is genuine before recording to prevent framing.
        if proposer_pubkey.verify(block_hash.as_bytes(), &signature).is_err() {
            return None;
        }
        if let Some((prior_hash, prior_pubkey, prior_sig)) = self.seen_proposals.get(&slot) {
            if *prior_hash != block_hash && prior_pubkey == &proposer_pubkey {
                return Some(SlashingEvidence::DoubleProposal {
                    slot,
                    block_hash_a: *prior_hash,
                    sig_a: prior_sig.clone(),
                    block_hash_b: block_hash,
                    sig_b: signature,
                    proposer_pubkey,
                });
            }
            // Same hash = duplicate gossip, not equivocation.
            return None;
        }
        self.seen_proposals
            .insert(slot, (block_hash, proposer_pubkey, signature));
        None
    }

    /// Record an attestation.  Returns slashing evidence if the validator
    /// has already attested to a different block in the same slot.
    pub fn record_attestation(&mut self, att: Attestation) -> Option<SlashingEvidence> {
        // Verify the attestation signature before recording.
        let sd = Attestation::sig_data(att.slot, &att.block_hash);
        if att.validator_pubkey.verify(sd.as_bytes(), &att.signature).is_err() {
            return None;
        }
        let addr = att.validator_pubkey.to_hash();
        let key = (att.slot, addr);
        if let Some(prior) = self.seen_attestations.get(&key) {
            if prior.block_hash != att.block_hash {
                return Some(SlashingEvidence::DoubleVote {
                    attestation_a: prior.clone(),
                    attestation_b: att,
                });
            }
            return None; // duplicate of same vote
        }
        self.seen_attestations.insert(key, att);
        None
    }

    /// Prune records for slots older than `keep_from_slot` to bound memory.
    pub fn prune(&mut self, keep_from_slot: u64) {
        self.seen_proposals
            .retain(|&slot, _| slot >= keep_from_slot);
        self.seen_attestations
            .retain(|(slot, _), _| *slot >= keep_from_slot);
    }
}

impl Default for SlashingDetector {
    fn default() -> Self {
        Self::new()
    }
}
