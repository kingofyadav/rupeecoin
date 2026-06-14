//! Flutter-facing API — annotated for flutter_rust_bridge codegen.
//!
//! Run `flutter_rust_bridge_codegen generate` from the `mobile-wallet/` directory
//! to produce `lib/src/rust/frb_generated.dart` + `src/frb_generated.rs`.

use flutter_rust_bridge::frb;

use rupeecoin_crypto::{decode_address, encode_address};
use rupeecoin_sdk::Wallet;

use crate::keystore::{self, KeystoreInner};

// ── Response types ─────────────────────────────────────────────────────────

#[frb(dart_metadata = ("freezed"))]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path() -> String {
        let dir = tempfile::tempdir().unwrap();
        dir.into_path().join("wallet.json").to_str().unwrap().to_string()
    }

    #[test]
    fn generate_and_unlock() {
        let path = tmp_path();
        let info = generate_wallet("pass123".into(), path.clone()).unwrap();
        assert_eq!(info.receive_index, 0);
        assert!(!info.address.is_empty());

        let unlocked = unlock_wallet("pass123".into(), path).unwrap();
        assert_eq!(unlocked.address, info.address);
    }

    #[test]
    fn advance_address_increments() {
        let path = tmp_path();
        let first = generate_wallet("pass".into(), path.clone()).unwrap();
        let second = advance_address("pass".into(), path.clone()).unwrap();
        assert_eq!(second.receive_index, 1);
        assert_ne!(first.address, second.address);
    }

    #[test]
    fn change_passphrase_works() {
        let path = tmp_path();
        generate_wallet("old".into(), path.clone()).unwrap();
        change_passphrase("old".into(), "new".into(), path.clone()).unwrap();
        assert!(unlock_wallet("new".into(), path.clone()).is_ok());
        assert!(unlock_wallet("old".into(), path).is_err());
    }
}
