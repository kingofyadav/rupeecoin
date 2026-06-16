pub mod checkpoints;
pub mod finality;
pub mod slashing;

use std::collections::HashMap;

use rupeecoin_crypto::{hash_data, Hash, PublicKey, VrfProof};
use rupeecoin_staking::ValidatorRegistry;
use rupeecoin_types::{Attestation, Block, SlashingEvidence};
use thiserror::Error;

pub use checkpoints::{CheckpointManager, WeakSubjectivityCheckpoint, CHECKPOINT_INTERVAL};
pub use finality::FinalityTracker;
pub use slashing::SlashingDetector;

// ── Protocol constants ────────────────────────────────────────────────────────

/// Seconds per slot (4 s as per whitepaper).
pub const SLOT_DURATION_SECS: u64 = 4;

/// Slots per epoch (32 as per whitepaper).
pub const SLOTS_PER_EPOCH: u64 = 32;

/// Justification threshold: 2/3 of attesting stake.
/// Stored as numerator of a fraction with denominator 3.
const JUSTIFY_NUMERATOR: u64 = 2;
const JUSTIFY_DENOMINATOR: u64 = 3;

// ── Slot / epoch helpers ──────────────────────────────────────────────────────

pub fn slot_to_epoch(slot: u64) -> u64 {
    slot / SLOTS_PER_EPOCH
}

pub fn epoch_start_slot(epoch: u64) -> u64 {
    epoch * SLOTS_PER_EPOCH
}

// ── Proposer selection ────────────────────────────────────────────────────────

/// VRF-chained proposer selection.
///
/// The proposer index for slot `slot` is derived from the *previous* block's
/// VRF output rather than its hash.  This chains the outputs so future
/// proposers are unpredictable until the VRF proof for each slot is revealed.
///
/// `prev_vrf_output` should be `tip.header.vrf_output` when calling from the
/// node; use `Hash::zero()` for the very first block after genesis.
pub fn select_proposer(slot: u64, prev_vrf_output: &Hash, active_count: usize) -> usize {
    if active_count == 0 {
        return 0;
    }
    let mut buf = [0u8; 40];
    buf[..32].copy_from_slice(prev_vrf_output.as_bytes());
    buf[32..].copy_from_slice(&slot.to_le_bytes());
    let seed = hash_data(&buf);
    let idx = u64::from_le_bytes(seed.as_bytes()[..8].try_into().unwrap());
    (idx % active_count as u64) as usize
}

/// Verify the VRF proof embedded in a block.
///
/// Returns `true` when the proof is absent (bootstrap / no validators) OR
/// when it correctly proves `block.header.vrf_output` was derived from the
/// proposer's key.
pub fn verify_block_vrf(
    block: &Block,
    proposer_pubkey: &PublicKey,
    prev_vrf_output: &Hash,
) -> bool {
    match &block.vrf_proof {
        None => true, // bootstrap mode — no proof required
        Some(sig) => {
            let proof = VrfProof {
                signature: sig.clone(),
                output: block.header.vrf_output,
            };
            proof.verify(proposer_pubkey, block.header.slot, prev_vrf_output)
        }
    }
}

// ── Attestation pool ──────────────────────────────────────────────────────────

#[derive(Error, Debug)]
pub enum ConsensusError {
    #[error("invalid attestation signature from {0:?}")]
    InvalidAttestationSig(PublicKey),
    #[error("attestation from unknown validator {0:?}")]
    UnknownValidator(Hash),
    #[error("duplicate attestation for slot {0}")]
    DuplicateAttestation(u64),
}

/// Tracks attestations for each slot and determines when blocks are justified.
pub struct AttestationPool {
    /// slot → block_hash → map of (validator_address → stake)
    votes: HashMap<u64, HashMap<Hash, HashMap<Hash, u64>>>,
}

impl AttestationPool {
    pub fn new() -> Self {
        AttestationPool {
            votes: HashMap::new(),
        }
    }

