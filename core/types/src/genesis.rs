/// Phase 13.2 — Genesis block parameters and mainnet configuration.
///
/// These constants are the single source of truth for network identity.
/// They are consumed by the node at startup to construct or validate the genesis block.
use serde::{Deserialize, Serialize};

use crate::{MAINNET_CHAIN_ID, TESTNET_CHAIN_ID};

// ── Mainnet constants ─────────────────────────────────────────────────────────

/// Block heights at which the block reward halves (every ~4 years at 12s/block).
/// Era 0: 50 RPC, Era 1: 25 RPC, Era 2: 12.5 RPC, …
/// 10_512_000 blocks ≈ 4 years (86_400 s/day × 365.25 × 4 / 12 s/block).
pub const HALVING_INTERVAL: u64 = 10_512_000;

/// Maximum total supply cap (21 billion micro-RPC; 210 million full RPC).
/// Matches whitepaper §6.1: infinite-series sum of halvings = 2 × Era-0-supply.
pub const MAX_SUPPLY_MICRO_RPC: u64 = 210_000_000 * 100_000_000;

/// Initial (Era 0) block reward in micro-RPC. Defined in core/types but repeated
/// here for genesis documentation clarity.
pub const GENESIS_BLOCK_REWARD_MICRO_RPC: u64 = 50 * 100_000_000; // 50 RPC

/// Initial treasury allocation at genesis (5% of Era-0 emission cap).
pub const GENESIS_TREASURY_MICRO_RPC: u64 = 1_050_000 * 100_000_000; // 10.5 M RPC

/// Minimum validator stake for mainnet.
pub const MAINNET_MIN_STAKE_MICRO_RPC: u64 = 10_000 * 100_000_000; // 10 000 RPC

/// Target validator count range for mainnet launch.
pub const MAINNET_TARGET_VALIDATORS_MIN: u32 = 500;
pub const MAINNET_TARGET_VALIDATORS_MAX: u32 = 1_000;

/// Human-readable network names.
pub const MAINNET_NETWORK_NAME: &str = "rupeecoin-mainnet-zeta";
pub const TESTNET_NETWORK_NAME: &str = "rupeecoin-testnet-zeta";

// ── Genesis configuration ─────────────────────────────────────────────────────

/// Complete set of parameters that fully defines a RupeeCoin network genesis.
///
/// Load from a JSON file with [`GenesisConfig::from_json`] or use the built-in
/// [`GenesisConfig::mainnet()`] / [`GenesisConfig::testnet()`] constants.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenesisConfig {
    /// EIP-155-style chain identifier (1 = mainnet, 2 = testnet).
    pub chain_id: u32,
    /// Human-readable network name (included in genesis block's coinbase_data).
    pub network_name: String,
    /// Unix timestamp of the genesis block (T-0 of the countdown).
    /// 0 means "use the node's current wall-clock time" (dev/testnet only).
    pub genesis_timestamp: u64,
    /// Initial treasury balance in micro-RPC, pre-minted in the genesis coinbase.
    pub treasury_micro_rpc: u64,
    /// Treasury recipient address (20-byte hex).
    pub treasury_address_hex: String,
    /// Block reward in micro-RPC for Era 0.
    pub initial_block_reward_micro_rpc: u64,
    /// Number of blocks between halvings.
    pub halving_interval: u64,
    /// Maximum supply cap in micro-RPC (informational; not enforced on-chain).
    pub max_supply_micro_rpc: u64,
    /// Minimum stake to join the validator set in micro-RPC.
    pub min_stake_micro_rpc: u64,
    /// Initial base fee (micro-RPC per gas unit) for EIP-1559 fee market.
    pub initial_base_fee: u64,
    /// Block gas target (in gas units).
    pub gas_target: u64,
    /// Extra arbitrary bytes included in the genesis coinbase (e.g. a message).
    pub genesis_message: String,
}

impl GenesisConfig {
    /// Mainnet genesis configuration.
    ///
    /// `genesis_timestamp` is set to 0 here and must be updated to the real
    /// T-0 timestamp before the network launches (Phase 14 step 1).
    pub fn mainnet() -> Self {
        GenesisConfig {
            chain_id: MAINNET_CHAIN_ID,
            network_name: MAINNET_NETWORK_NAME.into(),
            genesis_timestamp: 0, // set at T-72h during Phase 14
            treasury_micro_rpc: GENESIS_TREASURY_MICRO_RPC,
            treasury_address_hex: "0000000000000000000000000000000000000001".into(),
            initial_block_reward_micro_rpc: GENESIS_BLOCK_REWARD_MICRO_RPC,
            halving_interval: HALVING_INTERVAL,
            max_supply_micro_rpc: MAX_SUPPLY_MICRO_RPC,
            min_stake_micro_rpc: MAINNET_MIN_STAKE_MICRO_RPC,
            initial_base_fee: 1_000, // 1000 micro-RPC/gas (EIP-1559 starting point)
            gas_target: 15_000_000,
            genesis_message: "RupeeCoin: open, private, quantum-safe finance for all.".into(),
        }
    }

