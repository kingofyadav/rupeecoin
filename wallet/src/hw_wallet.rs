#![allow(dead_code)] // Phase 14 wires the trait into real Ledger/Trezor devices.

/// Phase 13.4 — Hardware wallet abstraction.
///
/// Defines the `HardwareWallet` trait that all hardware signing devices must
/// implement, plus a `MockHardwareWallet` for tests and CI.
///
/// # Supported protocols (Phase 13)
/// Only `MockHardwareWallet` is fully implemented.  Real device support
/// (Ledger via HID, Trezor via USB-HID) is planned for Phase 14.
///
/// # Usage
/// ```
/// let hw = MockHardwareWallet::new(seed);
/// let sig = hw.sign_transaction(&tx_bytes, path).unwrap();
/// ```
use rupeecoin_crypto::{KeyPair, HdNode, HD_RECEIVE, Signature};

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum HwError {
    /// The device is not connected or not recognised.
    NotConnected(String),
    /// The user rejected the signing request on the device.
    UserRejected,
    /// Communication with the device failed.
    Transport(String),
    /// The derivation path is not supported by this device/app.
    UnsupportedPath(String),
}

impl std::fmt::Display for HwError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HwError::NotConnected(m) => write!(f, "hardware wallet not connected: {m}"),
            HwError::UserRejected => write!(f, "transaction rejected on hardware wallet"),
            HwError::Transport(m) => write!(f, "hardware wallet transport error: {m}"),
            HwError::UnsupportedPath(m) => write!(f, "unsupported derivation path: {m}"),
        }
    }
}

// ── Protocol enum ─────────────────────────────────────────────────────────────

/// Hardware wallet protocols supported by RupeeCoin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HwProtocol {
    /// Ledger device running the RupeeCoin Ledger app (Phase 14).
    Ledger,
    /// Trezor device (Phase 14).
    Trezor,
    /// In-memory mock for tests.
    Mock,
}

// ── Derivation path ───────────────────────────────────────────────────────────

/// BIP-32-style derivation path, simplified to a sequence of child indices.
///
/// RupeeCoin uses `m / HD_RECEIVE / <index>` for transparent addresses.
#[derive(Clone, Debug)]
pub struct DerivationPath {
    pub indices: Vec<u32>,
}

impl DerivationPath {
    /// Standard transparent receive path at `index`.
    pub fn receive(index: u32) -> Self {
        DerivationPath { indices: vec![HD_RECEIVE, index] }
    }
}

impl std::fmt::Display for DerivationPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "m")?;
        for idx in &self.indices {
            write!(f, "/{idx}")?;
        }
        Ok(())
    }
}

// ── Trait ─────────────────────────────────────────────────────────────────────

/// All hardware signing devices implement this trait.
///
/// Methods are synchronous (blocking) because real USB/HID communication is
/// already blocking; callers should move to a thread-pool if needed.
pub trait HardwareWallet: Send + Sync {
    /// Which protocol/device type this is.
    fn protocol(&self) -> HwProtocol;

    /// Derive the compressed Dilithium3 public key at `path`.
    fn get_public_key(&self, path: &DerivationPath) -> Result<Vec<u8>, HwError>;

    /// Sign `data` with the key at `path`.  The device may display a human-
    /// readable summary and wait for the user to confirm.
    fn sign(&self, data: &[u8], path: &DerivationPath) -> Result<Signature, HwError>;

    /// Optional: return a human-readable device name / firmware version.
    fn device_info(&self) -> String {
        format!("{:?}", self.protocol())
    }
}

// ── Mock implementation ───────────────────────────────────────────────────────

/// In-memory hardware wallet backed by an HD master seed.
///
/// Suitable for unit tests and integration tests where no physical device is
/// available.  Signs immediately without user confirmation.
pub struct MockHardwareWallet {
    master: HdNode,
}

impl MockHardwareWallet {
    /// Create a mock device from a 32-byte HD master seed.
    pub fn new(master_seed: &[u8; 32]) -> Self {
        MockHardwareWallet { master: HdNode::master(master_seed) }
    }

    fn derive_keypair(&self, path: &DerivationPath) -> KeyPair {
        let mut node = self.master.clone();
        for &idx in &path.indices {
            node = node.child(idx);
        }
        node.to_keypair()
    }
}

impl HardwareWallet for MockHardwareWallet {
    fn protocol(&self) -> HwProtocol {
        HwProtocol::Mock
    }

    fn get_public_key(&self, path: &DerivationPath) -> Result<Vec<u8>, HwError> {
        let kp = self.derive_keypair(path);
        Ok(kp.public_key.as_bytes().to_vec())
    }

    fn sign(&self, data: &[u8], path: &DerivationPath) -> Result<Signature, HwError> {
        let kp = self.derive_keypair(path);
        Ok(kp.sign(data))
    }

    fn device_info(&self) -> String {
        "MockHardwareWallet v1.0 (in-memory, no user confirmation)".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_seed() -> [u8; 32] {
        [0x42u8; 32]
    }

    #[test]
    fn mock_hw_get_public_key() {
        let hw = MockHardwareWallet::new(&test_seed());
        let path = DerivationPath::receive(0);
        let pk = hw.get_public_key(&path).unwrap();
        assert!(!pk.is_empty(), "public key should not be empty");
    }

    #[test]
    fn mock_hw_sign_and_verify() {
        let hw = MockHardwareWallet::new(&test_seed());
        let path = DerivationPath::receive(0);
        let message = b"test transaction data";

        let sig = hw.sign(message, &path).unwrap();

        // Derive the keypair manually and verify.
        let seed = test_seed();
        let master = HdNode::master(&seed);
        let kp = master.child(HD_RECEIVE).child(0).to_keypair();
        kp.public_key.verify(message, &sig).expect("signature should verify");
    }

    #[test]
    fn mock_hw_different_paths_give_different_keys() {
        let hw = MockHardwareWallet::new(&test_seed());
        let pk0 = hw.get_public_key(&DerivationPath::receive(0)).unwrap();
        let pk1 = hw.get_public_key(&DerivationPath::receive(1)).unwrap();
        assert_ne!(pk0, pk1, "different indices must give different keys");
    }

    #[test]
    fn derivation_path_display() {
        let p = DerivationPath::receive(3);
        assert_eq!(p.to_string(), format!("m/{HD_RECEIVE}/3"));
    }

    #[test]
    fn mock_hw_device_info() {
        let hw = MockHardwareWallet::new(&test_seed());
        assert!(hw.device_info().contains("Mock"));
        assert_eq!(hw.protocol(), HwProtocol::Mock);
    }
}