    /// Validate and record an attestation.
    pub fn add(
        &mut self,
        att: Attestation,
        registry: &ValidatorRegistry,
    ) -> Result<(), ConsensusError> {
        let addr = att.validator_pubkey.to_hash();

        // Validator must be in the active set.
        if registry.get(&addr).is_none() {
            return Err(ConsensusError::UnknownValidator(addr));
        }

        // Verify the attestation signature.
        let sig_data = Attestation::sig_data(att.slot, &att.block_hash);
        att.validator_pubkey
            .verify(sig_data.as_bytes(), &att.signature)
            .map_err(|_| ConsensusError::InvalidAttestationSig(att.validator_pubkey.clone()))?;

        // Prevent double-voting: first vote for a slot is accepted; any subsequent
        // vote from the same validator is a duplicate (equivocation is the slashing
        // detector's concern, not ours here).
        let slot_votes = self.votes.entry(att.slot).or_default();
        for (_, voters) in slot_votes.iter() {
            if voters.contains_key(&addr) {
                return Err(ConsensusError::DuplicateAttestation(att.slot));
            }
        }

        let stake = registry.get(&addr).map(|v| v.stake).unwrap_or(0);
        slot_votes
            .entry(att.block_hash)
            .or_default()
            .insert(addr, stake);

        Ok(())
    }

    /// Stake that has attested for `block_hash` at `slot`.
    pub fn attesting_stake(&self, slot: u64, block_hash: &Hash) -> u64 {
        self.votes
            .get(&slot)
            .and_then(|m| m.get(block_hash))
            .map(|voters| voters.values().sum())
            .unwrap_or(0)
    }

    /// Whether `block_hash` at `slot` has reached 2/3 supermajority.
    pub fn is_justified(&self, slot: u64, block_hash: &Hash, total_stake: u64) -> bool {
        if total_stake == 0 {
            return false;
        }
        let att = self.attesting_stake(slot, block_hash);
        att * JUSTIFY_DENOMINATOR >= total_stake * JUSTIFY_NUMERATOR
    }

    /// Prune attestations for slots older than `keep_from_slot`.
    pub fn prune(&mut self, keep_from_slot: u64) {
        self.votes.retain(|&slot, _| slot >= keep_from_slot);
    }
}

impl Default for AttestationPool {
    fn default() -> Self {
        Self::new()
    }
}

// ── Top-level consensus state ─────────────────────────────────────────────────

pub struct ConsensusState {
    pub attestation_pool: AttestationPool,
    pub finality: FinalityTracker,
    pub slashing_detector: SlashingDetector,
    /// LMD-GHOST: latest attestation (slot, block_hash) per validator address.
    pub latest_messages: HashMap<Hash, (u64, Hash)>,
    /// LMD-GHOST: known block → parent mapping (for ancestor traversal).
    pub known_block_parents: HashMap<Hash, Hash>,
    /// LMD-GHOST: known block → child blocks mapping.
    pub known_block_children: HashMap<Hash, Vec<Hash>>,
}

impl ConsensusState {
    pub fn new() -> Self {
        ConsensusState {
            attestation_pool: AttestationPool::new(),
            finality: FinalityTracker::new(),
            slashing_detector: SlashingDetector::new(),
            latest_messages: HashMap::new(),
            known_block_parents: HashMap::new(),
            known_block_children: HashMap::new(),
        }
    }

    /// Process a new accepted block — update finality, prune old attestations,
    /// and register the block in the LMD-GHOST block tree.
    pub fn on_block_accepted(&mut self, block: &Block, registry: &ValidatorRegistry) {
        let slot = block.header.slot;
        let block_hash = block.hash();
        let total_stake = registry.total_active_stake();

        // Register in LMD-GHOST block tree.
        self.known_block_parents.insert(block_hash, block.header.prev_hash);
        self.known_block_children
            .entry(block.header.prev_hash)
            .or_default()
            .push(block_hash);

        // Record embedded attestations.
        for att in &block.attestations {
            let addr = att.validator_pubkey.to_hash();
            let entry = self.latest_messages.entry(addr).or_insert((0, block_hash));
            if att.slot > entry.0 {
                *entry = (att.slot, att.block_hash);
            }
            let _ = self.attestation_pool.add(att.clone(), registry);
        }

        // Check if this slot's block is now justified.
        if self
            .attestation_pool
            .is_justified(slot, &block_hash, total_stake)
        {
            let epoch = slot_to_epoch(slot);
            self.finality.justify(epoch, slot, block_hash);
        }

        // Prune attestations older than one finalized epoch.
        let prune_from = self
            .finality
            .last_finalized_epoch()
            .saturating_sub(1)
            .saturating_mul(SLOTS_PER_EPOCH);
        self.attestation_pool.prune(prune_from);
    }

