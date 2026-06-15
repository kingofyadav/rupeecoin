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

/// Private witness for one shielded spend.
pub struct SpendWitness {
    pub nullifier_key: [Felt; 4],
    pub note_commitment: [Felt; 4],
    pub siblings: [[Felt; 4]; 20],
    /// True at level k when the spending note is the RIGHT child in the Poseidon tree.
    /// The prover pre-swaps cols 0-3 / 4-7 so compress(cols[0..4], cols[4..8]) always
    /// yields the correct positional parent: compress(left, right).
    pub is_right: [bool; 20],
}

/// Compute nullifier = Poseidon(nk | cm).
pub fn compute_nullifier(nk: [Felt; 4], cm: [Felt; 4]) -> [Felt; 4] {
    compress(nk, cm)
}

/// Compute the Poseidon Merkle root from cm + siblings + direction bits.
///
/// Equivalent to `CommitmentTree::root()` for the leaf at the given path.
pub fn compute_merkle_root(
    cm: [Felt; 4],
    siblings: &[[Felt; 4]; 20],
    is_right: &[bool; 20],
) -> [Felt; 4] {
    let mut node = cm;
    for (i, sib) in siblings.iter().enumerate() {
        node = if is_right[i] {
            compress(*sib, node)   // right child: parent = compress(sibling, node)
        } else {
            compress(node, *sib)   // left child:  parent = compress(node, sibling)
        };
    }
    node
}

/// Default proof options: blowup=8, quadratic extension, 32 queries → ~96 bits security.
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
        let anchor = compute_merkle_root(cm, &self.witness.siblings, &self.witness.is_right);
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

/// Build the 12×256 execution trace from the spend witness.
///
/// Direction bits (is_right) control the left/right ordering at each Merkle level.
/// When the spending note is the RIGHT child at level k, the prover pre-swaps the
/// inputs so that cols[0..4] = sibling and cols[4..8] = node. The Poseidon constraint
/// then computes `compress(sibling, node)` = the correct positional parent.
pub fn build_trace(witness: &SpendWitness) -> TraceTable<Felt> {
    let nk = witness.nullifier_key;
    let cm = witness.note_commitment;
    let siblings = witness.siblings;
    let is_right = witness.is_right;

    // Row 0: [nk | cm | cm_carry]
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
                crate::hash::poseidon_round(&mut state[..8], round);
            } else {
                if hash_idx == 0 {
                    // After nullifier hash: set up Merkle level 0 input.
                    // If cm is the RIGHT child, put sibling on the left (cols 0-3)
                    // and cm on the right (cols 4-7) so compress(sib, cm) = correct parent.
                    if is_right[0] {
                        state[..4].copy_from_slice(&siblings[0]);
                        state[4..8].copy_from_slice(&cm);
                    } else {
                        state[..4].copy_from_slice(&cm);
                        state[4..8].copy_from_slice(&siblings[0]);
                    }
                } else if hash_idx < 20 {
                    // After Merkle hash k: the hash output is in cols 0-3.
                    // Set up next sibling; if node is a RIGHT child at level hash_idx,
                    // swap so sibling goes left and the running node goes right.
                    // The chaining constraint carries current[0..4] to next[0..4] OR
                    // next[4..8] (weakened product constraint), so we must also move
                    // the running node to cols 4-7 here when swapping.
                    if is_right[hash_idx] {
                        // running node was in cols 0-3 (hash output); move it to 4-7
                        // and put the sibling in 0-3.
                        let running = [state[0], state[1], state[2], state[3]];
                        state[..4].copy_from_slice(&siblings[hash_idx]);
                        state[4..8].copy_from_slice(&running);
                    } else {
                        state[4..8].copy_from_slice(&siblings[hash_idx]);
                        // cols 0-3 already hold running node (carried by constraint)
                    }
                } else {
                    state[..8].fill(Felt::ZERO);
                }
            }
        },
    );
    trace
}
