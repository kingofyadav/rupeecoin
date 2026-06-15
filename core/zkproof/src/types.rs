use serde::{Deserialize, Serialize};
use thiserror::Error;
use winterfell::{
    crypto::{hashers::Blake3_256, DefaultRandomCoin, MerkleTree},
    math::fields::f64::BaseElement as Felt,
    AcceptableOptions, Prover,
};

use crate::batch::{build_batched_trace, BatchedSpendPublicInputs, BatchSpendProver};
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

// ── Batched STARK spend proof ─────────────────────────────────────────────────

/// A single STARK proof covering up to 10 shielded inputs in one pass.
///
/// The proof is produced by `BatchedSpendCircuit` whose trace is
/// 256 × next_power_of_2(N) rows (N ≤ 10).  Verification takes roughly the
/// same time as one individual proof regardless of N.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchedStarkSpendProof {
    /// Winterfell STARK proof bytes.
    pub proof_bytes: Vec<u8>,
    /// Poseidon commitment tree anchor — all N inputs share the same tree root.
    pub anchor_felts: [u64; 4],
    /// Poseidon nullifier for each spend (4 Goldilocks u64s each), in order.
    ///
    /// These are the STARK-internal public inputs pinned at row 7 of each block.
    /// The verifier checks the proof is consistent with exactly these values.
    /// (On-chain BLAKE3 nullifiers in `ShieldedInput` guard double-spend separately.)
    pub nullifier_felts: Vec<[u64; 4]>,
    /// How many real spends are covered (1..=10).
    pub num_spends: u32,
}

/// Per-spend input for the batch prover (u64 arrays, mirrors `prove_spend`).
pub struct BatchSpendInput {
    pub nullifier_key: [u64; 4],
    pub note_commitment: [u64; 4],
    pub siblings: [[u64; 4]; 20],
    pub is_right: [bool; 20],
}

/// Prove up to 10 shielded spends in one aggregated STARK proof.
///
/// All inputs must belong to the same commitment tree snapshot (same anchor).
/// Returns `(proof, nullifiers)` where `nullifiers[i]` is the Poseidon nullifier
/// for spend `i`.
pub fn prove_spend_batch(
    inputs: &[BatchSpendInput],
) -> Result<(BatchedStarkSpendProof, Vec<[u64; 4]>), ZkError> {
    if inputs.is_empty() {
        return Err(ZkError::ProvingFailed("batch must have at least 1 input".into()));
    }
    if inputs.len() > 10 {
        return Err(ZkError::ProvingFailed("batch size exceeds maximum of 10".into()));
    }

    // Convert to Felt witnesses
    let witnesses: Vec<SpendWitness> = inputs
        .iter()
        .map(|inp| SpendWitness {
            nullifier_key: inp.nullifier_key.map(felt),
            note_commitment: inp.note_commitment.map(felt),
            siblings: inp.siblings.map(|s| s.map(felt)),
            is_right: inp.is_right,
        })
        .collect();

    // Compute public inputs (nullifiers + anchors) — verify all anchors match
    let mut nullifiers: Vec<[Felt; 4]> = Vec::with_capacity(witnesses.len());
    let mut anchors: Vec<[Felt; 4]> = Vec::with_capacity(witnesses.len());

    for w in &witnesses {
        nullifiers.push(compute_nullifier(w.nullifier_key, w.note_commitment));
        anchors.push(compute_merkle_root(w.note_commitment, &w.siblings, &w.is_right));
    }

    // All spends must share the same tree root
    let shared_anchor = anchors[0];
    for (i, anc) in anchors.iter().enumerate() {
        if *anc != shared_anchor {
            return Err(ZkError::ProvingFailed(format!(
                "anchor mismatch at input {i}: all batch inputs must use the same commitment tree snapshot"
            )));
        }
    }

    let pub_inputs = BatchedSpendPublicInputs { nullifiers: nullifiers.clone(), anchors };

    // Always use ≥ 2 blocks so the 256-row period is repeated at least once.
    // This keeps the cm-carry polynomial non-trivial for N=1, matching the
    // with_cycles(1,[256]) degree declaration in BatchedSpendCircuit.
    let total_blocks = inputs.len().next_power_of_two().max(2);
    let trace = build_batched_trace(&witnesses, total_blocks);
    let prover = BatchSpendProver::new(pub_inputs);

    let stark_proof = prover
        .prove(trace)
        .map_err(|e| ZkError::ProvingFailed(e.to_string()))?;

    let proof_bytes = stark_proof.to_bytes();
    let anchor_felts = shared_anchor.map(|f| f.as_int());
    let nullifiers_u64: Vec<[u64; 4]> =
        nullifiers.iter().map(|n| n.map(|f| f.as_int())).collect();

    Ok((
        BatchedStarkSpendProof {
            proof_bytes,
            anchor_felts,
            nullifier_felts: nullifiers_u64.clone(),
            num_spends: inputs.len() as u32,
        },
        nullifiers_u64,
    ))
}

