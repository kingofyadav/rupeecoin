use winterfell::{
    crypto::{hashers::Blake3_256, DefaultRandomCoin, MerkleTree},
    math::{fields::f64::BaseElement as Felt, FieldElement, ToElements},
    matrix::ColMatrix,
    Air, AirContext, Assertion, AuxRandElements, BatchingMethod, CompositionPoly,
    CompositionPolyTrace, ConstraintCompositionCoefficients, DefaultConstraintCommitment,
    DefaultConstraintEvaluator, DefaultTraceLde, EvaluationFrame, FieldExtension, PartitionOptions,
    ProofOptions, Prover, StarkDomain, TraceInfo, TracePolyTable, TraceTable,
    TransitionConstraintDegree,
};

use crate::hash::{eval_poseidon_round, get_round_constants};
use crate::prover::SpendWitness;

// ── Public inputs ─────────────────────────────────────────────────────────────

/// Public inputs for a batched spend proof covering N shielded inputs.
///
/// Layout returned by `to_elements`: [nul_0[0..4], nul_1[0..4], ...,
/// anc_0[0..4], anc_1[0..4], ...] where N = nullifiers.len() == anchors.len().
#[derive(Clone, Debug)]
pub struct BatchedSpendPublicInputs {
    pub nullifiers: Vec<[Felt; 4]>,
    pub anchors: Vec<[Felt; 4]>,
}

impl ToElements<Felt> for BatchedSpendPublicInputs {
    fn to_elements(&self) -> Vec<Felt> {
        let mut v = Vec::with_capacity(8 * self.nullifiers.len());
        for nul in &self.nullifiers {
            v.extend_from_slice(nul);
        }
        for anc in &self.anchors {
            v.extend_from_slice(anc);
        }
        v
    }
}

// ── AIR ───────────────────────────────────────────────────────────────────────

/// AIR for batched spend proofs (up to 10 shielded inputs in one proof).
///
/// Trace layout: 12 columns × (256 × P) rows, where P = next_power_of_2(N).
///
/// Each 256-row "block" is one spend proof with the identical column layout
/// and constraints as `SpendCircuit`.  Blocks N..P are dummy padding blocks
/// (all-zero witness) that satisfy every constraint but carry no assertions.
///
/// The 8 assertions are expressed as `Assertion::sequence` with stride=256,
/// which pins the real nullifier/anchor values at rows {7, 263, 519, …} and
/// {167, 423, 679, …} respectively, leaving padding block rows unconstrained.
pub struct BatchedSpendCircuit {
    context: AirContext<Felt>,
    pub_inputs: BatchedSpendPublicInputs,
}

impl Air for BatchedSpendCircuit {
    type BaseField = Felt;
    type PublicInputs = BatchedSpendPublicInputs;

    fn new(
        trace_info: TraceInfo,
        pub_inputs: BatchedSpendPublicInputs,
        options: ProofOptions,
    ) -> Self {
        let mut degrees = Vec::with_capacity(24);

        // [0..8]   Poseidon round — degree 7, period 8
        for _ in 0..8 {
            degrees.push(TransitionConstraintDegree::with_cycles(7, vec![8]));
        }
        // [8..12]  cm-carry copy — degree 1 with period 256.
        //
        // Constraint: sel_not_block_end * (next[8+k] - current[8+k]) = 0.
        //
        // The trace always uses ≥ 2 blocks (total_blocks = max(2, next_pow2(N))),
        // so the trace length is always ≥ 512.  With P ≥ 2 blocks:
        //   • sel_not_block_end has polynomial degree (n/256)*255 = P*255
        //   • (next[8+k] - current[8+k]) has degree n-1 = 256P-1
        //     (cm-carry is block-constant but differs between blocks, making the
        //      difference non-zero at all P block-boundary steps)
        //   • product degree = P*255 + 256P-1 = 511P-1
        //   • after dividing by zerofier (degree n-1 = 256P-1): 511P-1 - (256P-1) = 255P
        //   • Winterfell expected (with_cycles(1,[256])): (n/256)*255 = P*255  ✓
        for _ in 0..4 {
            degrees.push(TransitionConstraintDegree::with_cycles(1, vec![256]));
        }
        // [12..16] row-0 cm-carry init — degree 1, period 256
        for _ in 0..4 {
            degrees.push(TransitionConstraintDegree::with_cycles(1, vec![256]));
        }
        // [16..20] row-8 cm-carry check — product form, degree 2, period 256
        for _ in 0..4 {
            degrees.push(TransitionConstraintDegree::with_cycles(2, vec![256]));
        }
        // [20..24] hash-chaining — product form, degree 2, period 256
        for _ in 0..4 {
            degrees.push(TransitionConstraintDegree::with_cycles(2, vec![256]));
        }

        // 8 assertion objects (4 nullifier cols + 4 anchor cols), each a sequence.
        // Periodic column layout: 12 existing (same as SpendCircuit) + 1 new (sel_not_block_end).
        let context = AirContext::new(trace_info, degrees, 8, options);
        BatchedSpendCircuit {
            context,
            pub_inputs,
        }
    }

