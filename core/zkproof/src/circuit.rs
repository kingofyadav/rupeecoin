use winterfell::{
    math::{fields::f64::BaseElement as Felt, FieldElement, ToElements},
    Air, AirContext, Assertion, EvaluationFrame, ProofOptions, TraceInfo,
    TransitionConstraintDegree,
};

use crate::hash::{eval_poseidon_round, get_round_constants};

// Public inputs: nullifier (cols 0-3 at row 7) + anchor (cols 0-3 at row 167).
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
//   rows  8-15:  hash 1 — Merkle level 0: Poseidon(cm | sib0)
//   rows 16-23:  hash 2 — Merkle level 1: Poseidon(parent0 | sib1)
//   ...
//   rows 160-167: hash 20 — Merkle level 19: Poseidon(parent18 | sib19)
//   rows 168-255: padding (zeros)
//
// The cm-carry copy constraint (cols 8-11 constant) plus a row-0 equality
// (cm_carry = cm_nk at row 0) and a row-8 equality (cm_carry = cm_merkle
// at row 8) together prove that the SAME private cm is used in both the
// nullifier hash and as the Merkle leaf — without revealing cm.
pub struct SpendCircuit {
    context: AirContext<Felt>,
    pub_inputs: SpendPublicInputs,
}

impl Air for SpendCircuit {
    type BaseField = Felt;
    type PublicInputs = SpendPublicInputs;

    fn new(trace_info: TraceInfo, pub_inputs: SpendPublicInputs, options: ProofOptions) -> Self {
        let mut degrees = Vec::with_capacity(24);

        for _ in 0..8 {
            degrees.push(TransitionConstraintDegree::with_cycles(7, vec![8]));
        }

        for _ in 0..4 {
            degrees.push(TransitionConstraintDegree::new(1));
        }

        // row-0 + row-8 copy-constraint selectors (period 256)
        for _ in 0..8 {
            degrees.push(TransitionConstraintDegree::with_cycles(1, vec![256]));
        }

        // hash-chaining constraints: enforce output[k] of hash k carries into
        // cols 0-3 of hash k+1 input row (fires at steps 15, 23, …, 159)
        for _ in 0..4 {
            degrees.push(TransitionConstraintDegree::with_cycles(1, vec![256]));
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

        // [8..12] cm-carry copy: next[8+k] = current[8+k] at every step
        for k in 0..4 {
            result[8 + k] = next[8 + k] - current[8 + k];
        }

        // [12..16] row-0 eq: cm_carry = nullifier-hash right-input (cols 4-7)
        for k in 0..4 {
            result[12 + k] = sel_row0 * (current[8 + k] - current[4 + k]);
        }

        // [16..20] row-8 eq: cm_carry = Merkle-leaf left-input (cols 0-3)
        for k in 0..4 {
            result[16 + k] = sel_row8 * (current[8 + k] - current[k]);
        }

        // [20..24] hash-chaining: at inter-hash free rows (steps 15,23,…,159)
        // enforce next[k] = current[k] so hash-k output feeds hash-(k+1) input.
        for k in 0..4 {
            result[20 + k] = sel_chain * (next[k] - current[k]);
        }
    }

    fn get_assertions(&self) -> Vec<Assertion<Felt>> {
        let mut assertions = Vec::with_capacity(8);

        for k in 0..4 {
            assertions.push(Assertion::single(k, 7, self.pub_inputs.nullifier[k]));
        }

        for k in 0..4 {
            assertions.push(Assertion::single(k, 167, self.pub_inputs.anchor[k]));
        }

        assertions
    }

    fn get_periodic_column_values(&self) -> Vec<Vec<Felt>> {
        let rc = get_round_constants();

        let mut columns = Vec::with_capacity(11);

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
            let mut v: Vec<Felt> = (0..7).map(|r| rc[r][col]).collect();
            v.push(Felt::ZERO);
            columns.push(v);
        }

        let mut sel_row0 = vec![Felt::ZERO; 256];
        sel_row0[0] = Felt::ONE;
        columns.push(sel_row0);

        let mut sel_row8 = vec![Felt::ZERO; 256];
        sel_row8[8] = Felt::ONE;
        columns.push(sel_row8);

        // Column 11: sel_chain (period 256) — 1 at each inter-hash free row
        // steps 15, 23, 31, …, 159  (= 8k+7 for k=1..19)
        let mut sel_chain = vec![Felt::ZERO; 256];
        for k in 1..20usize {
            sel_chain[8 * k + 7] = Felt::ONE;
        }
        columns.push(sel_chain);

        columns
    }
}
