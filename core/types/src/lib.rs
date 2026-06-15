pub mod attestation;
pub mod block;
pub mod contract;
pub mod genesis;
pub mod transaction;

pub use attestation::{Attestation, SlashingEvidence, ValidatorAnnouncement};
pub use block::{Block, BlockHeader};
pub use contract::{ContractCall, ContractDeploy};
pub use genesis::GenesisConfig;
pub use transaction::{OutPoint, Transaction, TxInput, TxOutput};

/// Micro-RPC per full coin (8 decimal places, like satoshis).
pub const MICRO_RPC_PER_COIN: u64 = 100_000_000;

/// Initial (Era 0) block reward: 50 RPC in micro-RPC.
/// Use `rupeecoin_economics::block_reward(height)` for the height-adjusted value.
pub const BLOCK_REWARD: u64 = 50 * MICRO_RPC_PER_COIN;

// ── Chain IDs (EIP-155 style) ─────────────────────────────────────────────────
pub const MAINNET_CHAIN_ID: u32 = 1;
pub const TESTNET_CHAIN_ID: u32 = 2;
