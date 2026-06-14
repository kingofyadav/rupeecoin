//! Desktop-wallet keystore: encrypted JSON file.
//!
//! Format: Argon2id (64 MiB, 3 iter) + XChaCha20-Poly1305.
//! Plaintext: `{ "master_seed_hex": "...", "receive_index": N }`.
//! Compatible with the rupeecoin-wallet CLI v3 keystore for the HD seed fields
//! (the GUI wallet omits shielded-pool keys; they are re-derived on demand).

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    Key, XChaCha20Poly1305, XNonce,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};

const ARGON2_MEM_KIB: u32 = 65_536;
const ARGON2_ITERS: u32 = 3;
const ARGON2_LANES: u32 = 1;
const ARGON2_OUT_LEN: usize = 32;
const KEYSTORE_AD: &[u8] = b"rupeecoin-keystore-v2";

// ── On-disk JSON structure ─────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
pub struct EncryptedKeystore {
    pub version: u32,
    pub salt_hex: String,
    pub nonce_hex: String,
    pub ciphertext_hex: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct KeystoreInner {
    pub master_seed_hex: String,
    pub receive_index: u32,
}

// ── Crypto helpers ─────────────────────────────────────────────────────────

fn derive_key(password: &[u8], salt: &[u8]) -> [u8; ARGON2_OUT_LEN] {
    let params =
        Params::new(ARGON2_MEM_KIB, ARGON2_ITERS, ARGON2_LANES, Some(ARGON2_OUT_LEN)).unwrap();
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; ARGON2_OUT_LEN];
    argon2.hash_password_into(password, salt, &mut key).expect("argon2 params valid");
    key
}

fn xchacha_encrypt(plaintext: &[u8], key: &[u8; 32], nonce: &[u8; 24]) -> Vec<u8> {
    XChaCha20Poly1305::new(Key::from_slice(key))
        .encrypt(
            XNonce::from_slice(nonce),
            chacha20poly1305::aead::Payload { msg: plaintext, aad: KEYSTORE_AD },
        )
        .expect("encryption is infallible")
}

fn xchacha_decrypt(ct: &[u8], key: &[u8; 32], nonce: &[u8; 24]) -> Result<Vec<u8>, String> {
    XChaCha20Poly1305::new(Key::from_slice(key))
        .decrypt(
            XNonce::from_slice(nonce),
            chacha20poly1305::aead::Payload { msg: ct, aad: KEYSTORE_AD },
        )
        .map_err(|_| "decryption failed — wrong passphrase?".to_string())
}

// ── Public API ─────────────────────────────────────────────────────────────

/// Encrypt and save a keystore to `path`.
pub fn save(path: &str, inner: &KeystoreInner, passphrase: &str) -> Result<(), String> {
    let mut salt = [0u8; 16];
    let mut nonce_bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let key = derive_key(passphrase.as_bytes(), &salt);
    let plaintext = serde_json::to_vec(inner).map_err(|e| e.to_string())?;
    let ct = xchacha_encrypt(&plaintext, &key, &nonce_bytes);

    let ks = EncryptedKeystore {
        version: 3,
        salt_hex: hex::encode(salt),
        nonce_hex: hex::encode(nonce_bytes),
        ciphertext_hex: hex::encode(ct),
    };
    std::fs::write(path, serde_json::to_string_pretty(&ks).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

/// Load and decrypt a keystore from `path`.
pub fn load(path: &str, passphrase: &str) -> Result<KeystoreInner, String> {
    let json = std::fs::read_to_string(path).map_err(|e| format!("read keystore: {e}"))?;
    let ks: EncryptedKeystore =
        serde_json::from_str(&json).map_err(|e| format!("parse keystore: {e}"))?;

    let salt = hex::decode(&ks.salt_hex).map_err(|_| "bad salt hex")?;
    let nonce: [u8; 24] = hex::decode(&ks.nonce_hex)
        .map_err(|_| "bad nonce hex")?
        .try_into()
        .map_err(|_| "nonce must be 24 bytes")?;
    let ct = hex::decode(&ks.ciphertext_hex).map_err(|_| "bad ciphertext hex")?;

    let key = derive_key(passphrase.as_bytes(), &salt);
    let pt = xchacha_decrypt(&ct, &key, &nonce)?;
    serde_json::from_slice(&pt).map_err(|e| format!("parse plaintext: {e}"))
}
