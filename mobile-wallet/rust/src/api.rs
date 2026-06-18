//! Flutter-facing API — annotated for flutter_rust_bridge codegen.
//!
//! Run `flutter_rust_bridge_codegen generate` from the `mobile-wallet/` directory
//! to produce `lib/src/rust/frb_generated.dart` + `src/frb_generated.rs`.

use flutter_rust_bridge::frb;

use rupeecoin_crypto::{decode_address, encode_address};
use rupeecoin_sdk::Wallet;
use rupeecoin_zkproof::{prove_spend, prove_spend_batch, BatchSpendInput};

use crate::keystore::{self, KeystoreInner};

// ── Response types ─────────────────────────────────────────────────────────

pub struct WalletInfo {
    /// Base58Check transparent receive address.
    pub address: String,
    /// 40-char hex address (for RPC queries).
    pub address_hex: String,
    /// Current HD receive index.
    pub receive_index: u32,
    /// Path of the encrypted keystore file on device.
    pub keystore_path: String,
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn wallet_at(seed_hex: &str, index: u32) -> Result<Wallet, String> {
    let seed_bytes: [u8; 32] = hex::decode(seed_hex)
        .map_err(|e| format!("bad seed: {e}"))?
        .try_into()
        .map_err(|_| "seed must be 32 bytes")?;
    let mut w = Wallet::from_seed(seed_bytes);
    w.receive_index = index;
    Ok(w)
}

fn info_from_wallet(w: &Wallet, receive_index: u32, keystore_path: String) -> WalletInfo {
    let addr_raw = w.receive_address();
    WalletInfo {
        address: encode_address(&addr_raw),
        address_hex: hex::encode(addr_raw.0),
        receive_index,
        keystore_path,
    }
}

// ── Public API ─────────────────────────────────────────────────────────────

/// Generate a fresh HD wallet, encrypt, and save to `keystore_path`.
#[frb(sync)]
pub fn generate_wallet(passphrase: String, keystore_path: String) -> Result<WalletInfo, String> {
    let wallet = Wallet::generate();
    let inner = KeystoreInner {
        master_seed_hex: hex::encode(wallet.master_seed()),
        receive_index: 0,
    };
    keystore::save(&keystore_path, &inner, &passphrase)?;
    Ok(info_from_wallet(&wallet, 0, keystore_path))
}

/// Decrypt an existing keystore and return wallet info.
#[frb(sync)]
pub fn unlock_wallet(passphrase: String, keystore_path: String) -> Result<WalletInfo, String> {
    let inner = keystore::load(&keystore_path, &passphrase)?;
    let w = wallet_at(&inner.master_seed_hex, inner.receive_index)?;
    Ok(info_from_wallet(&w, inner.receive_index, keystore_path))
}

/// Advance HD receive index by 1 and persist the updated keystore.
#[frb(sync)]
pub fn advance_address(passphrase: String, keystore_path: String) -> Result<WalletInfo, String> {
    let mut inner = keystore::load(&keystore_path, &passphrase)?;
    inner.receive_index += 1;
    let w = wallet_at(&inner.master_seed_hex, inner.receive_index)?;
    keystore::save(&keystore_path, &inner, &passphrase)?;
    Ok(info_from_wallet(&w, inner.receive_index, keystore_path))
}

/// Verify that `passphrase` decrypts the keystore correctly (for PIN-change flows).
#[frb(sync)]
pub fn check_passphrase(passphrase: String, keystore_path: String) -> bool {
    keystore::load(&keystore_path, &passphrase).is_ok()
}

/// Re-encrypt the keystore under a new passphrase.
#[frb(sync)]
pub fn change_passphrase(
    old_passphrase: String,
    new_passphrase: String,
    keystore_path: String,
) -> Result<(), String> {
    let inner = keystore::load(&keystore_path, &old_passphrase)?;
    keystore::save(&keystore_path, &inner, &new_passphrase)
}

/// Validate a Base58Check address string. Returns the hex representation or an error.
#[frb(sync)]
pub fn validate_address(address: String) -> Result<String, String> {
    let raw = decode_address(&address).map_err(|e| format!("{e}"))?;
    Ok(hex::encode(raw.0))
}

// ── Phase 11.4 — Mobile prover ────────────────────────────────────────────────

/// Result of a STARK spend proof for a single shielded input.
pub struct SpendProofResult {
    /// Raw Winterfell proof bytes — pass directly to `verify_spend_mobile`.
    pub proof_bytes: Vec<u8>,
    /// Poseidon Merkle anchor (LE-bytes of 4 Goldilocks u64s, 32 bytes total).
    pub anchor_bytes: Vec<u8>,
    /// Poseidon nullifier (LE-bytes of 4 Goldilocks u64s, 32 bytes total).
    pub nullifier_bytes: Vec<u8>,
}

/// Result of a batched STARK spend proof (up to 10 shielded inputs).
pub struct BatchSpendProofResult {
    pub proof_bytes: Vec<u8>,
    pub anchor_bytes: Vec<u8>,
    /// Nullifiers in input order, each 32 bytes LE-encoded Goldilocks.
    pub nullifier_bytes: Vec<Vec<u8>>,
    pub num_spends: u32,
}

/// Convert 32 raw bytes into the `[u64; 4]` Goldilocks encoding used by the STARK circuit.
fn bytes_to_felt4(b: &[u8]) -> Result<[u64; 4], String> {
    if b.len() != 32 {
        return Err(format!("expected 32 bytes, got {}", b.len()));
    }
    Ok(std::array::from_fn(|i| {
        u64::from_le_bytes(b[8 * i..8 * i + 8].try_into().unwrap())
    }))
}

fn felt4_to_bytes(felts: &[u64; 4]) -> Vec<u8> {
    felts.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Generate a STARK spend proof on a background thread so the Flutter UI stays responsive.
///
/// All byte slices use the Goldilocks-LE encoding of the STARK circuit:
/// 32 bytes = 4 × u64-le = one Poseidon state element group.
///
/// `nk_bytes`      — 32-byte nullifier key (Goldilocks-LE)
/// `cm_bytes`      — 32-byte note commitment (Goldilocks-LE)
/// `siblings_bytes` — 20 × 32-byte sibling hashes (level 0 first)
/// `is_right`      — 20 direction bits (true = spending note is RIGHT child)
///
/// Proof generation takes 3–10 s on mobile hardware; call this from an async
/// Dart context so the UI thread is never blocked.
pub async fn prove_spend_mobile(
    nk_bytes: Vec<u8>,
    cm_bytes: Vec<u8>,
    siblings_bytes: Vec<Vec<u8>>,
    is_right: Vec<bool>,
) -> Result<SpendProofResult, String> {
    if siblings_bytes.len() != 20 {
        return Err(format!("need 20 siblings, got {}", siblings_bytes.len()));
    }
    if is_right.len() != 20 {
        return Err(format!("need 20 is_right flags, got {}", is_right.len()));
    }

    let nk   = bytes_to_felt4(&nk_bytes)?;
    let cm   = bytes_to_felt4(&cm_bytes)?;
    let mut sibs = [[0u64; 4]; 20];
    for (i, s) in siblings_bytes.iter().enumerate() {
        sibs[i] = bytes_to_felt4(s)?;
    }
    let ir: [bool; 20] = is_right.try_into().map_err(|_| "is_right length mismatch")?;

    // Run the CPU-intensive prover in a dedicated OS thread so the FRB
    // tokio runtime (which handles Dart↔Rust I/O) is never blocked.
    let result = tokio::task::spawn_blocking(move || prove_spend(nk, cm, sibs, ir))
        .await
        .map_err(|e| format!("prover thread panicked: {e}"))?
        .map_err(|e| format!("prove_spend failed: {e}"))?;

    let (proof, nullifier) = result;
    Ok(SpendProofResult {
        proof_bytes:   proof.proof_bytes,
        anchor_bytes:  felt4_to_bytes(&proof.anchor_felts),
        nullifier_bytes: felt4_to_bytes(&nullifier),
    })
}

/// Generate a batched STARK spend proof covering up to 10 inputs on a background thread.
///
/// All inputs must belong to the same commitment tree snapshot (same anchor).
/// Returns a single proof that verifies in the same time as one individual proof.
pub async fn prove_spend_batch_mobile(
    nk_bytes_list:       Vec<Vec<u8>>,
    cm_bytes_list:       Vec<Vec<u8>>,
    siblings_bytes_list: Vec<Vec<Vec<u8>>>,
    is_right_list:       Vec<Vec<bool>>,
) -> Result<BatchSpendProofResult, String> {
    let n = nk_bytes_list.len();
    if n == 0 || n > 10 {
        return Err(format!("batch size must be 1–10, got {n}"));
    }
    if cm_bytes_list.len() != n || siblings_bytes_list.len() != n || is_right_list.len() != n {
        return Err("all input lists must have the same length".into());
    }

    let mut inputs = Vec::with_capacity(n);
    for i in 0..n {
        let nk = bytes_to_felt4(&nk_bytes_list[i])?;
        let cm = bytes_to_felt4(&cm_bytes_list[i])?;
        if siblings_bytes_list[i].len() != 20 {
            return Err(format!("input {i}: need 20 siblings"));
        }
        if is_right_list[i].len() != 20 {
            return Err(format!("input {i}: need 20 is_right flags"));
        }
        let mut sibs = [[0u64; 4]; 20];
        for (j, s) in siblings_bytes_list[i].iter().enumerate() {
            sibs[j] = bytes_to_felt4(s)?;
        }
        let ir: [bool; 20] = is_right_list[i]
            .clone()
            .try_into()
            .map_err(|_| format!("input {i}: is_right length mismatch"))?;
        inputs.push(BatchSpendInput { nullifier_key: nk, note_commitment: cm, siblings: sibs, is_right: ir });
    }

    let result = tokio::task::spawn_blocking(move || prove_spend_batch(&inputs))
        .await
        .map_err(|e| format!("prover thread panicked: {e}"))?
        .map_err(|e| format!("prove_spend_batch failed: {e}"))?;

    let (proof, nullifiers) = result;
    Ok(BatchSpendProofResult {
        proof_bytes:    proof.proof_bytes,
        anchor_bytes:   felt4_to_bytes(&proof.anchor_felts),
        nullifier_bytes: nullifiers.iter().map(felt4_to_bytes).collect(),
        num_spends:     proof.num_spends,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wallet.json").to_str().unwrap().to_string();
        (dir, path)
    }

    #[test]
    fn generate_and_unlock() {
        let (_dir, path) = tmp_path();
        let info = generate_wallet("pass123".into(), path.clone()).unwrap();
        assert_eq!(info.receive_index, 0);
        assert!(!info.address.is_empty());

        let unlocked = unlock_wallet("pass123".into(), path).unwrap();
        assert_eq!(unlocked.address, info.address);
    }

    #[test]
    fn advance_address_increments() {
        let (_dir, path) = tmp_path();
        let first = generate_wallet("pass".into(), path.clone()).unwrap();
        let second = advance_address("pass".into(), path.clone()).unwrap();
        assert_eq!(second.receive_index, 1);
        assert_ne!(first.address, second.address);
    }

    #[test]
    fn change_passphrase_works() {
        let (_dir, path) = tmp_path();
        generate_wallet("old".into(), path.clone()).unwrap();
        change_passphrase("old".into(), "new".into(), path.clone()).unwrap();
        assert!(unlock_wallet("new".into(), path.clone()).is_ok());
        assert!(unlock_wallet("old".into(), path).is_err());
    }

    // ── Phase 11.4 mobile prover tests ───────────────────────────────────────

    const NK: [u64; 4] = [1, 2, 3, 4];
    const CM: [u64; 4] = [5, 6, 7, 8];
    const SIB: [u64; 4] = [9, 10, 11, 12];

    fn felt4_bytes(felts: &[u64; 4]) -> Vec<u8> {
        felts.iter().flat_map(|f| f.to_le_bytes()).collect()
    }

    fn sibling_list() -> Vec<Vec<u8>> {
        (0..20).map(|_| felt4_bytes(&SIB)).collect()
    }

    #[tokio::test]
    async fn prove_spend_mobile_roundtrip() {
        let nk_b = felt4_bytes(&NK);
        let cm_b = felt4_bytes(&CM);
        let sibs = sibling_list();
        let ir = vec![false; 20];

        let result = prove_spend_mobile(nk_b, cm_b, sibs, ir)
            .await
            .expect("prove_spend_mobile must succeed");

        assert_eq!(result.anchor_bytes.len(), 32);
        assert_eq!(result.nullifier_bytes.len(), 32);
        assert!(!result.proof_bytes.is_empty());
    }

    #[tokio::test]
    async fn prove_spend_mobile_bad_siblings_count_errors() {
        let nk_b = felt4_bytes(&NK);
        let cm_b = felt4_bytes(&CM);
        let sibs: Vec<Vec<u8>> = (0..5).map(|_| felt4_bytes(&SIB)).collect(); // wrong count
        let ir = vec![false; 20];

        assert!(prove_spend_mobile(nk_b, cm_b, sibs, ir).await.is_err());
    }

    #[tokio::test]
    async fn prove_spend_batch_mobile_two_inputs() {
        // Two leaves at positions 0 and 1 sharing the same Merkle root.
        let cm_a: [u64; 4] = [5, 6, 7, 8];
        let cm_b: [u64; 4] = [50, 60, 70, 80];
        let upper_sib: [u64; 4] = [9, 10, 11, 12];
        let nk_a: [u64; 4] = [1, 2, 3, 4];
        let nk_b: [u64; 4] = [10, 20, 30, 40];

        let mut sibs_a = vec![felt4_bytes(&cm_b)];
        sibs_a.extend((0..19).map(|_| felt4_bytes(&upper_sib)));

        let mut sibs_b = vec![felt4_bytes(&cm_a)];
        sibs_b.extend((0..19).map(|_| felt4_bytes(&upper_sib)));

        let mut ir_b = vec![false; 20];
        ir_b[0] = true;

        let result = prove_spend_batch_mobile(
            vec![felt4_bytes(&nk_a), felt4_bytes(&nk_b)],
            vec![felt4_bytes(&cm_a), felt4_bytes(&cm_b)],
            vec![sibs_a, sibs_b],
            vec![vec![false; 20], ir_b],
        )
        .await
        .expect("batch prove must succeed for N=2");

        assert_eq!(result.num_spends, 2);
        assert_eq!(result.nullifier_bytes.len(), 2);
        assert_eq!(result.anchor_bytes.len(), 32);
    }
}
