use chacha20poly1305::{
    aead::{Aead, KeyInit},
    XChaCha20Poly1305,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use rupeecoin_crypto::{hash_data, Hash};

use crate::keys::{NullifierKey, PaymentAddress, ViewCiphertext, ViewPublicKey, ViewSecretKey};

// ── Note ─────────────────────────────────────────────────────────────────────

/// A private UTXO.  Only the note holder knows the value and recipient.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Note {
    /// Value in micro-RPC.
    pub value: u64,
    /// Recipient payment address (hidden in the commitment).
    pub recipient: PaymentAddress,
    /// Random commitment material — 32 random bytes chosen by the sender.
    pub rcm: [u8; 32],
}

impl Note {
    pub fn new(value: u64, recipient: PaymentAddress) -> Self {
        let mut rcm = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut rcm);
        Note {
            value,
            recipient,
            rcm,
        }
    }

    /// `cm = BLAKE3(value_le || recipient_hash || rcm)`
    pub fn commitment(&self) -> NoteCommitment {
        let mut buf = Vec::with_capacity(8 + 32 + 32);
        buf.extend_from_slice(&self.value.to_le_bytes());
        buf.extend_from_slice(self.recipient.0.as_bytes());
        buf.extend_from_slice(&self.rcm);
        NoteCommitment(hash_data(&buf))
    }

    /// `nullifier = BLAKE3(nullifier_key || cm)`
    pub fn nullifier(&self, nk: &NullifierKey) -> Nullifier {
        let cm = self.commitment();
        let mut buf = Vec::with_capacity(32 + 32);
        buf.extend_from_slice(nk.as_bytes());
        buf.extend_from_slice(cm.0.as_bytes());
        Nullifier(hash_data(&buf))
    }
}

// ── NoteCommitment ────────────────────────────────────────────────────────────

/// A hiding and binding commitment to a note.  Stored on-chain in the privacy pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NoteCommitment(pub Hash);

// ── Nullifier ─────────────────────────────────────────────────────────────────

/// A spend token — revealed when a note is consumed.  Prevents double-spending
/// without identifying which note was spent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Nullifier(pub Hash);

// ── Note encryption ───────────────────────────────────────────────────────────

/// The plaintext payload encrypted inside a shielded output.
#[derive(Clone, Serialize, Deserialize)]
struct NotePlaintext {
    value: u64,
    recipient: PaymentAddress,
    rcm: [u8; 32],
}

/// Kyber-encrypted note: only the holder of the matching ViewSecretKey can recover the note.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedNote {
    /// Kyber768 ciphertext — from which the recipient derives the note encryption key.
    pub kem_ct: ViewCiphertext,
    /// XChaCha20-Poly1305 ciphertext over `(value, recipient, rcm)`.
    pub ciphertext: Vec<u8>,
}

impl EncryptedNote {
    /// Encrypt a note to a recipient's Kyber public key.
    pub fn encrypt(note: &Note, recipient_vpk: &ViewPublicKey) -> Self {
        let (shared_secret, kem_ct) = recipient_vpk.encapsulate();

        let plaintext = bincode::serialize(&NotePlaintext {
            value: note.value,
            recipient: note.recipient.clone(),
            rcm: note.rcm,
        })
        .expect("note plaintext serialization");

        let enc_key = note_enc_key(&shared_secret);
        let nonce = note_nonce(&shared_secret);
        let cipher = XChaCha20Poly1305::new(&enc_key);
        let ciphertext = cipher
            .encrypt(&nonce, plaintext.as_slice())
            .expect("note encryption");

        EncryptedNote { kem_ct, ciphertext }
    }

    /// Attempt decryption using the recipient's Kyber secret key.
    /// Returns `None` if the ciphertext is not addressed to this key.
    pub fn decrypt(&self, vsk: &ViewSecretKey) -> Option<Note> {
        let shared_secret = vsk.decapsulate(&self.kem_ct)?;
        let enc_key = note_enc_key(&shared_secret);
        let nonce = note_nonce(&shared_secret);
        let cipher = XChaCha20Poly1305::new(&enc_key);
        let plaintext = cipher.decrypt(&nonce, self.ciphertext.as_slice()).ok()?;
        let np: NotePlaintext = bincode::deserialize(&plaintext).ok()?;
        Some(Note {
            value: np.value,
            recipient: np.recipient,
            rcm: np.rcm,
        })
    }
}

// ── Key and nonce derivation ──────────────────────────────────────────────────

fn note_enc_key(shared_secret: &[u8; 32]) -> chacha20poly1305::Key {
    let mut key = [0u8; 32];
    blake3::Hasher::new_derive_key("rupeecoin-note-enc-key-v1")
        .update(shared_secret)
        .finalize_xof()
        .fill(&mut key);
    chacha20poly1305::Key::from(key)
}

fn note_nonce(shared_secret: &[u8; 32]) -> chacha20poly1305::XNonce {
    // BLAKE3 XOF(shared_secret || "_nce") → 192-bit nonce for XChaCha20
    let mut nonce = [0u8; 24];
    blake3::Hasher::new_derive_key("rupeecoin-note-nonce-v1")
        .update(shared_secret)
        .finalize_xof()
        .fill(&mut nonce);
    chacha20poly1305::XNonce::from(nonce)
}
