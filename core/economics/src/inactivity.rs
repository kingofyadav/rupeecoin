use rupeecoin_crypto::Hash;
use rupeecoin_staking::{ValidatorRegistry, MIN_STAKE_MICRO_RPC};

/// Consecutive missed epochs before the inactivity leak begins.
pub const INACTIVITY_THRESHOLD_EPOCHS: u64 = 4;

/// Each epoch missed beyond the threshold, the validator loses
/// `stake / INACTIVITY_PENALTY_DIVISOR` from their staked balance.
/// 1/65536 ≈ 0.00153 % per missed epoch.
pub const INACTIVITY_PENALTY_DIVISOR: u64 = 65_536;

/// Apply the inactivity leak at an epoch boundary.
///
/// For each active validator whose consecutive missed epoch count exceeds
/// `INACTIVITY_THRESHOLD_EPOCHS`, subtract a stake penalty proportional to
/// the number of epochs missed beyond the threshold.
///
/// Validators whose stake falls below `MIN_STAKE_MICRO_RPC` after the penalty
/// are force-exited from the active set.
pub fn apply_inactivity_leak(registry: &mut ValidatorRegistry, current_epoch: u64) {
    let active: Vec<Hash> = registry.active_set().to_vec();

    for addr in active {
        let missed = registry.consecutive_missed_epochs(&addr);
        if missed <= INACTIVITY_THRESHOLD_EPOCHS {
            continue;
        }
        let excess_epochs = missed - INACTIVITY_THRESHOLD_EPOCHS;
        let stake = registry
            .get(&addr)
            .map(|v| v.stake)
            .unwrap_or(0);
        if stake == 0 {
            continue;
        }

        let penalty = (stake as u128 * excess_epochs as u128
            / INACTIVITY_PENALTY_DIVISOR as u128) as u64;
        if penalty == 0 {
            continue;
        }

        let new_stake = registry.apply_stake_penalty(&addr, penalty);

        // If stake fell below the minimum, force-exit the validator.
        if new_stake < MIN_STAKE_MICRO_RPC {
            let _ = registry.request_exit(&addr, current_epoch * 32);
        }
    }
}
