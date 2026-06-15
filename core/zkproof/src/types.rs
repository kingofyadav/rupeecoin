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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StarkSpendProof {
    pub proof_bytes: Vec<u8>,
    pub anchor_felts: [u64; 4],
}

#[derive(Debug, Error)]
pub enum ZkError {
    #[error("STARK proof generation failed: {0}")]
    ProvingFailed(String),

    #[error("STARK proof verification failed")]
    VerificationFailed,

    #[error("proof deserialisation error: {0}")]
    Deserialisation(String),
}

const GOLDILOCKS_P: u64 = 0xFFFF_FFFF_0000_0001;

#[inline(always)]
fn felt(v: u64) -> Felt {
    Felt::new(if v >= GOLDILOCKS_P { v - GOLDILOCKS_P } else { v })
}

/// Generate a STARK spend proof.
///
/// `is_right[k]` is true when the spending note is the RIGHT child of its parent
/// at Merkle level k.  The `siblings` and `is_right` arrays come directly from
/// `CommitmentTree::auth_path()`.
pub fn prove_spend(
    nullifier_key: [u64; 4],
    note_commitment: [u64; 4],
    siblings: [[u64; 4]; 20],
    is_right: [bool; 20],
) -> Result<(StarkSpendProof, [u64; 4]), ZkError> {
    let nk: [Felt; 4] = nullifier_key.map(felt);
    let cm: [Felt; 4] = note_commitment.map(felt);
    let sibs: [[Felt; 4]; 20] = siblings.map(|s| s.map(felt));

    let nullifier = compute_nullifier(nk, cm);
    let anchor = compute_merkle_root(cm, &sibs, &is_right);

    let witness = SpendWitness {
        nullifier_key: nk,
        note_commitment: cm,
        siblings: sibs,
        is_right,
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

    Ok((StarkSpendProof { proof_bytes, anchor_felts }, nullifier_u64))
}

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

pub fn verify_spend_raw(
    proof_bytes: &[u8],
    anchor_felts: [u64; 4],
    nullifier_felts: [u64; 4],
) -> Result<(), ZkError> {
    let proof = StarkSpendProof { proof_bytes: proof_bytes.to_vec(), anchor_felts };
    verify_spend(&proof, nullifier_felts)
}

#[cfg(test)]
mod tests {
    use super::*;

    // All-left-child path: is_right = [false; 20]
    const NK: [u64; 4] = [1, 2, 3, 4];
    const CM: [u64; 4] = [5, 6, 7, 8];
    const SIBLINGS: [[u64; 4]; 20] = [[9, 10, 11, 12]; 20];
    const IS_RIGHT_ALL_LEFT: [bool; 20] = [false; 20];
    // Mixed path: leaf at index 3 → is_right = [true, true, false, false, ...]
    const IS_RIGHT_MIXED: [bool; 20] = {
        let mut b = [false; 20];
        b[0] = true;
        b[1] = true;
        b
    };

    #[test]
    fn prove_and_verify_all_left() {
        let (proof, nullifier) =
            prove_spend(NK, CM, SIBLINGS, IS_RIGHT_ALL_LEFT).expect("prove_spend must succeed");
        verify_spend_raw(&proof.proof_bytes, proof.anchor_felts, nullifier)
            .expect("valid proof must verify");
    }

    #[test]
    fn prove_and_verify_mixed_direction() {
        let (proof, nullifier) =
            prove_spend(NK, CM, SIBLINGS, IS_RIGHT_MIXED).expect("prove_spend mixed must succeed");
        verify_spend_raw(&proof.proof_bytes, proof.anchor_felts, nullifier)
            .expect("mixed direction proof must verify");
    }

    #[test]
    fn verify_wrong_nullifier_fails() {
        let (proof, nullifier) =
            prove_spend(NK, CM, SIBLINGS, IS_RIGHT_ALL_LEFT).expect("prove_spend must succeed");
        let bad = [nullifier[0].wrapping_add(1), nullifier[1], nullifier[2], nullifier[3]];
        assert!(
            verify_spend_raw(&proof.proof_bytes, proof.anchor_felts, bad).is_err(),
            "tampered nullifier must fail verification"
        );
    }
}
