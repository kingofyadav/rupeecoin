use pqcrypto_kyber::kyber768;
use pqcrypto_traits::kem::{
    Ciphertext as CiphertextTrait, PublicKey as PkTrait, SharedSecret as SsTrait,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use rupeecoin_crypto::{hash_data, Hash};

// Kyber768 key sizes (from PQClean spec).
const KYBER768_PK_BYTES: usize = 1184;
const KYBER768_SK_BYTES: usize = 2400;
const KYBER768_COINS_BYTES: usize = 64;

// Link-time assertions to catch upstream size changes before they silently corrupt keys.
const _: () = assert!(
    KYBER768_PK_BYTES == 1184,
    "Kyber768 public key size changed"
);
const _: () = assert!(
    KYBER768_SK_BYTES == 2400,
    "Kyber768 secret key size changed"
);
const _: () = assert!(KYBER768_COINS_BYTES == 64, "Kyber768 coins size changed");

extern "C" {
    /// Deterministic Kyber768 key generation.  `coins` must be exactly 64 bytes.
    fn PQCLEAN_KYBER768_CLEAN_crypto_kem_keypair_derand(
        pk: *mut u8,
        sk: *mut u8,
        coins: *const u8,
    ) -> std::ffi::c_int;
}

// ── SpendKey ──────────────────────────────────────────────────────────────────

/// Master spending secret.  Never transmitted; kept only in the local keystore.
pub struct SpendKey([u8; 32]);

impl SpendKey {
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        SpendKey(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        SpendKey(bytes)
    }

    /// Deterministic nullifier key — used to compute spend tokens.
    pub fn nullifier_key(&self) -> NullifierKey {
        let mut buf = [0u8; 33];
        buf[..32].copy_from_slice(&self.0);
        buf[32] = 0x01; // domain separator
        NullifierKey(*hash_data(&buf).as_bytes())
    }
}

// ── NullifierKey ──────────────────────────────────────────────────────────────

/// Derived from SpendKey; used to generate nullifiers without revealing SpendKey.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NullifierKey(pub [u8; 32]);

impl NullifierKey {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

// ── ViewKey (Kyber768 KEM) ────────────────────────────────────────────────────

/// Kyber768 public key — published so senders can encrypt notes to us.
#[derive(Clone, Serialize, Deserialize)]
pub struct ViewPublicKey(Vec<u8>);

impl ViewPublicKey {
    /// Encapsulate: returns `(shared_secret_32_bytes, ciphertext)`.
    pub fn encapsulate(&self) -> ([u8; 32], ViewCiphertext) {
        let pk = kyber768::PublicKey::from_bytes(&self.0)
            .expect("view public key bytes are valid Kyber768");
        let (ss, ct) = kyber768::encapsulate(&pk);
        let mut secret = [0u8; 32];
        secret.copy_from_slice(ss.as_bytes());
        (secret, ViewCiphertext(ct.as_bytes().to_vec()))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        ViewPublicKey(bytes)
    }
}

/// Kyber768 ciphertext — included in each shielded output so the recipient
/// can recover the note encryption key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewCiphertext(pub Vec<u8>);

/// Kyber768 secret key — stored locally by the note recipient.
pub struct ViewSecretKey(kyber768::SecretKey);

impl ViewSecretKey {
    /// Decapsulate: given a ciphertext, recover the 32-byte shared secret.
    pub fn decapsulate(&self, ct: &ViewCiphertext) -> Option<[u8; 32]> {
        let ct_obj = kyber768::Ciphertext::from_bytes(&ct.0).ok()?;
        let ss = kyber768::decapsulate(&ct_obj, &self.0);
        let mut secret = [0u8; 32];
        secret.copy_from_slice(ss.as_bytes());
        Some(secret)
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        use pqcrypto_traits::kem::SecretKey as SkTrait;
        self.0.as_bytes().to_vec()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        use pqcrypto_traits::kem::SecretKey as SkTrait;
        kyber768::SecretKey::from_bytes(bytes)
            .ok()
            .map(ViewSecretKey)
    }
}

// ── ViewKey pair ─────────────────────────────────────────────────────────────

/// Kyber768 keypair used for receiving encrypted notes.
pub struct ViewKey {
    pub public: ViewPublicKey,
    pub secret: ViewSecretKey,
}

impl ViewKey {
    pub fn generate() -> Self {
        let (pk, sk) = kyber768::keypair();
        ViewKey {
            public: ViewPublicKey(pk.as_bytes().to_vec()),
            secret: ViewSecretKey(sk),
        }
    }

    /// Derive a Kyber768 keypair deterministically from a 32-byte seed.
    pub fn from_seed(seed: &[u8; 32]) -> Result<Self, String> {
        let mut coins = [0u8; KYBER768_COINS_BYTES];
        blake3::Hasher::new_derive_key("rupeecoin-view-key-coins-v1")
            .update(seed)
            .finalize_xof()
            .fill(&mut coins);

        let mut pk_bytes = [0u8; KYBER768_PK_BYTES];
        let mut sk_bytes = [0u8; KYBER768_SK_BYTES];
        let ret = unsafe {
            PQCLEAN_KYBER768_CLEAN_crypto_kem_keypair_derand(
                pk_bytes.as_mut_ptr(),
                sk_bytes.as_mut_ptr(),
                coins.as_ptr(),
            )
        };
        if ret != 0 {
            return Err(format!("Kyber768 keypair_derand returned {ret}"));
        }

        Ok(ViewKey {
            public: ViewPublicKey::from_bytes(pk_bytes.to_vec()),
            secret: ViewSecretKey::from_bytes(&sk_bytes)
                .ok_or_else(|| "keypair_derand produced invalid secret key".to_string())?,
        })
    }
}

// ── PaymentAddress ────────────────────────────────────────────────────────────

/// A one-use or reusable address that encodes enough information for a sender
/// to create a shielded output the receiver can spend.
///
/// `payment_address = BLAKE3(nullifier_key || kyber_pubkey_bytes)`
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PaymentAddress(pub Hash);

impl PaymentAddress {
    pub fn from_keys(nk: &NullifierKey, vpk: &ViewPublicKey) -> Self {
        let mut buf = Vec::with_capacity(32 + vpk.0.len());
        buf.extend_from_slice(&nk.0);
        buf.extend_from_slice(&vpk.0);
        PaymentAddress(hash_data(&buf))
    }
}

// ── Full user key bundle ──────────────────────────────────────────────────────

/// Everything a user needs to send, receive, and spend private notes.
pub struct PrivacyKeyBundle {
    pub spend_key: SpendKey,
    pub nullifier_key: NullifierKey,
    pub view_key: ViewKey,
    pub payment_address: PaymentAddress,
}

impl PrivacyKeyBundle {
    /// Generate a fresh bundle from a cryptographically-random seed.
    /// All keys are co-derived so restoring the seed restores everything.
    pub fn generate() -> Self {
        let mut seed = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut seed);
        Self::from_seed(&seed)
    }

    /// Derive all privacy keys deterministically from a 32-byte master seed.
    ///
    /// Expands `seed` to 64 bytes via BLAKE3 XOF first, then splits into
    /// separate inputs for the spend key and the view key, preventing any
    /// relationship between the two derived secrets.
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let mut expanded = [0u8; 64];
        blake3::Hasher::new_derive_key("rupeecoin-from-seed-expand-v1")
            .update(seed)
            .finalize_xof()
            .fill(&mut expanded);
        let spend_seed: [u8; 32] = expanded[..32].try_into().unwrap();
        let view_seed: [u8; 32] = expanded[32..].try_into().unwrap();

        let spend_key_bytes = blake3::derive_key("rupeecoin-spend-key-v1", &spend_seed);
        let spend_key = SpendKey::from_bytes(spend_key_bytes);
        let nullifier_key = spend_key.nullifier_key();
        let view_key = ViewKey::from_seed(&view_seed)
            .expect("Kyber768 keygen from expanded seed must succeed");
        let payment_address = PaymentAddress::from_keys(&nullifier_key, &view_key.public);
        PrivacyKeyBundle {
            spend_key,
            nullifier_key,
            view_key,
            payment_address,
        }
    }
}