    /// Testnet (Zeta) genesis configuration.
    pub fn testnet() -> Self {
        GenesisConfig {
            chain_id: TESTNET_CHAIN_ID,
            network_name: TESTNET_NETWORK_NAME.into(),
            genesis_timestamp: 0,
            treasury_micro_rpc: 1_000_000 * 100_000_000, // 1 M RPC for faucet
            treasury_address_hex: "0000000000000000000000000000000000000002".into(),
            initial_block_reward_micro_rpc: GENESIS_BLOCK_REWARD_MICRO_RPC,
            halving_interval: HALVING_INTERVAL / 10, // faster halvings on testnet
            max_supply_micro_rpc: MAX_SUPPLY_MICRO_RPC,
            min_stake_micro_rpc: 100 * 100_000_000, // only 100 RPC to stake on testnet
            initial_base_fee: 1_000,
            gas_target: 15_000_000,
            genesis_message: "RupeeCoin Zeta Testnet — expect instability.".into(),
        }
    }

    /// Deserialise from JSON (e.g. `--genesis-config genesis.json`).
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Serialise to pretty-printed JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("GenesisConfig serialization")
    }

    /// Validate the configuration for obvious inconsistencies.
    pub fn validate(&self) -> Result<(), String> {
        if self.chain_id == 0 {
            return Err("chain_id must be > 0".into());
        }
        if self.network_name.is_empty() {
            return Err("network_name must not be empty".into());
        }
        if self.treasury_address_hex.len() != 40 {
            return Err("treasury_address_hex must be 40 hex chars (20 bytes)".into());
        }
        hex::decode(&self.treasury_address_hex)
            .map_err(|e| format!("treasury_address_hex is not valid hex: {e}"))?;
        if self.halving_interval == 0 {
            return Err("halving_interval must be > 0".into());
        }
        if self.gas_target == 0 {
            return Err("gas_target must be > 0".into());
        }
        if self.min_stake_micro_rpc == 0 {
            return Err("min_stake_micro_rpc must be > 0".into());
        }
        Ok(())
    }

    /// Block reward at a given block height, applying the halving schedule.
    pub fn block_reward_at(&self, height: u64) -> u64 {
        if self.halving_interval == 0 {
            return 0;
        }
        let era = height / self.halving_interval;
        if era >= 64 {
            return 0;
        }
        self.initial_block_reward_micro_rpc >> era
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mainnet_config_is_valid() {
        let cfg = GenesisConfig::mainnet();
        cfg.validate().expect("mainnet config should be valid");
        assert_eq!(cfg.chain_id, 1);
        assert_eq!(cfg.halving_interval, HALVING_INTERVAL);
    }

    #[test]
    fn testnet_config_is_valid() {
        let cfg = GenesisConfig::testnet();
        cfg.validate().expect("testnet config should be valid");
        assert_eq!(cfg.chain_id, 2);
    }

    #[test]
    fn json_roundtrip() {
        let cfg = GenesisConfig::mainnet();
        let json = cfg.to_json();
        let decoded = GenesisConfig::from_json(&json).unwrap();
        assert_eq!(cfg, decoded);
    }

    #[test]
    fn block_reward_halving() {
        let cfg = GenesisConfig::mainnet();
        assert_eq!(cfg.block_reward_at(0), GENESIS_BLOCK_REWARD_MICRO_RPC);
        assert_eq!(cfg.block_reward_at(HALVING_INTERVAL), GENESIS_BLOCK_REWARD_MICRO_RPC / 2);
        assert_eq!(cfg.block_reward_at(HALVING_INTERVAL * 2), GENESIS_BLOCK_REWARD_MICRO_RPC / 4);
    }

    #[test]
    fn block_reward_saturates_to_zero_after_64_halvings() {
        let cfg = GenesisConfig::mainnet();
        assert_eq!(cfg.block_reward_at(HALVING_INTERVAL * 64), 0);
        assert_eq!(cfg.block_reward_at(u64::MAX), 0);
    }

    #[test]
    fn invalid_config_rejected() {
        let mut cfg = GenesisConfig::mainnet();
        cfg.chain_id = 0;
        assert!(cfg.validate().is_err());

        let mut cfg2 = GenesisConfig::mainnet();
        cfg2.treasury_address_hex = "not-hex".into();
        assert!(cfg2.validate().is_err());

        let mut cfg3 = GenesisConfig::mainnet();
        cfg3.halving_interval = 0;
        assert!(cfg3.validate().is_err());
    }
}