/// Verify a batched STARK spend proof.
///
/// Uses the nullifier felts embedded in the proof (placed there by
/// `prove_spend_batch`).  The STARK verifier checks the proof is consistent
/// with exactly those values, so a wallet that tampers with `nullifier_felts`
/// will get a verification failure.
pub fn verify_spend_batch(proof: &BatchedStarkSpendProof) -> Result<(), ZkError> {
    let n = proof.num_spends as usize;
    if proof.nullifier_felts.len() != n {
        return Err(ZkError::VerificationFailed);
    }

    let raw = winterfell::Proof::from_bytes(&proof.proof_bytes)
        .map_err(|e| ZkError::Deserialisation(format!("{e:?}")))?;

    let anc_felt: [Felt; 4] = proof.anchor_felts.map(felt);
    let pub_inputs = BatchedSpendPublicInputs {
        nullifiers: proof.nullifier_felts.iter().map(|n| n.map(felt)).collect(),
        anchors: (0..n).map(|_| anc_felt).collect(),
    };

    use crate::batch::BatchedSpendCircuit;
    winterfell::verify::<
        BatchedSpendCircuit,
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

#[cfg(test)]
mod tests {
    use super::*;

    const NK: [u64; 4] = [1, 2, 3, 4];
    const CM: [u64; 4] = [5, 6, 7, 8];
    const SIBLINGS: [[u64; 4]; 20] = [[9, 10, 11, 12]; 20];
    const IS_RIGHT_ALL_LEFT: [bool; 20] = [false; 20];
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

    // ── Batch tests ──────────────────────────────────────────────────────────
    //
    // Two leaves at positions 0 and 1 in a depth-20 Poseidon tree share the
    // same Merkle root when their level-0 siblings are each other's commitment:
    //
    //   Root = compress(... compress(compress(CM_A, CM_B), UPPER_SIB) ..., UPPER_SIB)
    //
    // Leaf A (is_right=[false;20]): siblings[0]=CM_B, siblings[1..]=UPPER_SIB
    // Leaf B (is_right=[true,false...]): siblings[0]=CM_A, siblings[1..]=UPPER_SIB
    //
    // Both compute the same root. ✓

    const CM_A: [u64; 4] = [5, 6, 7, 8];
    const CM_B: [u64; 4] = [50, 60, 70, 80];
    const UPPER_SIB: [u64; 4] = [9, 10, 11, 12];

    fn siblings_leaf_a() -> [[u64; 4]; 20] { let mut s = [UPPER_SIB; 20]; s[0] = CM_B; s }
    fn siblings_leaf_b() -> [[u64; 4]; 20] { let mut s = [UPPER_SIB; 20]; s[0] = CM_A; s }
    fn is_right_leaf_b() -> [bool; 20] { let mut b = [false; 20]; b[0] = true; b }

    fn make_single_input(nk: [u64; 4], cm: [u64; 4]) -> BatchSpendInput {
        BatchSpendInput { nullifier_key: nk, note_commitment: cm, siblings: SIBLINGS, is_right: IS_RIGHT_ALL_LEFT }
    }

    #[test]
    fn batch_single_input_proves_and_verifies() {
        let inputs = [make_single_input(NK, CM)];
        let (proof, nullifiers) =
            prove_spend_batch(&inputs).expect("batch prove must succeed for N=1");
        assert_eq!(nullifiers.len(), 1);
        assert_eq!(&proof.nullifier_felts, &nullifiers);
        verify_spend_batch(&proof).expect("batch verify must succeed for N=1");
    }

    #[test]
    fn batch_two_inputs_proves_and_verifies() {
        // Two distinct notes at positions 0 and 1 in the same Poseidon tree.
        // Their auth paths are constructed so both compute the same Merkle root.
        let input_a = BatchSpendInput {
            nullifier_key: NK,
            note_commitment: CM_A,
            siblings: siblings_leaf_a(),
            is_right: IS_RIGHT_ALL_LEFT,
        };
        let input_b = BatchSpendInput {
            nullifier_key: [10, 20, 30, 40],
            note_commitment: CM_B,
            siblings: siblings_leaf_b(),
            is_right: is_right_leaf_b(),
        };
        let inputs = [input_a, input_b];
        let (proof, nullifiers) =
            prove_spend_batch(&inputs).expect("batch prove must succeed for N=2");
        assert_eq!(nullifiers.len(), 2);
        assert_ne!(nullifiers[0], nullifiers[1], "distinct nk/cm → distinct nullifiers");
        verify_spend_batch(&proof).expect("batch verify must succeed for N=2");
    }

    #[test]
    fn batch_wrong_nullifier_fails() {
        let inputs = [make_single_input(NK, CM)];
        let (mut proof, _) = prove_spend_batch(&inputs).expect("prove must succeed");
        proof.nullifier_felts[0][0] = proof.nullifier_felts[0][0].wrapping_add(1);
        assert!(
            verify_spend_batch(&proof).is_err(),
            "tampered nullifier must fail batch verification"
        );
    }

    #[test]
    fn batch_rejects_too_many_inputs() {
        let inputs: Vec<BatchSpendInput> =
            (0u64..11).map(|i| make_single_input([i; 4], [i + 1; 4])).collect();
        assert!(
            prove_spend_batch(&inputs).is_err(),
            "11 inputs must be rejected (max is 10)"
        );
    }
}
