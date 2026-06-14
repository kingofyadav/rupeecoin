//! Keystore: persist keys to an encrypted JSON file.
//!
//! Version 1  — plaintext hex (legacy, read-only).
//! Version 2  — Argon2id + XChaCha20-Poly1305 (privacy keys only).
//! Version 3  — Argon2id + XChaCha20-Poly1305 (privacy keys + HD master seed).
use std::path::Path;

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    Key, XChaCha20Poly1305, XNonce,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use rupeecoin_crypto::Hash;
use rupeecoin_privacy::{
    NullifierKey, PaymentAddress, PrivacyKeyBundle, SpendKey, ViewKey, ViewPublicKey, ViewSecretKey,
};

// ── Argon2id parameters ────────────────────────────────────────────────────────

const ARGON2_MEM_KIB: u32 = 65_536;
const ARGON2_ITERS: u32 = 3;
const ARGON2_LANES: u32 = 1;
const ARGON2_OUT_LEN: usize = 32;
const KEYSTORE_AD: &[u8] = b"rupeecoin-keystore-v2"; // same AD for v2 and v3

// ── Errors ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum KeystoreError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("hex decode error: {0}")]
    Hex(#[from] hex::FromHexError),
    #[error("invalid key data: {0}")]
    InvalidKey(String),
    #[error("decryption failed — wrong passphrase?")]
    DecryptionFailed,
    #[error("passphrase prompt failed: {0}")]
    Passphrase(String),
}

// ── On-disk structures ─────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct KeystoreV1 {
    version: u32,
    spend_key_hex: String,
    nullifier_key_hex: String,
    view_public_key_hex: String,
    view_secret_key_hex: String,
    payment_address_hex: String,
}

/// Inner plaintext for v2/v3 — extends v1 with optional HD fields.
#[derive(Serialize, Deserialize)]
struct KeystoreInner {
    // Privacy keys (always present)
    spend_key_hex: String,
    nullifier_key_hex: String,
    view_public_key_hex: String,
    view_secret_key_hex: String,
    payment_address_hex: String,
    // HD keys (v3+, absent in v1/v2 inner blobs → default to None/0)
    #[serde(default)]
    hd_master_seed_hex: Option<String>,
    #[serde(default)]
    hd_receive_index: u32,
}

/// Encrypted outer wrapper (same format for v2 and v3).
#[derive(Serialize, Deserialize)]
struct EncryptedKeystore {
    version: u32,
    salt_hex: String,
    nonce_hex: String,
    ciphertext_hex: String,
}

#[derive(Deserialize)]
struct VersionProbe {
    version: u32,
}

// ── Returned bundle, extended with HD info ────────────────────────────────────

pub struct LoadedKeystore {
    pub bundle: PrivacyKeyBundle,
    /// HD master seed (32 bytes), present when keystore version ≥ 3.
    pub hd_master_seed: Option<[u8; 32]>,
    /// Next unused receive address index.
    pub hd_receive_index: u32,
}

// ── Crypto helpers ─────────────────────────────────────────────────────────────

fn derive_key(password: &[u8], salt: &[u8]) -> [u8; ARGON2_OUT_LEN] {
    let params =
        Params::new(ARGON2_MEM_KIB, ARGON2_ITERS, ARGON2_LANES, Some(ARGON2_OUT_LEN)).unwrap();
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; ARGON2_OUT_LEN];
    argon2.hash_password_into(password, salt, &mut key).expect("argon2 params are valid");
    key
}

fn encrypt(plaintext: &[u8], key: &[u8; 32], nonce: &[u8; 24]) -> Vec<u8> {
    XChaCha20Poly1305::new(Key::from_slice(key))
        .encrypt(
            XNonce::from_slice(nonce),
            chacha20poly1305::aead::Payload { msg: plaintext, aad: KEYSTORE_AD },
        )
        .expect("encryption is infallible for valid params")
}

fn decrypt(ct: &[u8], key: &[u8; 32], nonce: &[u8; 24]) -> Result<Vec<u8>, KeystoreError> {
    XChaCha20Poly1305::new(Key::from_slice(key))
        .decrypt(
            XNonce::from_slice(nonce),
            chacha20poly1305::aead::Payload { msg: ct, aad: KEYSTORE_AD },
        )
        .map_err(|_| KeystoreError::DecryptionFailed)
}

fn prompt_password(prompt: &str) -> Result<String, KeystoreError> {
    rpassword::prompt_password(prompt).map_err(|e| KeystoreError::Passphrase(e.to_string()))
}

// ── Bundle ↔ inner ────────────────────────────────────────────────────────────

fn bundle_to_inner(
    bundle: &PrivacyKeyBundle,
    hd_seed: Option<&[u8; 32]>,
    receive_index: u32,
) -> KeystoreInner {
    KeystoreInner {
        spend_key_hex: hex::encode(bundle.spend_key.as_bytes()),
        nullifier_key_hex: hex::encode(&bundle.nullifier_key.0),
        view_public_key_hex: hex::encode(bundle.view_key.public.as_bytes()),
        view_secret_key_hex: hex::encode(bundle.view_key.secret.as_bytes()),
        payment_address_hex: hex::encode(bundle.payment_address.0.as_bytes()),
        hd_master_seed_hex: hd_seed.map(hex::encode),
        hd_receive_index: receive_index,
    }
}

