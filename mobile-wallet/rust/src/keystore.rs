//! Mobile keystore — identical format to the desktop wallet (Argon2id + XChaCha20-Poly1305).
//! Files saved on Android in `getApplicationDocumentsDirectory()/wallet.json`.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    Key, XChaCha20Poly1305, XNonce,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};

const ARGON2_MEM_KIB: u32 = 65_536; // 64 MiB
const ARGON2_ITERS: u32 = 3;
const ARGON2_LANES: u32 = 1;
const ARGON2_OUT_LEN: usize = 32;
const KEYSTORE_AD: &[u8] = b"rupeecoin-keystore-v2";

#[derive(Serialize, Deserialize, Clone)]
pub struct KeystoreInner {
    pub master_seed_hex: String,
    pub receive_index: u32,
}

#[derive(Serialize, Deserialize)]
struct EncryptedKeystore {
    version: u32,
    salt_hex: String,
    nonce_hex: String,
    ciphertext_hex: String,
}

fn derive_key(password: &[u8], salt: &[u8]) -> [u8; ARGON2_OUT_LEN] {
    let params =
        Params::new(ARGON2_MEM_KIB, ARGON2_ITERS, ARGON2_LANES, Some(ARGON2_OUT_LEN)).unwrap();
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; ARGON2_OUT_LEN];
    argon2.hash_password_into(password, salt, &mut key).expect("argon2 params valid");
    key
}

fn encrypt(plaintext: &[u8], key: &[u8; 32], nonce: &[u8; 24]) -> Vec<u8> {
    XChaCha20Poly1305::new(Key::from_slice(key))
        .encrypt(
            XNonce::from_slice(nonce),
            chacha20poly1305::aead::Payload { msg: plaintext, aad: KEYSTORE_AD },
        )
        .expect("encryption is infallible")
}

fn decrypt(ct: &[u8], key: &[u8; 32], nonce: &[u8; 24]) -> Result<Vec<u8>, String> {
    XChaCha20Poly1305::new(Key::from_slice(key))
        .decrypt(
            XNonce::from_slice(nonce),
            chacha20poly1305::aead::Payload { msg: ct, aad: KEYSTORE_AD },
        )
        .map_err(|_| "wrong passphrase or corrupted keystore".to_string())
}

pub fn save(path: &str, inner: &KeystoreInner, passphrase: &str) -> Result<(), String> {
    let mut salt = [0u8; 16];
    let mut nonce_bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let key = derive_key(passphrase.as_bytes(), &salt);
    let plaintext = serde_json::to_vec(inner).map_err(|e| e.to_string())?;
    let ct = encrypt(&plaintext, &key, &nonce_bytes);

    let ks = EncryptedKeystore {
        version: 3,
        salt_hex: hex::encode(salt),
        nonce_hex: hex::encode(nonce_bytes),
        ciphertext_hex: hex::encode(ct),
    };
    let json = serde_json::to_string_pretty(&ks).map_err(|e| e.to_string())?;
    // Atomic write: write to a temp file then rename to prevent partial writes.
    let tmp = format!("{path}.tmp");
    std::fs::write(&tmp, &json).map_err(|e| format!("write keystore tmp: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename keystore: {e}"))
}

pub fn load(path: &str, passphrase: &str) -> Result<KeystoreInner, String> {
    let json = std::fs::read_to_string(path).map_err(|e| format!("read keystore: {e}"))?;
    let ks: EncryptedKeystore = serde_json::from_str(&json).map_err(|e| format!("parse: {e}"))?;

    let salt = hex::decode(&ks.salt_hex).map_err(|_| "bad salt")?;
    let nonce: [u8; 24] = hex::decode(&ks.nonce_hex)
        .map_err(|_| "bad nonce")?
        .try_into()
        .map_err(|_| "nonce len")?;
    let ct = hex::decode(&ks.ciphertext_hex).map_err(|_| "bad ciphertext")?;

    let key = derive_key(passphrase.as_bytes(), &salt);
    let pt = decrypt(&ct, &key, &nonce)?;
    serde_json::from_slice(&pt).map_err(|e| format!("parse inner: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wallet.json").to_str().unwrap().to_string();
        let inner = KeystoreInner { master_seed_hex: "aa".repeat(32), receive_index: 3 };
        save(&path, &inner, "hunter2").unwrap();
        let loaded = load(&path, "hunter2").unwrap();
        assert_eq!(loaded.master_seed_hex, inner.master_seed_hex);
        assert_eq!(loaded.receive_index, 3);
    }

    #[test]
    fn wrong_passphrase() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wallet.json").to_str().unwrap().to_string();
        let inner = KeystoreInner { master_seed_hex: "bb".repeat(32), receive_index: 0 };
        save(&path, &inner, "correct").unwrap();
        assert!(load(&path, "wrong").is_err());
    }
}
