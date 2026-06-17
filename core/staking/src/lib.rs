use std::collections::{HashMap, VecDeque};

use rupeecoin_crypto::{Hash, PublicKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ── Protocol constants ────────────────────────────────────────────────────────

/// Minimum stake to become a validator: 10,000 RPC in micro-RPC.
pub const MIN_STAKE_MICRO_RPC: u64 = 10_000 * 100_000_000;

/// Maximum active validators in the initial protocol version.
pub const MAX_ACTIVE_VALIDATORS: usize = 10_000;

/// Blocks a validator must wait after requesting exit before stake is returned.
/// 28 days at 4s/block = 604,800 blocks.
pub const UNSTAKING_DELAY_BLOCKS: u64 = 604_800;

// ── Contribution weights (sum = 1.0) ─────────────────────────────────────────

const W_UPTIME: f64 = 0.3;
const W_ATTESTATION: f64 = 0.4;
const W_PROPOSAL: f64 = 0.2;
const W_PROPAGATION: f64 = 0.1;

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ValidatorStatus {
    Active,
    Queued,
    Exiting { exit_at_block: u64 },
    Slashed,
}

/// Rolling contribution metrics over the last ~1000 epochs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContributionMetrics {
    /// Slots assigned as proposer.
    pub slots_assigned: u64,
    /// Slots where the validator produced a timely block.
    pub slots_timely: u64,
    /// Attestation duties assigned.
    pub attestations_assigned: u64,
    /// Attestation duties fulfilled on time.
    pub attestations_timely: u64,
    /// Propagation score: subjective \[0,1\], updated by peers (simplified).
    pub propagation_score: f64,
}

impl ContributionMetrics {
    pub fn new() -> Self {
        ContributionMetrics {
            slots_assigned: 0,
            slots_timely: 0,
            attestations_assigned: 0,
            attestations_timely: 0,
            propagation_score: 1.0, // full score until we observe otherwise
        }
    }

    pub fn score(&self) -> f64 {
        let att_rate = if self.attestations_assigned == 0 {
            1.0
        } else {
            self.attestations_timely as f64 / self.attestations_assigned as f64
        };
        // uptime: attestation-based liveness (previously duplicated proposal; now distinct)
        let uptime = att_rate;
        let attest = att_rate;
        // proposal: block-production rate when scheduled as proposer
        let proposal = if self.slots_assigned == 0 {
            1.0
        } else {
            self.slots_timely as f64 / self.slots_assigned as f64
        };
        let propagation = self.propagation_score;

        (W_UPTIME * uptime
            + W_ATTESTATION * attest
            + W_PROPOSAL * proposal
            + W_PROPAGATION * propagation)
            .clamp(0.0, 1.0)
    }

    /// Maps contribution score ∈ \[0,1\] to multiplier ∈ \[0.5, 1.5\].
    pub fn multiplier(&self) -> f64 {
        piecewise_multiplier(self.score())
    }
}