    /// Add an out-of-block attestation (from the network) and return whether it
    /// caused justification of a new epoch.
    pub fn add_attestation(
        &mut self,
        att: Attestation,
        slot: u64,
        block_hash: &Hash,
        registry: &ValidatorRegistry,
    ) -> Result<bool, ConsensusError> {
        // Update LMD-GHOST latest message for this validator.
        let addr = att.validator_pubkey.to_hash();
        let entry = self.latest_messages.entry(addr).or_insert((0, *block_hash));
        if slot > entry.0 {
            *entry = (slot, *block_hash);
        }

        self.attestation_pool.add(att, registry)?;
        let total_stake = registry.total_active_stake();
        if self
            .attestation_pool
            .is_justified(slot, block_hash, total_stake)
        {
            let epoch = slot_to_epoch(slot);
            let newly_justified = self.finality.justify(epoch, slot, *block_hash);
            return Ok(newly_justified);
        }
        Ok(false)
    }

    /// Check for slashing evidence in a block proposal and record for future detection.
    pub fn check_and_record_proposal(
        &mut self,
        slot: u64,
        block_hash: Hash,
        proposer_pubkey: &PublicKey,
        signature: &rupeecoin_crypto::Signature,
    ) -> Option<SlashingEvidence> {
        self.slashing_detector.record_proposal(
            slot,
            block_hash,
            proposer_pubkey.clone(),
            signature.clone(),
        )
    }

    /// Register a fork block in the LMD-GHOST block tree without running
    /// attestation validation.  Called for blocks that arrived on an alternative
    /// fork so `lmd_select_head` can consider them when choosing the heaviest path.
    pub fn register_fork_block(&mut self, block_hash: Hash, parent_hash: Hash) {
        self.known_block_parents.entry(block_hash).or_insert(parent_hash);
        self.known_block_children
            .entry(parent_hash)
            .or_default()
            .push(block_hash);
    }

    // ── LMD-GHOST ─────────────────────────────────────────────────────────────

    /// Select the canonical chain head using LMD-GHOST starting from `start`.
    ///
    /// At each fork point, the child with the most "latest message" stake wins.
    /// Returns the deepest block reachable from `start` following the heaviest path.
    pub fn lmd_select_head(&self, start: Hash, registry: &ValidatorRegistry) -> Hash {
        let mut head = start;
        loop {
            let children = match self.known_block_children.get(&head) {
                Some(c) if !c.is_empty() => c,
                _ => break,
            };
            if children.len() == 1 {
                head = children[0];
                continue;
            }
            // Multiple children: pick the one whose subtree has the most latest-message stake.
            let mut best = children[0];
            let mut best_weight = self.subtree_weight(children[0], registry);
            for &child in &children[1..] {
                let w = self.subtree_weight(child, registry);
                if w > best_weight {
                    best_weight = w;
                    best = child;
                }
            }
            head = best;
        }
        head
    }

    /// Sum of stakes of validators whose latest message is in the subtree rooted at `root`.
    fn subtree_weight(&self, root: Hash, registry: &ValidatorRegistry) -> u64 {
        self.latest_messages
            .iter()
            .filter(|(_, (_, voted))| self.is_in_subtree(*voted, root))
            .map(|(addr, _)| registry.get(addr).map(|v| v.stake).unwrap_or(0))
            .sum()
    }

    /// Return true if `hash` is `root` or a descendant of `root`.
    fn is_in_subtree(&self, mut hash: Hash, root: Hash) -> bool {
        loop {
            if hash == root {
                return true;
            }
            match self.known_block_parents.get(&hash) {
                Some(&parent) => hash = parent,
                None => return false,
            }
        }
    }
}

impl Default for ConsensusState {
    fn default() -> Self {
        Self::new()
    }
}
