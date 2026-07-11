use pqcrypto_dilithium::dilithium3;
use pqcrypto_traits::sign::{
    DetachedSignature as DetachedSigTrait, PublicKey as PkTrait, SecretKey as SkTrait,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::hash::{hash_data, Address, Hash};

// ── Dilithium3 sizes (CLEAN variant, always available) ────────────────────────

const DL3_PK_BYTES: usize = 1952;
const DL3_SK_BYTES: usize = 4032;

// PQClean Dilithium3 CLEAN seeded keypair — symbol is always present since the
// pqcrypto-dilithium crate compiles the CLEAN C sources unconditionally.
extern "C" {
    fn PQCLEAN_DILITHIUM3_CLEAN_crypto_sign_seed_keypair(
        pk: *mut u8,
        sk: *mut u8,
        seed: *const u8,
    ) -> std::os::raw::c_int;
}

// ── Errors ────────────────────────────────────────────────────────────────────

#[derive(Error, Debug)]
pub enum CryptoError {
    #[error("invalid signature")]
    InvalidSignature,
    #[error("invalid public key encoding")]
    InvalidPublicKey,
    #[error("invalid address encoding")]
    InvalidAddress,
    #[error("address checksum mismatch")]
    InvalidChecksum,
}

// ── Types ─────────────────────────────────────────────────────────────────────

/// CRYSTALS-Dilithium3 public key (1952 bytes).
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PublicKey(pub(crate) Vec<u8>);

/// CRYSTALS-Dilithium3 detached signature (3293 bytes).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature(pub(crate) Vec<u8>);

impl std::fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PublicKey({}…)", hex::encode(&self.0[..4]))
    }
}

impl std::fmt::Debug for Signature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Signature({} bytes)", self.0.len())
    }
}

impl Signature {
    pub fn empty() -> Self {
        Signature(vec![])
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Reconstruct a signature from raw bytes, validating the Dilithium3 encoding.
    pub fn from_bytes(b: &[u8]) -> Result<Self, CryptoError> {
        dilithium3::DetachedSignature::from_bytes(b).map_err(|_| CryptoError::InvalidSignature)?;
        Ok(Signature(b.to_vec()))
    }
}

/// Ephemeral keypair — secret key is not serializable by design.
pub struct KeyPair {
    pub public_key: PublicKey,
    pub(crate) secret_key: dilithium3::SecretKey,
}

impl KeyPair {
    /// Generate a fresh keypair using OS randomness.
    pub fn generate() -> Self {
        let (pk, sk) = dilithium3::keypair();
        KeyPair {
            public_key: PublicKey(pk.as_bytes().to_vec()),
            secret_key: sk,
        }
    }

    /// Deterministically generate a keypair from a 32-byte seed.
    ///
    /// Uses the PQClean Dilithium3 CLEAN seeded keygen function; the result
    /// is compatible with standard Dilithium3 signing/verification regardless
    /// of which platform variant (AVX2 / AARCH64 / CLEAN) is used at runtime.
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let dil_seed = hash_data(seed); // domain-separate the HD seed from raw Dilithium input
        let mut pk_bytes = [0u8; DL3_PK_BYTES];
        let mut sk_bytes = [0u8; DL3_SK_BYTES];
        let ret = unsafe {
            PQCLEAN_DILITHIUM3_CLEAN_crypto_sign_seed_keypair(
                pk_bytes.as_mut_ptr(),
                sk_bytes.as_mut_ptr(),
                dil_seed.as_bytes().as_ptr(),
            )
        };
        assert_eq!(
            ret, 0,
            "PQCLEAN_DILITHIUM3_CLEAN_crypto_sign_seed_keypair failed"
        );

        let pk = dilithium3::PublicKey::from_bytes(&pk_bytes).expect("valid pk from seed");
        let sk = dilithium3::SecretKey::from_bytes(&sk_bytes).expect("valid sk from seed");
        KeyPair {
            public_key: PublicKey(pk.as_bytes().to_vec()),
            secret_key: sk,
        }
    }

    pub fn sign(&self, message: &[u8]) -> Signature {
        let sig = dilithium3::detached_sign(message, &self.secret_key);
        Signature(sig.as_bytes().to_vec())
    }
}

impl PublicKey {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Reconstruct a public key from raw bytes, validating the Dilithium3 encoding.
    pub fn from_bytes(b: &[u8]) -> Result<Self, CryptoError> {
        dilithium3::PublicKey::from_bytes(b).map_err(|_| CryptoError::InvalidPublicKey)?;
        Ok(PublicKey(b.to_vec()))
    }

    /// Derive the 20-byte UTXO address: first 20 bytes of BLAKE3(public_key_bytes).
    pub fn to_address(&self) -> Address {
        let h = hash_data(&self.0);
        let mut arr = [0u8; 20];
        arr.copy_from_slice(&h.as_bytes()[..20]);
        Address(arr)
    }

    /// Full 32-byte BLAKE3 of the public key — used in the validator registry.
    pub fn to_hash(&self) -> Hash {
        hash_data(&self.0)
    }

    pub fn verify(&self, message: &[u8], signature: &Signature) -> Result<(), CryptoError> {
        let pk = dilithium3::PublicKey::from_bytes(&self.0)
            .map_err(|_| CryptoError::InvalidPublicKey)?;
        let sig = dilithium3::DetachedSignature::from_bytes(&signature.0)
            .map_err(|_| CryptoError::InvalidSignature)?;
        dilithium3::verify_detached_signature(&sig, message, &pk)
            .map_err(|_| CryptoError::InvalidSignature)
    }
}