impl Default for ContributionMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Piecewise-linear multiplier matching the whitepaper table:
/// score 1.0 → 1.5 | 0.8 → 1.1 | 0.5 → 1.0 | 0.2 → 0.5 | 0.0 → 0.5
fn piecewise_multiplier(score: f64) -> f64 {
    if score >= 1.0 {
        1.5
    } else if score >= 0.8 {
        1.1 + (score - 0.8) / 0.2 * 0.4
    } else if score >= 0.5 {
        1.0 + (score - 0.5) / 0.3 * 0.1
    } else if score >= 0.2 {
        0.5 + (score - 0.2) / 0.3 * 0.5
    } else {
        0.5
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidatorInfo {
    pub pubkey: PublicKey,
    /// Address = BLAKE3(pubkey).
    pub address: Hash,
    /// Stake in micro-RPC.
    pub stake: u64,
    pub status: ValidatorStatus,
    pub registered_at_block: u64,
    pub contribution: ContributionMetrics,
    // ── Phase 6: economics ────────────────────────────────────────────────────
    /// Accumulated rewards not yet claimed by the validator.
    pub claimable_rewards: u64,
    /// Consecutive epochs in which the validator had zero participation.
    pub consecutive_missed_epochs: u64,
    /// Epoch number of the most recent attestation duty fulfilled.
    pub last_attested_epoch: Option<u64>,
}

impl ValidatorInfo {
    /// Effective weight = stake × contribution_multiplier.
    ///
    /// The multiplier is computed in f64 (acceptable: always in [0.5, 1.5] with
    /// limited precision), then scaled ×1000 and converted to u64.  The final
    /// multiplication uses u128 to avoid precision loss for large stakes
    /// (stakes up to ~18 × 10^18 micro-RPC fit in u128 × 1500 / 1000).
    pub fn effective_weight(&self) -> u64 {
        let mult_1000 = (self.contribution.multiplier() * 1000.0).round() as u64;
        ((self.stake as u128 * mult_1000 as u128) / 1000) as u64
    }
}

// ── Registry ──────────────────────────────────────────────────────────────────

#[derive(Error, Debug)]
pub enum StakingError {
    #[error("stake {0} is below minimum {MIN_STAKE_MICRO_RPC}")]
    BelowMinimum(u64),
    #[error("validator already registered")]
    AlreadyRegistered,
    #[error("validator not found")]
    NotFound,
    #[error("validator is not active")]
    NotActive,
    #[error("validator is already slashed")]
    AlreadySlashed,
    #[error("validator is already exiting")]
    AlreadyExiting,
    #[error("insufficient claimable rewards: available {available}, requested {requested}")]
    InsufficientRewards { available: u64, requested: u64 },
}

#[derive(Serialize, Deserialize)]
pub struct ValidatorRegistry {
    /// All known validators keyed by their address.
    validators: HashMap<Hash, ValidatorInfo>,
    /// Ordered active set (by registration time, bounded to MAX_ACTIVE_VALIDATORS).
    active_set: Vec<Hash>,
    /// Queue of validators waiting to be activated.
    queue: VecDeque<Hash>,
}

impl ValidatorRegistry {
    pub fn new() -> Self {
        ValidatorRegistry {
            validators: HashMap::new(),
            active_set: Vec::new(),
            queue: VecDeque::new(),
        }
    }

    // ── Registration ─────────────────────────────────────────────────────────

    pub fn register(
        &mut self,
        pubkey: PublicKey,
        stake: u64,
        current_block: u64,
    ) -> Result<(), StakingError> {
        if stake < MIN_STAKE_MICRO_RPC {
            return Err(StakingError::BelowMinimum(stake));
        }
        let address = pubkey.to_hash();
        if self.validators.contains_key(&address) {
            return Err(StakingError::AlreadyRegistered);
        }

        let info = ValidatorInfo {
            pubkey,
            address,
            stake,
            status: ValidatorStatus::Queued,
            registered_at_block: current_block,
            contribution: ContributionMetrics::new(),
            claimable_rewards: 0,
            consecutive_missed_epochs: 0,
            last_attested_epoch: None,
        };
        self.validators.insert(address, info);
        self.queue.push_back(address);
        self.activate_queued();
        Ok(())
    }

    /// Promote queued validators until the active set is full.
    ///
    /// The active set is kept sorted by address so that all nodes with the
    /// same validator composition produce identical proposer schedules,
    /// regardless of local registration order.
    fn activate_queued(&mut self) {
        while self.active_set.len() < MAX_ACTIVE_VALIDATORS {
            match self.queue.pop_front() {
                Some(addr) => {
                    if let Some(v) = self.validators.get_mut(&addr) {
                        v.status = ValidatorStatus::Active;
                    }
                    let pos = self.active_set.partition_point(|&a| a < addr);
                    self.active_set.insert(pos, addr);
                }
                None => break,
            }
        }
    }

    // ── Exit / Unstake ───────────────────────────────────────────────────────

    pub fn request_exit(&mut self, address: &Hash, current_block: u64) -> Result<(), StakingError> {
        let v = self
            .validators
            .get_mut(address)
            .ok_or(StakingError::NotFound)?;
        match v.status {
            ValidatorStatus::Active => {
                v.status = ValidatorStatus::Exiting {
                    exit_at_block: current_block + UNSTAKING_DELAY_BLOCKS,
                };
                self.active_set.retain(|a| a != address);
                self.activate_queued();
                Ok(())
            }
            ValidatorStatus::Exiting { .. } => Err(StakingError::AlreadyExiting),
            ValidatorStatus::Slashed => Err(StakingError::AlreadySlashed),
            ValidatorStatus::Queued => {
                v.status = ValidatorStatus::Exiting {
                    exit_at_block: current_block + UNSTAKING_DELAY_BLOCKS,
                };
                self.queue.retain(|a| a != address);
                Ok(())
            }
        }
    }

    /// Remove validators whose unstaking delay has elapsed.
    pub fn process_exits(&mut self, current_block: u64) {
        let to_remove: Vec<Hash> = self
            .validators
            .iter()
            .filter(|(_, v)| matches!(v.status, ValidatorStatus::Exiting { exit_at_block: b } if current_block >= b))
            .map(|(a, _)| *a)
            .collect();
        for addr in to_remove {
            self.validators.remove(&addr);
        }
    }

    // ── Slashing ─────────────────────────────────────────────────────────────

    /// Slash a validator: zero their stake and remove from active set.
    /// Returns the slashed amount.
    pub fn slash(&mut self, address: &Hash) -> Result<u64, StakingError> {
        let v = self
            .validators
            .get_mut(address)
            .ok_or(StakingError::NotFound)?;
        if v.status == ValidatorStatus::Slashed {
            return Err(StakingError::AlreadySlashed);
        }
        let slashed = v.stake;
        v.stake = 0;
        v.status = ValidatorStatus::Slashed;
        self.active_set.retain(|a| a != address);
        self.queue.retain(|a| a != address);
        self.activate_queued();
        Ok(slashed)
    }

    // ── Contribution tracking ────────────────────────────────────────────────

    pub fn record_proposal(&mut self, address: &Hash, timely: bool) {
        if let Some(v) = self.validators.get_mut(address) {
            v.contribution.slots_assigned += 1;
            if timely {
                v.contribution.slots_timely += 1;
            }
        }
    }

    pub fn record_attestation_duty(&mut self, address: &Hash, fulfilled: bool) {
        if let Some(v) = self.validators.get_mut(address) {
            v.contribution.attestations_assigned += 1;
            if fulfilled {
                v.contribution.attestations_timely += 1;
            }
        }
    }

    // ── Queries ──────────────────────────────────────────────────────────────

    pub fn active_set(&self) -> &[Hash] {
        &self.active_set
    }

    pub fn active_count(&self) -> usize {
        self.active_set.len()
    }

    pub fn get(&self, address: &Hash) -> Option<&ValidatorInfo> {
        self.validators.get(address)
    }

    pub fn total_active_stake(&self) -> u64 {
        self.active_set
            .iter()
            .filter_map(|a| self.validators.get(a))
            .map(|v| v.stake)
            .sum()
    }

    pub fn effective_weight(&self, address: &Hash) -> u64 {
        self.validators
            .get(address)
            .map(|v| v.effective_weight())
            .unwrap_or(0)
    }

    pub fn iter_active(&self) -> impl Iterator<Item = &ValidatorInfo> {
        self.active_set
            .iter()
            .filter_map(|a| self.validators.get(a))
    }

    pub fn is_empty(&self) -> bool {
        self.active_set.is_empty()
    }

    /// Sum of effective weights (stake × contribution_multiplier) across the entire
    /// active set.  Used for proportional reward distribution.
    pub fn total_effective_weight(&self) -> u64 {
        self.active_set
            .iter()
            .filter_map(|a| self.validators.get(a))
            .map(|v| v.effective_weight())
            .sum()
    }

    // ── Phase 6: economics ────────────────────────────────────────────────────

    /// Withdraw `amount` from a validator's claimable rewards.
    pub fn withdraw_rewards(&mut self, address: &Hash, amount: u64) -> Result<(), StakingError> {
        let v = self.validators.get_mut(address).ok_or(StakingError::NotFound)?;
        if v.claimable_rewards < amount {
            return Err(StakingError::InsufficientRewards {
                available: v.claimable_rewards,
                requested: amount,
            });
        }
        v.claimable_rewards -= amount;
        Ok(())
    }

    /// Add `amount` to a validator's claimable rewards balance.
    pub fn credit_rewards(&mut self, address: &Hash, amount: u64) {
        if let Some(v) = self.validators.get_mut(address) {
            v.claimable_rewards = v.claimable_rewards.saturating_add(amount);
        }
    }

    /// Subtract `penalty` from a validator's stake.  Returns the new stake.
    pub fn apply_stake_penalty(&mut self, address: &Hash, penalty: u64) -> u64 {
        if let Some(v) = self.validators.get_mut(address) {
            v.stake = v.stake.saturating_sub(penalty);
            return v.stake;
        }
        0
    }

    /// Record that `address` attested in `epoch`.  Resets the consecutive-missed
    /// counter; used by the inactivity-leak logic.
    pub fn mark_attested(&mut self, address: &Hash, epoch: u64) {
        if let Some(v) = self.validators.get_mut(address) {
            v.last_attested_epoch = Some(epoch);
            v.consecutive_missed_epochs = 0;
        }
    }

    /// How many consecutive epochs has `address` missed?
    pub fn consecutive_missed_epochs(&self, address: &Hash) -> u64 {
        self.validators
            .get(address)
            .map(|v| v.consecutive_missed_epochs)
            .unwrap_or(0)
    }

    /// Called at each epoch boundary.  For every active validator that has not
    /// attested in `current_epoch - 1` (i.e. `last_attested_epoch < current_epoch`),
    /// increments their consecutive-missed counter.  Validators that attested get
    /// their counter reset.
    pub fn process_epoch_participation(&mut self, current_epoch: u64) {
        let active: Vec<Hash> = self.active_set.clone();
        for addr in active {
            if let Some(v) = self.validators.get_mut(&addr) {
                let attested_this_epoch = v
                    .last_attested_epoch
                    .map(|e| e >= current_epoch.saturating_sub(1))
                    .unwrap_or(false);
                if attested_this_epoch {
                    v.consecutive_missed_epochs = 0;
                } else {
                    v.consecutive_missed_epochs =
                        v.consecutive_missed_epochs.saturating_add(1);
                }
            }
        }
    }
}

impl Default for ValidatorRegistry {
    fn default() -> Self {
        Self::new()
    }
}
