pub mod rpc1155;
pub mod rpc20;
pub mod rpc721;

pub use rpc1155::rpc1155_bytecode;
pub use rpc20::rpc20_bytecode;
pub use rpc721::rpc721_bytecode;

// ── Calling convention ────────────────────────────────────────────────────────
//
// Call data layout (all values big-endian u64 in 8-byte slots):
//   offset  0..8  : function selector (u64)
//   offset  8..16 : arg0
//   offset 16..24 : arg1
//   offset 24..32 : arg2
//   ...
//
// Return data layout:
//   offset  0..8  : return value (or first return value for multi-return)
//
// Function selectors are the low 64 bits of BLAKE3("FunctionName(args)").
// Pre-computed values live in each token module.

/// Compute a deterministic 64-bit selector from a human-readable signature string.
/// Used by tests and the token templates to agree on dispatch values.
pub fn selector(sig: &str) -> u64 {
    let hash = rupeecoin_crypto::hash_data(sig.as_bytes());
    u64::from_be_bytes(hash.as_bytes()[..8].try_into().unwrap())
}

// ── Storage slot helpers (shared across token standards) ──────────────────────

/// Balance storage key: high half marks namespace, low half is addr.
pub fn balance_slot(addr: u64) -> u64 {
    // Mix addr into a different region of the key space to avoid colliding
    // with direct slot indices (0, 1, …).
    0x0001_0000_0000_0000u64.wrapping_add(addr.wrapping_mul(0x517c_c1b7_2722_0a95))
}

/// Allowance storage key: XOR-mix of owner and spender into namespace 2.
pub fn allowance_slot(owner: u64, spender: u64) -> u64 {
    let mixed =
        owner.wrapping_mul(0x517c_c1b7_2722_0a95) ^ spender.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    0x0002_0000_0000_0000u64.wrapping_add(mixed)
}

/// RPC-721 owner-of slot.
pub fn owner_slot(token_id: u64) -> u64 {
    0x0003_0000_0000_0000u64.wrapping_add(token_id.wrapping_mul(0x517c_c1b7_2722_0a95))
}

/// RPC-1155 balance slot: (account, token_id) pair.
pub fn multi_balance_slot(account: u64, token_id: u64) -> u64 {
    let mixed =
        account.wrapping_mul(0x517c_c1b7_2722_0a95) ^ token_id.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    0x0004_0000_0000_0000u64.wrapping_add(mixed)
}
