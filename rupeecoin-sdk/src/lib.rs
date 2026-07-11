//! # RupeeCoin SDK v0.1
//!
//! Rust SDK for building applications on the RupeeCoin blockchain.
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use rupeecoin_sdk::{Wallet, RpcClient};
//!
//! // Generate a new wallet
//! let wallet = Wallet::generate();
//! println!("Receive address: {}", wallet.receive_address());
//!
//! // Connect to a node
//! let rpc = RpcClient::new("http://127.0.0.1:9944");
//! let height = rpc.height().unwrap();
//! println!("Chain height: {height}");
//! ```

pub mod keys;
pub mod rpc;
pub mod tx;

pub use keys::Wallet;
pub use rpc::{NodeStatus, RpcClient, UtxoEntry};

// ── Re-exported core types ────────────────────────────────────────────────────

pub use rupeecoin_crypto::{
    decode_address, encode_address, hash_data, Address, Hash, HdNode, KeyPair, PublicKey,
    HD_RECEIVE, HD_VALIDATOR,
};
pub use rupeecoin_privacy::{
    EncryptedNote, Note, NoteCommitment, Nullifier, PrivacyKeyBundle, ShieldedTxBuilder,
};
pub use rupeecoin_types::{
    Block, BlockHeader, OutPoint, Transaction, TxInput, TxOutput, MAINNET_CHAIN_ID,
    MICRO_RPC_PER_COIN, TESTNET_CHAIN_ID,
};
