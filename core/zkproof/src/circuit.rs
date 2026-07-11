use winterfell::{
    math::{fields::f64::BaseElement as Felt, FieldElement, ToElements},
    Air, AirContext, Assertion, EvaluationFrame, ProofOptions, TraceInfo,
    TransitionConstraintDegree,
};

use crate::hash::{eval_poseidon_round, get_round_constants};

/// Public inputs: nullifier (cols 0-3 at row 7) + anchor (cols 0-3 at row 167).
#[derive(Clone, Debug)]
pub struct SpendPublicInputs {
    pub nullifier: [Felt; 4],
    pub anchor: [Felt; 4],
}

impl ToElements<Felt> for SpendPublicInputs {
    fn to_elements(&self) -> Vec<Felt> {
        let mut v = self.nullifier.to_vec();
        v.extend_from_slice(&self.anchor);
        v
    }
}

// AIR for the spend circuit.
//
// Trace layout (12 columns × 256 rows):
//   cols 0-7:  8-element Poseidon state
//   cols 8-11: cm-carry (constant copy of the private note commitment)
//
// Row layout:
//   rows  0-7:   hash 0 — nullifier = Poseidon(nk | cm)
//   rows  8-15:  hash 1 — Merkle level 0
//   rows 16-23:  hash 2 — Merkle level 1
//   ...
//   rows 160-167: hash 20 — Merkle level 19
//   rows 168-255: padding (zeros)
//
// Direction bits (is_right) are handled in the PROVER trace only:
// when the spending note is the right child at level k, the trace pre-swaps
// cols 0-3 and cols 4-7 so the Poseidon constraint still evaluates
// compress(cols[0..4], cols[4..8]) = correct positional parent.
//
// The row-8 cm-carry check and hash-chaining constraint use a product form
// to allow the node to appear in either cols 0-3 (left child) or cols 4-7
// (right child) without revealing which one — maintaining zero-knowledge.
pub struct SpendCircuit {
    context: AirContext<Felt>,
    pub_inputs: SpendPublicInputs,
}

impl Air for SpendCircuit {
    type BaseField = Felt;
    type PublicInputs = SpendPublicInputs;

    fn new(trace_info: TraceInfo, pub_inputs: SpendPublicInputs, options: ProofOptions) -> Self {
        let mut degrees = Vec::with_capacity(24);

        // [0..8]  Poseidon round: degree 7 with period 8
        for _ in 0..8 {
            degrees.push(TransitionConstraintDegree::with_cycles(7, vec![8]));
        }

        // [8..12] cm-carry copy: next[8+k] = current[8+k] at every step — degree 1
        for _ in 0..4 {
            degrees.push(TransitionConstraintDegree::new(1));
        }

        // [12..16] row-0 copy: cm_carry = nullifier-hash right-input (cols 4-7) — degree 1
        for _ in 0..4 {
            degrees.push(TransitionConstraintDegree::with_cycles(1, vec![256]));
        }

        // [16..20] row-8 cm check: cm_carry ∈ {cols[0..4], cols[4..8]} — degree 2 × periodic
        // Product form: sel_row8 * (cm_carry - left) * (cm_carry - right) = 0
        for _ in 0..4 {
            degrees.push(TransitionConstraintDegree::with_cycles(2, vec![256]));
        }

        // [20..24] hash-chaining: running node carried to left OR right input — degree 2 × periodic
        // Product form: sel_chain * (next[k] - current[k]) * (next[4+k] - current[k]) = 0
        for _ in 0..4 {
            degrees.push(TransitionConstraintDegree::with_cycles(2, vec![256]));
        }

        let context = AirContext::new(trace_info, degrees, 8, options);
        SpendCircuit {
            context,
            pub_inputs,
        }
    }

    fn context(&self) -> &AirContext<Felt> {
        &self.context
    }

