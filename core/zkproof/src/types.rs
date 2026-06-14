use serde::{Deserialize, Serialize};
use thiserror::Error;
use winterfell::{
    crypto::{hashers::Blake3_256, DefaultRandomCoin, MerkleTree},
    math::fields::f64::BaseElement as Felt,
    AcceptableOptions, Prover,
};

use crate::circuit::{SpendCircuit, SpendPublicInputs};
use crate::prover::{
    build_trace, compute_merkle_root, compute_nullifier, default_proof_options, SpendProver,
    SpendWitness,
};

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StarkSpendProof {
    pub proof_bytes: Vec<u8>,
    pub anchor_felts: [u64; 4],
}

// ── Errors ────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum ZkError {
    #[error("STARK proof generation failed: {0}")]
    ProvingFailed(String),

    #[error("STARK proof verification failed")]
    VerificationFailed,

    #[error("proof deserialisation error: {0}")]
    Deserialisation(String),
}

// ── prove / verify ────────────────────────────────────────────────────────────

// Goldilocks p = 2^64 - 2^32 + 1; Felt::new(v) requires v < p.
const GOLDILOCKS_P: u64 = 0xFFFF_FFFF_0000_0001;

#[inline(always)]
fn felt(v: u64) -> Felt {
    Felt::new(if v >= GOLDILOCKS_P {
        v - GOLDILOCKS_P
    } else {
        v
    })
}

pub fn prove_spend(
    nullifier_key: [u64; 4],
    note_commitment: [u64; 4],
    siblings: [[u64; 4]; 20],
) -> Result<(StarkSpendProof, [u64; 4]), ZkError> {
    let nk: [Felt; 4] = nullifier_key.map(felt);
    let cm: [Felt; 4] = note_commitment.map(felt);
    let sibs: [[Felt; 4]; 20] = siblings.map(|s| s.map(felt));

    let nullifier = compute_nullifier(nk, cm);
    let anchor = compute_merkle_root(cm, &sibs);

    let witness = SpendWitness {
        nullifier_key: nk,
        note_commitment: cm,
        siblings: sibs,
    };

    let trace = build_trace(&witness);

    let options = default_proof_options();
    let prover = SpendProver::new(witness, options);

    let stark_proof = prover
        .prove(trace)
        .map_err(|e| ZkError::ProvingFailed(e.to_string()))?;

    let proof_bytes = stark_proof.to_bytes();
    let anchor_felts = anchor.map(|f| f.as_int());
    let nullifier_u64 = nullifier.map(|f| f.as_int());

    Ok((
        StarkSpendProof {
            proof_bytes,
            anchor_felts,
        },
        nullifier_u64,
    ))
}

/// Verify a STARK spend proof.
///
/// `nullifier_u64` is the public nullifier.  The anchor is taken directly from
/// `proof.anchor_felts` — the values baked in at prove time — so callers do not
/// need to pass it separately.  Callers should independently check that
/// `proof.anchor_felts` corresponds to a recent known Blake3 tree root.
pub fn verify_spend(proof: &StarkSpendProof, nullifier_u64: [u64; 4]) -> Result<(), ZkError> {
    let raw = winterfell::Proof::from_bytes(&proof.proof_bytes)
        .map_err(|e| ZkError::Deserialisation(format!("{e:?}")))?;

    let pub_inputs = SpendPublicInputs {
        nullifier: nullifier_u64.map(felt),
        anchor: proof.anchor_felts.map(felt),
    };

    winterfell::verify::<
        SpendCircuit,
        Blake3_256<Felt>,
        DefaultRandomCoin<Blake3_256<Felt>>,
        MerkleTree<Blake3_256<Felt>>,
    >(
        raw,
        pub_inputs,
        &AcceptableOptions::MinConjecturedSecurity(95),
    )
    .map_err(|_| ZkError::VerificationFailed)
}

/// Verify a STARK spend proof from its raw components.
///
/// Convenience wrapper for callers (e.g. `rupeecoin-privacy`) that hold
/// `proof_bytes`, `anchor_felts`, and `nullifier_felts` as separate fields
/// and want to avoid constructing a [`StarkSpendProof`] from a foreign crate.
pub fn verify_spend_raw(
    proof_bytes: &[u8],
    anchor_felts: [u64; 4],
    nullifier_felts: [u64; 4],
) -> Result<(), ZkError> {
    let proof = StarkSpendProof {
        proof_bytes: proof_bytes.to_vec(),
        anchor_felts,
    };
    verify_spend(&proof, nullifier_felts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prove_and_verify_roundtrip() {
        let nk: [u64; 4] = [1, 2, 3, 4];
        let cm: [u64; 4] = [5, 6, 7, 8];
        let siblings: [[u64; 4]; 20] = [[9, 10, 11, 12]; 20];

        let (proof, nullifier) =
            prove_spend(nk, cm, siblings).expect("prove_spend must succeed");

        verify_spend_raw(&proof.proof_bytes, proof.anchor_felts, nullifier)
            .expect("valid proof must verify");
    }

    #[test]
    fn verify_wrong_nullifier_fails() {
        let nk: [u64; 4] = [1, 2, 3, 4];
        let cm: [u64; 4] = [5, 6, 7, 8];
        let siblings: [[u64; 4]; 20] = [[9, 10, 11, 12]; 20];

        let (proof, nullifier) =
            prove_spend(nk, cm, siblings).expect("prove_spend must succeed");

        let bad = [nullifier[0].wrapping_add(1), nullifier[1], nullifier[2], nullifier[3]];
        assert!(
            verify_spend_raw(&proof.proof_bytes, proof.anchor_felts, bad).is_err(),
            "tampered nullifier must fail verification"
        );
    }
}
