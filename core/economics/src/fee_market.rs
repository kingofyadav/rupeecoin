/// Number of non-coinbase transactions targeted per block (50 % of limit).
pub const GAS_TARGET: u64 = 500;

/// Maximum non-coinbase transactions per block.
pub const GAS_LIMIT: u64 = 1_000;

/// Starting base fee in micro-RPC per transaction.
pub const INITIAL_BASE_FEE: u64 = 1_000;

/// Floor: base fee cannot drop below this.
pub const MIN_BASE_FEE: u64 = 100;

/// Max per-block change fraction denominator: ±1/8 = ±12.5 %.
const CHANGE_DENOM: u64 = 8;

/// Compute the base fee for the block that follows one with `gas_used` transactions
/// and `current_base_fee`.
///
/// EIP-1559 formula (integer arithmetic via u128 intermediates):
///   over  target → fee += fee × (gas_used − target) / target / 8  (at least +1)
///   under target → fee -= fee × (target − gas_used) / target / 8
///   at    target → unchanged
///   floor: MIN_BASE_FEE
pub fn next_base_fee(current_base_fee: u64, gas_used: u64) -> u64 {
    let gas_used = gas_used.min(GAS_LIMIT);
    if gas_used == GAS_TARGET {
        return current_base_fee;
    }
    let new_fee: u64 = if gas_used > GAS_TARGET {
        let excess = (gas_used - GAS_TARGET) as u128;
        let increase = (current_base_fee as u128 * excess
            / GAS_TARGET as u128
            / CHANGE_DENOM as u128) as u64;
        current_base_fee.saturating_add(increase.max(1))
    } else {
        let deficit = (GAS_TARGET - gas_used) as u128;
        let decrease = (current_base_fee as u128 * deficit
            / GAS_TARGET as u128
            / CHANGE_DENOM as u128) as u64;
        current_base_fee.saturating_sub(decrease)
    };
    new_fee.max(MIN_BASE_FEE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn at_target_unchanged() {
        assert_eq!(next_base_fee(1_000, GAS_TARGET), 1_000);
    }

    #[test]
    fn full_block_increases_fee() {
        let fee = next_base_fee(1_000, GAS_LIMIT);
        assert!(fee > 1_000, "fee should increase: got {fee}");
        // Max increase: 1_000 * 500/500/8 = 125 → 1_125
        assert_eq!(fee, 1_125);
    }

    #[test]
    fn empty_block_decreases_fee() {
        let fee = next_base_fee(1_000, 0);
        assert!(fee < 1_000, "fee should decrease: got {fee}");
        // Decrease: 1_000 * 500/500/8 = 125 → 875
        assert_eq!(fee, 875);
    }

    #[test]
    fn fee_never_below_minimum() {
        let fee = next_base_fee(MIN_BASE_FEE, 0);
        assert_eq!(fee, MIN_BASE_FEE);
    }

    proptest! {
        /// The floor invariant: base fee must never drop below MIN_BASE_FEE.
        #[test]
        fn base_fee_floor_invariant(
            current in MIN_BASE_FEE..u64::MAX / 2,
            gas_used in 0u64..=GAS_LIMIT,
        ) {
            let next = next_base_fee(current, gas_used);
            prop_assert!(next >= MIN_BASE_FEE, "base fee {next} < MIN_BASE_FEE {MIN_BASE_FEE}");
        }

        /// Over-target blocks always produce a strictly higher fee (when there
        /// is room to increase above MIN_BASE_FEE).
        #[test]
        fn over_target_increases_fee(
            current in MIN_BASE_FEE + 1..u64::MAX / 2,
            gas_used in GAS_TARGET + 1..=GAS_LIMIT,
        ) {
            let next = next_base_fee(current, gas_used);
            prop_assert!(next > current, "expected increase: {current} → {next} at gas_used={gas_used}");
        }

        /// Under-target blocks never increase the fee.
        #[test]
        fn under_target_does_not_increase_fee(
            current in MIN_BASE_FEE..u64::MAX / 2,
            gas_used in 0u64..GAS_TARGET,
        ) {
            let next = next_base_fee(current, gas_used);
            prop_assert!(next <= current, "unexpected increase: {current} → {next}");
        }

        /// At-target gas, fee is always unchanged.
        #[test]
        fn at_target_fee_unchanged(current in MIN_BASE_FEE..u64::MAX / 2) {
            prop_assert_eq!(next_base_fee(current, GAS_TARGET), current);
        }

        /// Gas capping: values above GAS_LIMIT are treated as GAS_LIMIT.
        #[test]
        fn excess_gas_capped_at_limit(
            current in MIN_BASE_FEE..u64::MAX / 2,
            gas_extra in 1u64..10_000,
        ) {
            let fee_at_limit    = next_base_fee(current, GAS_LIMIT);
            let fee_above_limit = next_base_fee(current, GAS_LIMIT + gas_extra);
            prop_assert_eq!(fee_at_limit, fee_above_limit);
        }

        /// Function is deterministic.
        #[test]
        fn fee_is_deterministic(current in MIN_BASE_FEE..u64::MAX / 2, gas in 0u64..=GAS_LIMIT) {
            prop_assert_eq!(next_base_fee(current, gas), next_base_fee(current, gas));
        }
    }
}
