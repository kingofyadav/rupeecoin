use std::sync::OnceLock;

use rupeecoin_crypto::{hash_data, Address, Hash};
use rupeecoin_staking::ValidatorRegistry;

use crate::supply::block_reward;

// ── Protocol-development fund (PDF) ──────────────────────────────────────────

/// 5 % of every block reward goes to the treasury address.
pub const PDF_PERCENT: u64 = 5;

// ── Fee routing ───────────────────────────────────────────────────────────────

/// 70 % of collected base fees are burned (sent to zero address).
pub const FEE_BURN_PERCENT: u64 = 70;

/// 30 % of collected base fees flow into the PMFP accumulator.
pub const FEE_PMFP_PERCENT: u64 = 30;

// ── Treasury address ──────────────────────────────────────────────────────────

static TREASURY_OVERRIDE: OnceLock<Address> = OnceLock::new();

/// Canonical treasury address: first 20 bytes of BLAKE3("rupeecoin-treasury-v1").
/// Returns the override set by `set_treasury_address()` if one has been set.
pub fn treasury_address() -> Address {
    if let Some(addr) = TREASURY_OVERRIDE.get() {
        return *addr;
    }
    static DEFAULT: OnceLock<Address> = OnceLock::new();
    *DEFAULT.get_or_init(|| {
        let h = hash_data(b"rupeecoin-treasury-v1");
        let mut arr = [0u8; 20];
        arr.copy_from_slice(&h.as_bytes()[..20]);
        Address(arr)
    })
}

/// Override the treasury address before the node starts.
///
/// Must be called at most once before any block is processed; subsequent
/// calls are ignored.  Used by the `--treasury-address` CLI flag.
pub fn set_treasury_address(addr: Address) {
    let _ = TREASURY_OVERRIDE.set(addr);
}

// ── Reward split ──────────────────────────────────────────────────────────────

/// Per-block reward distribution computed before building a coinbase transaction.
#[derive(Debug, Clone)]
pub struct BlockRewardSplit {
    /// Proposer's proportional share — goes directly in coinbase output 0.
    pub proposer_reward: u64,
    /// PDF amount (5 % of gross) — goes in coinbase output 1.
    pub treasury_amount: u64,
    /// Each other active validator's share (address → amount).
    /// These are credited to `claimable_rewards` rather than appearing in the
    /// coinbase transaction; a future phase will add on-chain claims.
    pub validator_credits: Vec<(Hash, u64)>,
}

/// Compute the block reward split for `height`, given the current registry and
/// the proposer's address.
pub fn compute_block_rewards(
    height: u64,
    registry: &ValidatorRegistry,
    proposer_addr: &Hash,
) -> BlockRewardSplit {
    let gross = block_reward(height);
    let treasury_amount = gross * PDF_PERCENT / 100;
    let distributable = gross.saturating_sub(treasury_amount);

    let total_weight = registry.total_effective_weight();
    if total_weight == 0 {
        // Bootstrap: proposer gets everything distributable, no other validators.
        return BlockRewardSplit {
            proposer_reward: distributable,
            treasury_amount,
            validator_credits: vec![],
        };
    }

    let proposer_weight = registry.effective_weight(proposer_addr);
    let proposer_reward = (distributable as u128 * proposer_weight as u128
        / total_weight as u128) as u64;

    let mut credits = Vec::new();
    let mut credited_sum: u64 = 0;
    for v in registry.iter_active() {
        if v.address == *proposer_addr {
            continue;
        }
        let share =
            (distributable as u128 * v.effective_weight() as u128 / total_weight as u128) as u64;
        if share > 0 {
            credits.push((v.address, share));
            credited_sum = credited_sum.saturating_add(share);
        }
    }

    // Dust correction: any rounding remainder goes to the proposer.
    // Use saturating_add to prevent overflow when summing proposer + credits.
    let total_distributed = proposer_reward.saturating_add(credited_sum);
    let dust = distributable.saturating_sub(total_distributed);
    let proposer_reward = proposer_reward.saturating_add(dust);

    BlockRewardSplit {
        proposer_reward,
        treasury_amount,
        validator_credits: credits,
    }
}

// ── Fee routing ───────────────────────────────────────────────────────────────

/// How base fees collected in a block should be routed.
#[derive(Debug, Clone, Copy)]
pub struct FeeRouteSplit {
    /// Burned (sent to zero address, permanently removed from supply).
    pub burned: u64,
    /// Added to the PMFP accumulator for long-term distribution.
    pub pmfp_contribution: u64,
}

/// Split `gas_used * base_fee` between burn and PMFP.
pub fn route_fees(gas_used: u64, base_fee: u64) -> FeeRouteSplit {
    let total = gas_used.saturating_mul(base_fee);
    let burned = total * FEE_BURN_PERCENT / 100;
    let pmfp = total.saturating_sub(burned);
    FeeRouteSplit {
        burned,
        pmfp_contribution: pmfp,
    }
}
