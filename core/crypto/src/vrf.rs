//! BLAKE3/Dilithium VRF for proposer selection.
//!
//! The Dilithium3 signature over the VRF input serves as the cryptographic
//! proof; the VRF output is derived deterministically from the proof bytes.
//! Any node can verify the output is correct given the public key.
use serde::{Deserialize, Serialize};

use crate::hash::{hash_data, Hash};
use crate::signature::{KeyPair, PublicKey, Signature};

/// A VRF proof-output pair.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VrfProof {
    /// Dilithium3 signature over the VRF input — the cryptographic proof.
    pub signature: Signature,
    /// Deterministic output: BLAKE3("rupeecoin-vrf-output" || sig_bytes || input_bytes).
    pub output: Hash,
}

impl VrfProof {
    /// Generate a VRF proof for `(slot, prev_vrf_output)` using `keypair`.
    pub fn generate(keypair: &KeyPair, slot: u64, prev_vrf_output: &Hash) -> Self {
        let msg = vrf_msg(slot, prev_vrf_output);
        let signature = keypair.sign(&msg);
        let output = vrf_output(&signature, slot, prev_vrf_output);
        VrfProof { signature, output }
    }

    /// Verify that this proof was produced by `pubkey` for `(slot, prev_vrf_output)`
    /// and that the `output` field is correct.
    pub fn verify(&self, pubkey: &PublicKey, slot: u64, prev_vrf_output: &Hash) -> bool {
        let msg = vrf_msg(slot, prev_vrf_output);
        pubkey.verify(&msg, &self.signature).is_ok()
            && self.output == vrf_output(&self.signature, slot, prev_vrf_output)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn vrf_msg(slot: u64, prev_vrf_output: &Hash) -> Vec<u8> {
    let mut msg = b"rupeecoin-vrf-input-v1".to_vec();
    msg.extend_from_slice(prev_vrf_output.as_bytes());
    msg.extend_from_slice(&slot.to_le_bytes());
    msg
}

fn vrf_output(sig: &Signature, slot: u64, prev_vrf_output: &Hash) -> Hash {
    let mut data = b"rupeecoin-vrf-output-v1".to_vec();
    data.extend_from_slice(sig.as_bytes());
    data.extend_from_slice(prev_vrf_output.as_bytes());
    data.extend_from_slice(&slot.to_le_bytes());
    hash_data(&data)
}