    fn context(&self) -> &AirContext<Felt> {
        &self.context
    }

    /// Identical transition constraints to `SpendCircuit`.
    ///
    /// Periodic columns have periods 8 and 256, which divide any 256×P trace
    /// length (P = power of 2), so constraints fire correctly at every block.
    fn evaluate_transition<E: FieldElement<BaseField = Felt>>(
        &self,
        frame: &EvaluationFrame<E>,
        periodic_values: &[E],
        result: &mut [E],
    ) {
        let current = frame.current();
        let next = frame.next();

        let sel = periodic_values[0];
        let rc: [E; 8] = core::array::from_fn(|k| periodic_values[1 + k]);
        let sel_row0 = periodic_values[9];
        let sel_row8 = periodic_values[10];
        let sel_chain = periodic_values[11];
        let sel_not_blk_end = periodic_values[12];

        eval_poseidon_round(current, next, &rc, sel, result);

        // cm-carry copy: suppressed at block-end transitions (local step 255 → next block)
        // so the carry can legitimately reset to the new block's note commitment.
        for k in 0..4 {
            result[8 + k] = sel_not_blk_end * (next[8 + k] - current[8 + k]);
        }
        for k in 0..4 {
            result[12 + k] = sel_row0 * (current[8 + k] - current[4 + k]);
        }
        for k in 0..4 {
            let dl = current[8 + k] - current[k];
            let dr = current[8 + k] - current[4 + k];
            result[16 + k] = sel_row8 * dl * dr;
        }
        for k in 0..4 {
            let dl = next[k] - current[k];
            let dr = next[4 + k] - current[k];
            result[20 + k] = sel_chain * dl * dr;
        }
    }

    /// 8 sequence assertions — one per column (0..3) at nullifier and anchor rows.
    ///
    /// Each assertion covers only the N real spends (not the padding blocks), so
    /// `first_step + (N-1)*stride < trace_length` holds for any P ≥ N.
    fn get_assertions(&self) -> Vec<Assertion<Felt>> {
        let mut assertions = Vec::with_capacity(8);

        for k in 0..4 {
            let vals: Vec<Felt> = self.pub_inputs.nullifiers.iter().map(|n| n[k]).collect();
            assertions.push(Assertion::sequence(k, 7, 256, vals));
        }
        for k in 0..4 {
            let vals: Vec<Felt> = self.pub_inputs.anchors.iter().map(|a| a[k]).collect();
            assertions.push(Assertion::sequence(k, 167, 256, vals));
        }

        assertions
    }

    /// Same periodic columns as `SpendCircuit`.
    ///
    /// Periods 8 and 256 both divide 256×P for any integer P ≥ 1, so they
    /// repeat correctly across all blocks in the batched trace.
    fn get_periodic_column_values(&self) -> Vec<Vec<Felt>> {
        let rc = get_round_constants();
        let mut columns = Vec::with_capacity(12);

        columns.push(vec![
            Felt::ONE,
            Felt::ONE,
            Felt::ONE,
            Felt::ONE,
            Felt::ONE,
            Felt::ONE,
            Felt::ONE,
            Felt::ZERO,
        ]);
        for col in 0..8 {
            let mut v: Vec<Felt> = rc.iter().map(|round| round[col]).collect();
            v.push(Felt::ZERO);
            columns.push(v);
        }

        let mut sel_row0 = vec![Felt::ZERO; 256];
        sel_row0[0] = Felt::ONE;
        columns.push(sel_row0);

        let mut sel_row8 = vec![Felt::ZERO; 256];
        sel_row8[8] = Felt::ONE;
        columns.push(sel_row8);

        let mut sel_chain = vec![Felt::ZERO; 256];
        for k in 1..20usize {
            sel_chain[8 * k + 7] = Felt::ONE;
        }
        columns.push(sel_chain);

        // sel_not_block_end: period 256, 0 at position 255, 1 everywhere else.
        // Gates the cm-carry copy constraint so it does not fire at the step that
        // transitions from one spend block to the next (local row 255 → row 0).
        let mut sel_not_block_end = vec![Felt::ONE; 256];
        sel_not_block_end[255] = Felt::ZERO;
        columns.push(sel_not_block_end);

        columns
    }
}

// ── Trace builder ─────────────────────────────────────────────────────────────

