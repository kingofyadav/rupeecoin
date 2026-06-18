use rupeecoin_types::MICRO_RPC_PER_COIN;

pub const INITIAL_BLOCK_REWARD: u64 = 50 * MICRO_RPC_PER_COIN;

// Block heights at which the reward halves (365-day year at 4 s/block).
pub const HALVING_1: u64 = 31_536_000;  // Year  4
pub const HALVING_2: u64 = 63_072_000;  // Year  8
pub const HALVING_3: u64 = 94_608_000;  // Year 12
pub const HALVING_4: u64 = 126_144_000; // Year 16

// Fixed tail emission after the final halving: 1 RPC per block.
pub const TAIL_EMISSION: u64 = MICRO_RPC_PER_COIN;

/// Block reward (in micro-RPC) at a given chain height.
///
/// Schedule:
///   Era 0 (0 …31 535 999):    50   RPC
///   Era 1 (31 536 000 …):     25   RPC
///   Era 2 (63 072 000 …):     12.5 RPC
///   Era 3+ (94 608 000 …):     6.25 RPC, then 1 RPC tail emission after halving 4
pub fn block_reward(height: u64) -> u64 {
    if height >= HALVING_4 {
        TAIL_EMISSION
    } else if height >= HALVING_3 {
        INITIAL_BLOCK_REWARD / 8   // 625_000_000
    } else if height >= HALVING_2 {
        INITIAL_BLOCK_REWARD / 4   // 1_250_000_000
    } else if height >= HALVING_1 {
        INITIAL_BLOCK_REWARD / 2   // 2_500_000_000
    } else {
        INITIAL_BLOCK_REWARD       // 5_000_000_000
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn halving_schedule() {
        assert_eq!(block_reward(0), INITIAL_BLOCK_REWARD);
        assert_eq!(block_reward(HALVING_1 - 1), INITIAL_BLOCK_REWARD);
        assert_eq!(block_reward(HALVING_1), INITIAL_BLOCK_REWARD / 2);
        assert_eq!(block_reward(HALVING_2), INITIAL_BLOCK_REWARD / 4);
        assert_eq!(block_reward(HALVING_3), INITIAL_BLOCK_REWARD / 8);
        assert_eq!(block_reward(HALVING_4), TAIL_EMISSION);
        assert_eq!(block_reward(200_000_000), TAIL_EMISSION);
    }

    proptest! {
        /// The reward schedule is monotone non-increasing: later blocks never
        /// receive more than earlier blocks.
        #[test]
        fn reward_is_monotone_non_increasing(h in 0u64..200_000_001u64) {
            let next = h.saturating_add(1);
            prop_assert!(block_reward(h) >= block_reward(next));
        }

        /// Every block always produces a positive reward (tail emission never
        /// drops to zero).
        #[test]
        fn reward_is_always_positive(h in 0u64..200_000_001u64) {
            prop_assert!(block_reward(h) > 0, "block_reward({h}) = 0");
        }

        /// Reward is always bounded by the initial value.
        #[test]
        fn reward_never_exceeds_initial(h in 0u64..200_000_001u64) {
            prop_assert!(block_reward(h) <= INITIAL_BLOCK_REWARD);
        }

        /// Reward never drops below TAIL_EMISSION.
        #[test]
        fn reward_never_below_tail(h in 0u64..200_000_001u64) {
            prop_assert!(block_reward(h) >= TAIL_EMISSION);
        }

        /// Function is deterministic.
        #[test]
        fn reward_is_deterministic(h in 0u64..200_000_001u64) {
            prop_assert_eq!(block_reward(h), block_reward(h));
        }
    }
}
