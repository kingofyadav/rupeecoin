//! Key generation and HD wallet management.
//!
//! The `Wallet` struct owns a BLAKE3 HD master seed and a `PrivacyKeyBundle`.
//! All child keys are re-derived on demand — private key material is never
//! serialised.

use rand::RngCore;

use rupeecoin_crypto::{encode_address, Address, HdNode, KeyPair, HD_RECEIVE, HD_VALIDATOR};
use rupeecoin_privacy::PrivacyKeyBundle;

/// A RupeeCoin wallet: HD master seed + shielded-pool privacy keys.
///
/// The 32-byte `master_seed` is the only secret that needs to be backed up.
/// Encrypt it before storing (e.g. with Argon2id + XChaCha20-Poly1305).
pub struct Wallet {
    master_seed: [u8; 32],
    /// Transparent receive address index.  Each call to `advance_receive_index`
    /// increments this and returns a fresh address.
    pub receive_index: u32,
    pub privacy: PrivacyKeyBundle,
}

impl Wallet {
    /// Generate a new wallet from a cryptographically random seed.
    pub fn generate() -> Self {
        let mut seed = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut seed);
        Self::from_seed(seed)
    }

    /// Restore a wallet from a known 32-byte master seed.
    /// All keys — transparent HD keys AND shielded privacy keys — are derived
    /// deterministically, so the same seed always restores the same wallet.
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Wallet {
            master_seed: seed,
            receive_index: 0,
            privacy: PrivacyKeyBundle::from_seed(&seed),
        }
    }

    /// Return the 32-byte master seed for secure backup.
    pub fn master_seed(&self) -> &[u8; 32] {
        &self.master_seed
    }

    // ── HD address derivation ─────────────────────────────────────────────────

    fn hd_root(&self) -> HdNode {
        HdNode::master(&self.master_seed)
    }

    /// Derive the transparent receive address at `m / HD_RECEIVE / index`.
    pub fn address_at(&self, index: u32) -> Address {
        self.hd_root()
            .child(HD_RECEIVE)
            .child(index)
            .to_keypair()
            .public_key
            .to_address()
    }

    /// Return the current (next unused) receive address without advancing the
    /// index.
    pub fn receive_address(&self) -> Address {
        self.address_at(self.receive_index)
    }

    /// Advance the receive index and return the new address.
    pub fn advance_receive_index(&mut self) -> Address {
        self.receive_index += 1;
        self.address_at(self.receive_index)
    }

    /// Derive the Base58Check-encoded receive address string.
    pub fn receive_address_string(&self) -> String {
        encode_address(&self.receive_address())
    }

    /// Return the `KeyPair` for the current receive address.
    pub fn receive_keypair(&self) -> KeyPair {
        self.hd_root()
            .child(HD_RECEIVE)
            .child(self.receive_index)
            .to_keypair()
    }

    /// Return the validator signing `KeyPair` (HD path `m / HD_VALIDATOR / 0`).
    pub fn validator_keypair(&self) -> KeyPair {
        self.hd_root()
            .child(HD_VALIDATOR)
            .child(0)
            .to_keypair()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_seed_is_deterministic() {
        let seed = [0x42u8; 32];
        let w1 = Wallet::from_seed(seed);
        let w2 = Wallet::from_seed(seed);
        assert_eq!(w1.address_at(0), w2.address_at(0));
        assert_eq!(w1.validator_keypair().public_key, w2.validator_keypair().public_key);
        // Privacy keys must also be deterministic from seed.
        assert_eq!(w1.privacy.payment_address, w2.privacy.payment_address);
        // Different seed → different payment address.
        let w3 = Wallet::from_seed([0x43u8; 32]);
        assert_ne!(w1.privacy.payment_address, w3.privacy.payment_address);
    }

    #[test]
    fn different_seeds_different_addresses() {
        let w1 = Wallet::from_seed([1u8; 32]);
        let w2 = Wallet::from_seed([2u8; 32]);
        assert_ne!(w1.address_at(0), w2.address_at(0));
    }

    #[test]
    fn different_indices_different_addresses() {
        let w = Wallet::from_seed([0xABu8; 32]);
        assert_ne!(w.address_at(0), w.address_at(1));
        assert_ne!(w.address_at(1), w.address_at(2));
    }

    #[test]
    fn advance_receive_index_increments() {
        let mut w = Wallet::from_seed([5u8; 32]);
        assert_eq!(w.receive_index, 0);
        let addr0 = w.receive_address();
        let addr1 = w.advance_receive_index();
        assert_eq!(w.receive_index, 1);
        assert_ne!(addr0, addr1);
    }

    #[test]
    fn receive_does_not_advance_index() {
        let w = Wallet::from_seed([7u8; 32]);
        let a1 = w.receive_address();
        let a2 = w.receive_address();
        assert_eq!(a1, a2);
    }

    #[test]
    fn validator_and_receive_keys_are_distinct() {
        let w = Wallet::from_seed([9u8; 32]);
        assert_ne!(
            w.receive_keypair().public_key,
            w.validator_keypair().public_key
        );
    }

    #[test]
    fn generate_produces_unique_wallets() {
        let w1 = Wallet::generate();
        let w2 = Wallet::generate();
        assert_ne!(w1.master_seed(), w2.master_seed());
    }
}