/// Build a batched 12-column × (256 × `total_blocks`) execution trace.
///
/// Each 256-row block follows the same layout as the single-spend trace.
/// Block boundaries (local step 255 → next block's row 0) are handled by
/// the update closure: instead of continuing the padding evolution, it resets
/// `state` to the initial values of the next block.
///
/// Padding blocks (`k >= witnesses.len()`) use an all-zero witness; every
/// constraint is still satisfied because we run valid Poseidon on zeros.
pub fn build_batched_trace(witnesses: &[SpendWitness], total_blocks: usize) -> TraceTable<Felt> {
    debug_assert!(total_blocks >= witnesses.len());
    debug_assert!(total_blocks.is_power_of_two());

    let n = witnesses.len();

    // Pre-copy all block data (owned) so closures below have no borrow issues.
    // (nullifier_key, note_commitment, siblings, is_right) per 256-row block.
    type BlockWitness = ([Felt; 4], [Felt; 4], [[Felt; 4]; 20], [bool; 20]);
    let blocks: Vec<BlockWitness> = (0..total_blocks)
        .map(|k| {
            if k < n {
                (
                    witnesses[k].nullifier_key,
                    witnesses[k].note_commitment,
                    witnesses[k].siblings,
                    witnesses[k].is_right,
                )
            } else {
                (
                    [Felt::ZERO; 4],
                    [Felt::ZERO; 4],
                    [[Felt::ZERO; 4]; 20],
                    [false; 20],
                )
            }
        })
        .collect();

    let (nk0, cm0, _, _) = blocks[0];
    let init: [Felt; 12] = [
        nk0[0], nk0[1], nk0[2], nk0[3], cm0[0], cm0[1], cm0[2], cm0[3], cm0[0], cm0[1], cm0[2],
        cm0[3],
    ];

    let mut trace = TraceTable::new(12, 256 * total_blocks);
    trace.fill(
        |state| state.copy_from_slice(&init),
        |step, state| {
            let block = step / 256;
            let local_step = step % 256;
            let (_, cm, siblings, is_right) = &blocks[block];

            // Block boundary: reset to next block's initial state.
            if local_step == 255 {
                let nb = block + 1;
                if nb < total_blocks {
                    let (nk1, cm1, _, _) = blocks[nb];
                    state[..4].copy_from_slice(&nk1);
                    state[4..8].copy_from_slice(&cm1);
                    state[8..12].copy_from_slice(&cm1);
                }
                return;
            }

            let hash_idx = local_step / 8;
            let round = local_step % 8;

            if round < 7 {
                crate::hash::poseidon_round(&mut state[..8], round);
            } else if hash_idx == 0 {
                if is_right[0] {
                    state[..4].copy_from_slice(&siblings[0]);
                    state[4..8].copy_from_slice(cm);
                } else {
                    state[..4].copy_from_slice(cm);
                    state[4..8].copy_from_slice(&siblings[0]);
                }
            } else if hash_idx < 20 {
                if is_right[hash_idx] {
                    let running = [state[0], state[1], state[2], state[3]];
                    state[..4].copy_from_slice(&siblings[hash_idx]);
                    state[4..8].copy_from_slice(&running);
                } else {
                    state[4..8].copy_from_slice(&siblings[hash_idx]);
                }
            } else {
                state[..8].fill(Felt::ZERO);
            }
        },
    );

    trace
}

// ── Prover ────────────────────────────────────────────────────────────────────

pub struct BatchSpendProver {
    pub_inputs: BatchedSpendPublicInputs,
    options: ProofOptions,
}

impl BatchSpendProver {
    pub fn new(pub_inputs: BatchedSpendPublicInputs) -> Self {
        let options = ProofOptions::new(
            32,
            8,
            0,
            FieldExtension::Quadratic,
            8,
            31,
            BatchingMethod::Linear,
            BatchingMethod::Linear,
        );
        BatchSpendProver {
            pub_inputs,
            options,
        }
    }
}

impl Prover for BatchSpendProver {
    type BaseField = Felt;
    type Air = BatchedSpendCircuit;
    type Trace = TraceTable<Felt>;
    type HashFn = Blake3_256<Felt>;
    type VC = MerkleTree<Self::HashFn>;
    type RandomCoin = DefaultRandomCoin<Self::HashFn>;
    type TraceLde<E: FieldElement<BaseField = Felt>> = DefaultTraceLde<E, Self::HashFn, Self::VC>;
    type ConstraintCommitment<E: FieldElement<BaseField = Felt>> =
        DefaultConstraintCommitment<E, Self::HashFn, Self::VC>;
    type ConstraintEvaluator<'a, E: FieldElement<BaseField = Felt>> =
        DefaultConstraintEvaluator<'a, Self::Air, E>;

    fn get_pub_inputs(&self, _trace: &Self::Trace) -> BatchedSpendPublicInputs {
        self.pub_inputs.clone()
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
        num_cols: usize,
        domain: &StarkDomain<Felt>,
        partition_options: PartitionOptions,
    ) -> (Self::ConstraintCommitment<E>, CompositionPoly<E>) {
        DefaultConstraintCommitment::new(
            composition_poly_trace,
            num_cols,
            domain,
            partition_options,
        )
    }
}
