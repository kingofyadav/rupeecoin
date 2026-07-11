//! BLAKE3/Dilithium VRF for proposer selection.
//!
//! The Dilithium3 signature over the VRF input serves as the cryptographic
//! proof; the VRF output is derived deterministically from the proof bytes.
//! Any node can verify the output is correct given the public key.
//!
//! # ⚠ Security: this is NOT a uniqueness-secure VRF (pre-mainnet blocker)
//!
//! A sound VRF requires **uniqueness**: for a given key and input there is
//! exactly one output any verifier will accept. Dilithium signatures are *not*
//! unique — many distinct signatures verify for the same (key, message). A
//! malicious proposer can therefore generate alternative valid signatures,
//! observe the resulting `vrf_output`, and pick whichever most favourably
//! biases the *next* slot's `select_proposer` (which chains off this output).
//! This is stake-grinding on proposer election.
//!
//! Honest nodes are unaffected only because PQClean Dilithium3 signs
//! deterministically (guarded by `dilithium3_signing_is_deterministic` below);
//! that guard does **not** stop a malicious signer, who is free to randomise.
//!
//! Decision & migration path are recorded in `docs/adr/0001-vrf-uniqueness.md`.
//! Do not rely on this VRF for adversarial randomness on mainnet.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::hash_data;
    use crate::signature::KeyPair;

    /// Interim safeguard for the VRF uniqueness gap (see module docs and
    /// `docs/adr/0001-vrf-uniqueness.md`). Honest proposers only produce a
    /// single `vrf_output` per (key, slot) as long as Dilithium3 signing is
    /// deterministic. If a dependency bump ever switches PQClean to randomised
    /// signing, this test fails loudly — at which point even *honest* nodes
    /// would disagree on the VRF output and the chain would fork.
    #[test]
    fn dilithium3_signing_is_deterministic() {
        let kp = KeyPair::from_seed(&[7u8; 32]);
        let msg = b"rupeecoin-vrf-determinism-probe";
        let a = kp.sign(msg);
        let b = kp.sign(msg);
        assert_eq!(
            a.as_bytes(),
            b.as_bytes(),
            "Dilithium3 signing is non-deterministic in this build — the \
             signature-derived VRF is broken for honest nodes too. See \
             docs/adr/0001-vrf-uniqueness.md before shipping."
        );
    }

    /// A well-formed proof verifies and its output matches the derivation.
    #[test]
    fn vrf_proof_roundtrip() {
        let kp = KeyPair::from_seed(&[9u8; 32]);
        let prev = hash_data(b"prev");
        let proof = VrfProof::generate(&kp, 42, &prev);
        assert!(proof.verify(&kp.public_key, 42, &prev));
        // Wrong slot must not verify.
        assert!(!proof.verify(&kp.public_key, 43, &prev));
    }
}
