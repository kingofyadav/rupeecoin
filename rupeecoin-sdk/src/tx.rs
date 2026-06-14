//! Transaction construction helpers.
//!
//! Provides `build_transfer` for transparent UTXO transactions.
//! For shielded transactions use `rupeecoin_privacy::ShieldedTxBuilder` directly.

use rupeecoin_crypto::{KeyPair, Signature};
use rupeecoin_types::{OutPoint, Transaction, TxInput, TxOutput};

/// Build and sign a transparent transfer transaction.
///
/// # Arguments
/// * `keypair`  — signing key; the same key must control all `outpoints`
/// * `outpoints` — UTXO inputs to spend
/// * `outputs`   — transaction outputs
/// * `chain_id`  — `MAINNET_CHAIN_ID` (1) or `TESTNET_CHAIN_ID` (2)
/// * `nonce`     — sender's current nonce (from `UtxoSet::nonce_for`)
/// * `expiry`    — last block height at which the tx is valid (0 = no expiry)
///
/// # Returns
/// A fully signed `Transaction` ready for submission via `RpcClient::submit_tx`.
pub fn build_transfer(
    keypair: &KeyPair,
    outpoints: Vec<OutPoint>,
    outputs: Vec<TxOutput>,
    chain_id: u32,
    nonce: u64,
    expiry: u64,
) -> Transaction {
    let mut tx = Transaction {
        version: 1,
        chain_id,
        nonce,
        expiry,
        inputs: outpoints
            .into_iter()
            .map(|op| TxInput {
                outpoint: op,
                public_key: keypair.public_key.clone(),
                signature: Signature::empty(),
            })
            .collect(),
        outputs,
        coinbase_data: None,
    };
    let sig_hash = tx.sig_hash();
    let sig = keypair.sign(sig_hash.as_bytes());
    for input in &mut tx.inputs {
        input.signature = sig.clone();
    }
    tx
}

/// Convert whole RPC coins to micro-RPC (the on-chain unit).
///
/// `1 RPC = 100_000_000 micro-RPC`
pub fn coins_to_micro(amount: f64) -> u64 {
    (amount * rupeecoin_types::MICRO_RPC_PER_COIN as f64).round() as u64
}

/// Convert micro-RPC to a human-readable coin string.
pub fn micro_to_coins_str(micro: u64) -> String {
    let per_coin = rupeecoin_types::MICRO_RPC_PER_COIN;
    let coins = micro / per_coin;
    let frac = micro % per_coin;
    if frac == 0 {
        format!("{coins} RPC")
    } else {
        let s = format!("{}.{:08}", coins, frac);
        format!("{} RPC", s.trim_end_matches('0').trim_end_matches('.'))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rupeecoin_types::MICRO_RPC_PER_COIN;

    #[test]
    fn coins_to_micro_roundtrip() {
        assert_eq!(coins_to_micro(1.0), MICRO_RPC_PER_COIN);
        assert_eq!(coins_to_micro(0.5), MICRO_RPC_PER_COIN / 2);
        assert_eq!(coins_to_micro(10.0), 10 * MICRO_RPC_PER_COIN);
    }

    #[test]
    fn micro_to_coins_str_whole() {
        assert_eq!(micro_to_coins_str(MICRO_RPC_PER_COIN), "1 RPC");
        assert_eq!(micro_to_coins_str(10 * MICRO_RPC_PER_COIN), "10 RPC");
    }

    #[test]
    fn micro_to_coins_str_fractional() {
        // MICRO_RPC_PER_COIN = 100_000_000
        assert_eq!(micro_to_coins_str(150_000_000), "1.5 RPC");
        assert_eq!(micro_to_coins_str(100_000_001), "1.00000001 RPC");
    }
}