fn inner_to_loaded(inner: &KeystoreInner) -> Result<LoadedKeystore, KeystoreError> {
    let spend_bytes: [u8; 32] = hex::decode(&inner.spend_key_hex)?
        .try_into()
        .map_err(|_| KeystoreError::InvalidKey("spend key".into()))?;
    let nk_bytes: [u8; 32] = hex::decode(&inner.nullifier_key_hex)?
        .try_into()
        .map_err(|_| KeystoreError::InvalidKey("nullifier key".into()))?;
    let vpk_bytes = hex::decode(&inner.view_public_key_hex)?;
    let vsk_bytes = hex::decode(&inner.view_secret_key_hex)?;
    let pa_bytes: [u8; 32] = hex::decode(&inner.payment_address_hex)?
        .try_into()
        .map_err(|_| KeystoreError::InvalidKey("payment address".into()))?;

    let bundle = PrivacyKeyBundle {
        spend_key: SpendKey::from_bytes(spend_bytes),
        nullifier_key: NullifierKey(nk_bytes),
        view_key: ViewKey {
            public: ViewPublicKey::from_bytes(vpk_bytes),
            secret: ViewSecretKey::from_bytes(&vsk_bytes)
                .ok_or_else(|| KeystoreError::InvalidKey("view secret key invalid".into()))?,
        },
        payment_address: PaymentAddress(Hash::from_bytes(pa_bytes)),
    };

    let hd_master_seed = inner
        .hd_master_seed_hex
        .as_deref()
        .map(|h| {
            hex::decode(h)
                .ok()
                .and_then(|b| b.try_into().ok())
                .ok_or_else(|| KeystoreError::InvalidKey("hd master seed".into()))
        })
        .transpose()?;

    Ok(LoadedKeystore {
        bundle,
        hd_master_seed,
        hd_receive_index: inner.hd_receive_index,
    })
}

// ── Public API ─────────────────────────────────────────────────────────────────

/// Save a v3 (encrypted) keystore.  Prompts for passphrase interactively.
pub fn save(
    bundle: &PrivacyKeyBundle,
    hd_seed: Option<&[u8; 32]>,
    receive_index: u32,
    path: &Path,
) -> Result<(), KeystoreError> {
    let password = prompt_password("Enter keystore passphrase: ")?;
    let confirm = prompt_password("Confirm passphrase: ")?;
    if password != confirm {
        return Err(KeystoreError::Passphrase("passphrases do not match".into()));
    }

    let mut salt = [0u8; 16];
    let mut nonce_bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let key = derive_key(password.as_bytes(), &salt);
    let inner = bundle_to_inner(bundle, hd_seed, receive_index);
    let plaintext = serde_json::to_vec(&inner)?;
    let ct = encrypt(&plaintext, &key, &nonce_bytes);

    let file = EncryptedKeystore {
        version: 3,
        salt_hex: hex::encode(salt),
        nonce_hex: hex::encode(nonce_bytes),
        ciphertext_hex: hex::encode(ct),
    };
    std::fs::write(path, serde_json::to_string_pretty(&file)?)?;
    Ok(())
}

/// Load a keystore (version 1, 2, or 3).  Prompts for passphrase on v2/v3.
pub fn load(path: &Path) -> Result<LoadedKeystore, KeystoreError> {
    let json = std::fs::read_to_string(path)?;
    let probe: VersionProbe = serde_json::from_str(&json)?;

    match probe.version {
        2 | 3 => {
            let file: EncryptedKeystore = serde_json::from_str(&json)?;
            let password = prompt_password("Enter keystore passphrase: ")?;

            let salt = hex::decode(&file.salt_hex)
                .map_err(|_| KeystoreError::InvalidKey("bad salt".into()))?;
            let nonce: [u8; 24] = hex::decode(&file.nonce_hex)
                .map_err(|_| KeystoreError::InvalidKey("bad nonce".into()))?
                .try_into()
                .map_err(|_| KeystoreError::InvalidKey("nonce length".into()))?;
            let ct = hex::decode(&file.ciphertext_hex)
                .map_err(|_| KeystoreError::InvalidKey("bad ciphertext".into()))?;

            let key = derive_key(password.as_bytes(), &salt);
            let pt = decrypt(&ct, &key, &nonce)?;
            let inner: KeystoreInner = serde_json::from_slice(&pt)?;
            inner_to_loaded(&inner)
        }
        1 => {
            let file: KeystoreV1 = serde_json::from_str(&json)?;
            let inner = KeystoreInner {
                spend_key_hex: file.spend_key_hex,
                nullifier_key_hex: file.nullifier_key_hex,
                view_public_key_hex: file.view_public_key_hex,
                view_secret_key_hex: file.view_secret_key_hex,
                payment_address_hex: file.payment_address_hex,
                hd_master_seed_hex: None,
                hd_receive_index: 0,
            };
            inner_to_loaded(&inner)
        }
        v => Err(KeystoreError::InvalidKey(format!("unknown keystore version {v}"))),
    }
}
