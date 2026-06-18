use serde::{Deserialize, Serialize};
use std::fmt;

use crate::signature::CryptoError;

// ── Hash ──────────────────────────────────────────────────────────────────────

/// 32-byte BLAKE3 digest.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Hash([u8; 32]);

impl Hash {
    pub fn zero() -> Self {
        Hash([0u8; 32])
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn from_bytes(b: [u8; 32]) -> Self {
        Hash(b)
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash({}…)", &hex::encode(self.0)[..8])
    }
}

pub fn hash_data(data: &[u8]) -> Hash {
    Hash(*blake3::hash(data).as_bytes())
}

pub fn hash_concat(a: &Hash, b: &Hash) -> Hash {
    // Domain-keyed to prevent collisions with hash_data / leaf_hash / node_hash outputs.
    let mut h = blake3::Hasher::new_derive_key("rupeecoin-hash-concat-v1");
    h.update(a.as_bytes());
    h.update(b.as_bytes());
    Hash(*h.finalize().as_bytes())
}

/// Domain-separated Merkle leaf hash: BLAKE3("rpc_leaf" || data).
pub fn leaf_hash(data: &[u8]) -> Hash {
    let mut h = blake3::Hasher::new();
    h.update(b"rpc_leaf");
    h.update(data);
    Hash(*h.finalize().as_bytes())
}

/// Domain-separated Merkle internal-node hash: BLAKE3("rpc_node" || left || right).
pub fn node_hash(left: &Hash, right: &Hash) -> Hash {
    let mut h = blake3::Hasher::new();
    h.update(b"rpc_node");
    h.update(left.as_bytes());
    h.update(right.as_bytes());
    Hash(*h.finalize().as_bytes())
}

// ── Address ───────────────────────────────────────────────────────────────────

/// 20-byte UTXO address: the first 20 bytes of BLAKE3(public_key_bytes).
/// Displayed as Base58Check with version byte 0x52 ('R' for RupeeCoin).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Address(pub [u8; 20]);

impl Address {
    pub fn zero() -> Self {
        Address([0u8; 20])
    }

    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", encode_address(self))
    }
}

impl fmt::Debug for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Address({})", encode_address(self))
    }
}

/// Version byte: 'R' = 0x52 for RupeeCoin transparent addresses.
const ADDRESS_VERSION: u8 = 0x52;

/// Encode a 20-byte address as Base58Check(0x52 || address_bytes).
///
/// Checksum = BLAKE3(BLAKE3(version || address_bytes))\[0:4\].
pub fn encode_address(addr: &Address) -> String {
    let mut payload = [0u8; 25]; // 1 version + 20 address + 4 checksum
    payload[0] = ADDRESS_VERSION;
    payload[1..21].copy_from_slice(&addr.0);
    let h1 = hash_data(&payload[..21]);
    let h2 = hash_data(h1.as_bytes());
    payload[21..25].copy_from_slice(&h2.as_bytes()[..4]);
    bs58::encode(&payload).into_string()
}

/// Decode a Base58Check address string.  Returns `Err` on version mismatch or bad checksum.
pub fn decode_address(s: &str) -> Result<Address, CryptoError> {
    let bytes = bs58::decode(s)
        .into_vec()
        .map_err(|_| CryptoError::InvalidAddress)?;
    if bytes.len() != 25 {
        return Err(CryptoError::InvalidAddress);
    }
    if bytes[0] != ADDRESS_VERSION {
        return Err(CryptoError::InvalidAddress);
    }
    let h1 = hash_data(&bytes[..21]);
    let h2 = hash_data(h1.as_bytes());
    if bytes[21..25] != h2.as_bytes()[..4] {
        return Err(CryptoError::InvalidChecksum);
    }
    let mut arr = [0u8; 20];
    arr.copy_from_slice(&bytes[1..21]);
    Ok(Address(arr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_roundtrip() {
        let addr = Address([1u8; 20]);
        let encoded = encode_address(&addr);
        let decoded = decode_address(&encoded).unwrap();
        assert_eq!(addr, decoded);
    }

    #[test]
    fn tampered_address_rejected() {
        let addr = Address([42u8; 20]);
        let mut encoded = encode_address(&addr).into_bytes();
        // Flip the last character
        let last = encoded.len() - 1;
        encoded[last] ^= 1;
        let s = String::from_utf8(encoded).unwrap();
        // May fail to decode as base58 or fail checksum
        let result = decode_address(&s);
        assert!(result.is_err());
    }

    #[test]
    fn leaf_hash_domain_separated() {
        let data = b"test";
        assert_ne!(leaf_hash(data), hash_data(data));
        assert_ne!(leaf_hash(data), node_hash(&Hash::zero(), &Hash::zero()));
    }
}
