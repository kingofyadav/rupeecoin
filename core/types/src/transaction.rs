use rupeecoin_crypto::{hash_data, Address, Hash, PublicKey, Signature};
use serde::{Deserialize, Serialize};

use crate::attestation::SlashingEvidence;
use crate::MAINNET_CHAIN_ID;

// ── Marker prefixes for special coinbase-like transactions ────────────────────
// 9 bytes each so they can never be confused with the 8-byte LE u64 height
// stored in a regular block-reward coinbase_data.
const REWARD_CLAIM_PREFIX: &[u8] = b"RC\x01\x00\x00\x00\x00\x00\x00";
const SLASH_REPORT_PREFIX: &[u8] = b"SR\x01\x00\x00\x00\x00\x00\x00";

/// Reference to a previous unspent output.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
pub struct OutPoint {
    pub txid: Hash,
    pub vout: u32,
}

/// Spending of one UTXO, authorised by the owner's Dilithium3 signature.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct TxInput {
    pub outpoint: OutPoint,
    pub public_key: PublicKey,
    pub signature: Signature,
}

/// New coin allocation: a recipient address and an amount in micro-RPC.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct TxOutput {
    /// Amount in micro-RPC (1 RPC = 100_000_000 micro-RPC).
    pub amount: u64,
    /// Recipient: 20-byte Base58Check address (first 20 bytes of BLAKE3(pubkey)).
    pub recipient: Address,
}

/// A transaction.
///
/// `chain_id` prevents cross-chain replay.
/// `nonce`    prevents same-chain replay (sequential, per-sender, tracked in UtxoSet).
/// `expiry`   makes the tx invalid after a given block height (0 = never expires).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct Transaction {
    pub version: u32,
    /// Chain the tx is bound to (1=mainnet, 2=testnet).
    pub chain_id: u32,
    /// Sender's current nonce (monotonically increasing per address in UtxoSet).
    pub nonce: u64,
    /// Last block height at which this tx may be included (0 = no expiry).
    pub expiry: u64,
    pub inputs: Vec<TxInput>,
    pub outputs: Vec<TxOutput>,
    /// Arbitrary bytes only present in coinbase transactions (e.g. block height).
    pub coinbase_data: Option<Vec<u8>>,
}

// Fields covered by every input signature — does NOT include the signatures themselves.
#[derive(Serialize)]
struct SigHashData<'a> {
    chain_id: u32,
    nonce: u64,
    expiry: u64,
    version: u32,
    outpoints: Vec<&'a OutPoint>,
    outputs: &'a [TxOutput],
}

impl Transaction {
    /// The hash every input signature must cover (chain-bound, replay-safe).
    pub fn sig_hash(&self) -> Hash {
        let data = SigHashData {
            chain_id: self.chain_id,
            nonce: self.nonce,
            expiry: self.expiry,
            version: self.version,
            outpoints: self.inputs.iter().map(|i| &i.outpoint).collect(),
            outputs: &self.outputs,
        };
        hash_data(&bincode::serialize(&data).expect("sig_hash serialization"))
    }

    /// Stable transaction identifier: BLAKE3 of the fully serialised transaction.
    pub fn txid(&self) -> Hash {
        hash_data(&bincode::serialize(self).expect("txid serialization"))
    }

    /// True for block-reward coinbase transactions (not reward claims or slash reports).
    pub fn is_coinbase(&self) -> bool {
        self.inputs.is_empty() && !self.is_reward_claim() && !self.is_slashing_report()
    }

    /// True when this is a validator reward-claim transaction.
    pub fn is_reward_claim(&self) -> bool {
        self.inputs.is_empty()
            && self
                .coinbase_data
                .as_deref()
                .map(|d| d.starts_with(REWARD_CLAIM_PREFIX))
                .unwrap_or(false)
    }

    /// Extract the validator address from a reward-claim transaction.
    pub fn reward_claim_addr(&self) -> Option<Hash> {
        let data = self.coinbase_data.as_deref()?;
        if data.starts_with(REWARD_CLAIM_PREFIX) && data.len() >= REWARD_CLAIM_PREFIX.len() + 32 {
            let off = REWARD_CLAIM_PREFIX.len();
            let bytes: [u8; 32] = data[off..off + 32].try_into().ok()?;
            Some(Hash::from_bytes(bytes))
        } else {
            None
        }
    }

    /// Extract the authorization `(validator_pubkey_bytes, signature_bytes)` from a
    /// reward-claim transaction. Layout after the prefix:
    /// `addr(32) || pk_len(4, LE) || pk || sig`.
    pub fn reward_claim_auth(&self) -> Option<(&[u8], &[u8])> {
        let data = self.coinbase_data.as_deref()?;
        if !data.starts_with(REWARD_CLAIM_PREFIX) {
            return None;
        }
        let mut off = REWARD_CLAIM_PREFIX.len() + 32;
        if data.len() < off + 4 {
            return None;
        }
        let pk_len = u32::from_le_bytes(data[off..off + 4].try_into().ok()?) as usize;
        off += 4;
        if data.len() < off + pk_len {
            return None;
        }
        let pk = &data[off..off + pk_len];
        let sig = &data[off + pk_len..];
        if sig.is_empty() {
            return None;
        }
        Some((pk, sig))
    }

    /// True when this is an on-chain slashing-report transaction.
    pub fn is_slashing_report(&self) -> bool {
        self.inputs.is_empty()
            && self
                .coinbase_data
                .as_deref()
                .map(|d| d.starts_with(SLASH_REPORT_PREFIX))
                .unwrap_or(false)
    }

    /// Deserialise the `SlashingEvidence` embedded in a slashing-report transaction.
    pub fn slashing_evidence(&self) -> Option<SlashingEvidence> {
        let data = self.coinbase_data.as_deref()?;
        if data.starts_with(SLASH_REPORT_PREFIX) {
            bincode::deserialize(&data[SLASH_REPORT_PREFIX.len()..]).ok()
        } else {
            None
        }
    }
}

impl Default for Transaction {
    fn default() -> Self {
        Transaction {
            version: 1,
            chain_id: MAINNET_CHAIN_ID,
            nonce: 0,
            expiry: 0,
            inputs: vec![],
            outputs: vec![],
            coinbase_data: None,
        }
    }
}