    fn evaluate_transition<E: FieldElement<BaseField = Felt>>(
        &self,
        frame: &EvaluationFrame<E>,
        periodic_values: &[E],
        result: &mut [E],
    ) {
        let current = frame.current();
        let next = frame.next();

        // Periodic column layout:
        //   [0]    : sel_poseidon   period 8,   1 at rounds 0-6, 0 at free row
        //   [1..9] : rc[0..7]       period 8,   round constants per column
        //   [9]    : sel_row0       period 256, 1 only at step 0
        //   [10]   : sel_row8       period 256, 1 only at step 8
        //   [11]   : sel_chain      period 256, 1 at steps 15,23,31,…,159
        let sel = periodic_values[0];
        let rc: [E; 8] = core::array::from_fn(|k| periodic_values[1 + k]);
        let sel_row0 = periodic_values[9];
        let sel_row8 = periodic_values[10];
        let sel_chain = periodic_values[11];

        // [0..8] Poseidon round constraints
        eval_poseidon_round(current, next, &rc, sel, result);

        // [8..12] cm-carry copy: constant throughout trace
        for k in 0..4 {
            result[8 + k] = next[8 + k] - current[8 + k];
        }

        // [12..16] row-0: cm_carry = nullifier-hash right-input (cols 4-7)
        for k in 0..4 {
            result[12 + k] = sel_row0 * (current[8 + k] - current[4 + k]);
        }

        // [16..20] row-8 cm check: cm_carry appears in left (cols 0-3) OR right (cols 4-7).
        // Product: sel_row8 * (cm_carry - left) * (cm_carry - right) = 0.
        // This is zero when cm_carry matches either input position.
        for k in 0..4 {
            let diff_left = current[8 + k] - current[k];
            let diff_right = current[8 + k] - current[4 + k];
            result[16 + k] = sel_row8 * diff_left * diff_right;
        }

        // [20..24] hash-chaining: the running node (current cols 0-3 = hash output)
        // must appear in either cols 0-3 (left child) OR cols 4-7 (right child) of the
        // next hash input row.
        // Product: sel_chain * (next[k] - current[k]) * (next[4+k] - current[k]) = 0.
        for k in 0..4 {
            let diff_left = next[k] - current[k];
            let diff_right = next[4 + k] - current[k];
            result[20 + k] = sel_chain * diff_left * diff_right;
        }
    }

    fn get_assertions(&self) -> Vec<Assertion<Felt>> {
        let mut assertions = Vec::with_capacity(8);
        // Nullifier at row 7 (output of hash 0)
        for k in 0..4 {
            assertions.push(Assertion::single(k, 7, self.pub_inputs.nullifier[k]));
        }
        // Anchor at row 167 (output of hash 20 = Merkle level 19)
        for k in 0..4 {
            assertions.push(Assertion::single(k, 167, self.pub_inputs.anchor[k]));
        }
        assertions
    }

    fn get_periodic_column_values(&self) -> Vec<Vec<Felt>> {
        let rc = get_round_constants();
        let mut columns = Vec::with_capacity(12);

        // sel_poseidon: period 8
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

        // Round constants: 8 columns, period 8
        for col in 0..8 {
            let mut v: Vec<Felt> = rc.iter().map(|round| round[col]).collect();
            v.push(Felt::ZERO);
            columns.push(v);
        }

        // sel_row0: period 256 — fires at step 0
        let mut sel_row0 = vec![Felt::ZERO; 256];
        sel_row0[0] = Felt::ONE;
        columns.push(sel_row0);

        // sel_row8: period 256 — fires at step 8
        let mut sel_row8 = vec![Felt::ZERO; 256];
        sel_row8[8] = Felt::ONE;
        columns.push(sel_row8);

        // sel_chain: period 256 — fires at steps 15, 23, …, 159 (inter-hash injection rows)
        let mut sel_chain = vec![Felt::ZERO; 256];
        for k in 1..20usize {
            sel_chain[8 * k + 7] = Felt::ONE;
        }
        columns.push(sel_chain);

        columns
    }
}
