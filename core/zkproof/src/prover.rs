use winterfell::{
    crypto::{hashers::Blake3_256, DefaultRandomCoin, MerkleTree},
    math::{fields::f64::BaseElement as Felt, FieldElement},
    matrix::ColMatrix,
    AuxRandElements, BatchingMethod, CompositionPoly, CompositionPolyTrace,
    ConstraintCompositionCoefficients, DefaultConstraintCommitment, DefaultConstraintEvaluator,
    DefaultTraceLde, FieldExtension, PartitionOptions, ProofOptions, Prover, StarkDomain,
    TraceInfo, TracePolyTable, TraceTable,
};

use crate::circuit::{SpendCircuit, SpendPublicInputs};
use crate::hash::compress;

// Witness: private inputs to the spend circuit.
pub struct SpendWitness {
    /// Nullifier key (4 field elements)
    pub nullifier_key: [Felt; 4],
    /// Note commitment — private, hidden by the STARK proof
    pub note_commitment: [Felt; 4],
    /// 20 Merkle sibling nodes (one per tree level)
    pub siblings: [[Felt; 4]; 20],
}

/// Compute nullifier = Poseidon(nk | cm).
pub fn compute_nullifier(nk: [Felt; 4], cm: [Felt; 4]) -> [Felt; 4] {
    compress(nk, cm)
}

/// Compute the Merkle root by hashing cm up 20 levels with the given siblings.
pub fn compute_merkle_root(cm: [Felt; 4], siblings: &[[Felt; 4]; 20]) -> [Felt; 4] {
    let mut node = cm;
    for sib in siblings.iter() {
        node = compress(node, *sib);
    }
    node
}

/// Default proof options: blowup=8, quadratic extension, 32 queries → ~96 bits.
///
/// `FieldExtension::Quadratic` lifts the Goldilocks field to GF(p^2) ≈ 2^128,
/// removing the ~63-bit field-size cap on FRI security and letting the
/// 32 queries × log₂(8) = 96-bit budget reach the 95-bit target.
pub fn default_proof_options() -> ProofOptions {
    ProofOptions::new(
        32,
        8,
        0,
        FieldExtension::Quadratic,
        8,
        31,
        BatchingMethod::Linear,
        BatchingMethod::Linear,
    )
}

pub struct SpendProver {
    witness: SpendWitness,
    options: ProofOptions,
}

impl SpendProver {
    pub fn new(witness: SpendWitness, options: ProofOptions) -> Self {
        SpendProver { witness, options }
    }
}

impl Prover for SpendProver {
    type BaseField = Felt;
    type Air = SpendCircuit;
    type Trace = TraceTable<Felt>;
    type HashFn = Blake3_256<Felt>;
    type VC = MerkleTree<Self::HashFn>;
    type RandomCoin = DefaultRandomCoin<Self::HashFn>;
    type TraceLde<E: FieldElement<BaseField = Felt>> = DefaultTraceLde<E, Self::HashFn, Self::VC>;
    type ConstraintCommitment<E: FieldElement<BaseField = Felt>> =
        DefaultConstraintCommitment<E, Self::HashFn, Self::VC>;
    type ConstraintEvaluator<'a, E: FieldElement<BaseField = Felt>> =
        DefaultConstraintEvaluator<'a, Self::Air, E>;

    fn get_pub_inputs(&self, _trace: &Self::Trace) -> SpendPublicInputs {
        let nk = self.witness.nullifier_key;
        let cm = self.witness.note_commitment;
        let nullifier = compute_nullifier(nk, cm);
        let anchor = compute_merkle_root(cm, &self.witness.siblings);
        SpendPublicInputs { nullifier, anchor }
    }

    fn options(&self) -> &ProofOptions {
        &self.options
    }

    fn new_trace_lde<E: FieldElement<BaseField = Felt>>(
        &self,
        trace_info: &TraceInfo,
        main_trace: &ColMatrix<Felt>,
        domain: &StarkDomain<Felt>,
        partition_option: PartitionOptions,
    ) -> (Self::TraceLde<E>, TracePolyTable<E>) {
        DefaultTraceLde::new(trace_info, main_trace, domain, partition_option)
    }

    fn new_evaluator<'a, E: FieldElement<BaseField = Felt>>(
        &self,
        air: &'a Self::Air,
        aux_rand_elements: Option<AuxRandElements<E>>,
        composition_coefficients: ConstraintCompositionCoefficients<E>,
    ) -> Self::ConstraintEvaluator<'a, E> {
        DefaultConstraintEvaluator::new(air, aux_rand_elements, composition_coefficients)
    }

    fn build_constraint_commitment<E: FieldElement<BaseField = Felt>>(
        &self,
        composition_poly_trace: CompositionPolyTrace<E>,
        num_constraint_composition_columns: usize,
        domain: &StarkDomain<Felt>,
        partition_options: PartitionOptions,
    ) -> (Self::ConstraintCommitment<E>, CompositionPoly<E>) {
        DefaultConstraintCommitment::new(
            composition_poly_trace,
            num_constraint_composition_columns,
            domain,
            partition_options,
        )
    }
}

// Build the 12×256 execution trace from the spend witness.
pub fn build_trace(witness: &SpendWitness) -> TraceTable<Felt> {
    let nk = witness.nullifier_key;
    let cm = witness.note_commitment;
    let siblings = witness.siblings;

    // Row 0: [nk | cm | cm_carry], where cm_carry = cm throughout
    let init_state: [Felt; 12] = [
        nk[0], nk[1], nk[2], nk[3], cm[0], cm[1], cm[2], cm[3], cm[0], cm[1], cm[2], cm[3],
    ];

    let mut trace = TraceTable::new(12, 256);
    trace.fill(
        |state| state.copy_from_slice(&init_state),
        |step, state| {
            let hash_idx = step / 8;
            let round = step % 8;

            if round < 7 {
                // Constrained row: apply Poseidon round to cols 0-7
                crate::hash::poseidon_round(&mut state[..8], round);
                // cols 8-11 (cm-carry) untouched
            } else {
                // Free injection row: set up the NEXT hash's input
                if hash_idx == 0 {
                    // After nullifier hash: Merkle path starts with [cm, sib0]
                    state[..4].copy_from_slice(&cm);
                    state[4..8].copy_from_slice(&siblings[0]);
                    // cols 8-11 unchanged (remain cm)
                } else if hash_idx < 20 {
                    // After Merkle hash k: cols 0-3 already = running node
                    state[4..8].copy_from_slice(&siblings[hash_idx]);
                    // cols 8-11 unchanged (remain cm)
                } else {
                    // Rows 168+: zero the Poseidon state (cols 0-7) but leave
                    // cm-carry (cols 8-11) unchanged — the copy constraint fires
                    // at every step and requires cols 8-11 to be constant.
                    state[..8].fill(Felt::ZERO);
                }
            }
        },
    );
    trace
}
