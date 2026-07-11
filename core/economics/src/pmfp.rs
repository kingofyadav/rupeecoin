use rupeecoin_crypto::Hash;
use rupeecoin_staking::ValidatorRegistry;
use serde::{Deserialize, Serialize};

/// Number of epochs in a 365-day rolling distribution window.
/// 365 days × 86_400 s/day / (32 slots/epoch × 4 s/slot) = 246_375 epochs.
pub const DISTRIBUTION_WINDOW_EPOCHS: u64 = 246_375;

/// Protocol-Managed Fee Pool (PMFP).
///
/// Collects 30 % of every block's base fees and redistributes the balance
/// proportionally to active validators over a 365-day rolling window.
/// Each epoch boundary triggers a `distribute_epoch` call that releases
/// `balance / remaining_window` to all active validators' claimable balances.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PmfpAccumulator {
    /// Current undistributed balance in micro-RPC.
    pub balance: u64,
    /// Lifetime total distributed (for accounting / telemetry).
    pub total_distributed: u64,
}

impl PmfpAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add newly-collected fees to the pool.
    pub fn add_fees(&mut self, amount: u64) {
        self.balance = self.balance.saturating_add(amount);
    }

    /// Amount to emit in a single epoch distribution pass.
    /// Capped so the window never extends beyond the target.
    pub fn epoch_emission(&self) -> u64 {
        if DISTRIBUTION_WINDOW_EPOCHS == 0 {
            return 0;
        }
        self.balance / DISTRIBUTION_WINDOW_EPOCHS
    }

    /// Distribute this epoch's emission to all active validators, proportional
    /// to their effective weight (stake × contribution_multiplier).
    ///
    /// Amounts are credited to each validator's `claimable_rewards` balance.
    /// Returns the total amount distributed.
    pub fn distribute_epoch(&mut self, registry: &mut ValidatorRegistry) -> u64 {
        let emission = self.epoch_emission();
        if emission == 0 {
            return 0;
        }
        let total_weight = registry.total_effective_weight();
        if total_weight == 0 {
            return 0;
        }

        let mut distributed: u64 = 0;
        let active: Vec<Hash> = registry.active_set().to_vec();
        for addr in &active {
            let weight = registry.effective_weight(addr);
            let share = (emission as u128 * weight as u128 / total_weight as u128) as u64;
            if share > 0 {
                registry.credit_rewards(addr, share);
                distributed = distributed.saturating_add(share);
            }
        }

        self.balance = self.balance.saturating_sub(distributed);
        self.total_distributed = self.total_distributed.saturating_add(distributed);
        distributed
    }
}
