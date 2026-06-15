use winterfell::math::fields::f64::BaseElement as Felt;
use winterfell::math::FieldElement;

// Goldilocks Poseidon-7: t=8 state, alpha=7 S-box, 7 full rounds,
// MDS circulant first-row=[2,1,1,1,1,1,1,1] → out[k] = in[k] + sum(in).

// Round constants derived from BLAKE3("rupeecoin-poseidon-rc-r{r}-c{c}").
pub fn get_round_constants() -> &'static [[Felt; 8]; 7] {
    use std::sync::OnceLock;
    static RC: OnceLock<[[Felt; 8]; 7]> = OnceLock::new();
    RC.get_or_init(|| {
        let mut rc = [[Felt::ZERO; 8]; 7];
        for r in 0..7usize {
            for c in 0..8usize {
                let msg = format!("rupeecoin-poseidon-rc-r{r}-c{c}");
                let hash = blake3::hash(msg.as_bytes());
                let bytes = hash.as_bytes();
                let raw = u64::from_le_bytes(bytes[..8].try_into().unwrap());
                // Reduce into Goldilocks: wrap at p = 2^64 - 2^32 + 1.
                // Using checked subtraction so large values are properly reduced.
                const P: u64 = 0xFFFF_FFFF_0000_0001;
                let val = if raw >= P { raw - P } else { raw };
                rc[r][c] = Felt::new(val);
            }
        }
        rc
    })
}

// Apply one Poseidon round in-place to the first 8 elements of `state`.
// Accepts a slice so callers with 12-column traces can pass &mut state[..8].
pub fn poseidon_round(state: &mut [Felt], round: usize) {
    let rc = get_round_constants();
    for s in &mut state[..8] {
        *s = s.exp7();
    }
    let total = state[..8].iter().copied().fold(Felt::ZERO, |a, b| a + b);
    for s in &mut state[..8] {
        *s = *s + total;
    }
    for (s, &r) in state[..8].iter_mut().zip(rc[round].iter()) {
        *s = *s + r;
    }
}

// Full 7-round Poseidon permutation over an 8-element state.
pub fn poseidon7(state: &mut [Felt; 8]) {
    for r in 0..7 {
        poseidon_round(&mut state[..], r);
    }
}

// Compress two 4-element halves into a 4-element output using Poseidon-7.
pub fn compress(left: [Felt; 4], right: [Felt; 4]) -> [Felt; 4] {
    let mut state = [
        left[0], left[1], left[2], left[3], right[0], right[1], right[2], right[3],
    ];
    poseidon7(&mut state);
    [state[0], state[1], state[2], state[3]]
}

const GOLDILOCKS_P: u64 = 0xFFFF_FFFF_0000_0001;

#[inline]
fn bytes_to_felt4(b: &[u8; 32]) -> [Felt; 4] {
    core::array::from_fn(|i| {
        let v = u64::from_le_bytes(b[i * 8..(i + 1) * 8].try_into().unwrap());
        Felt::new(if v >= GOLDILOCKS_P { v - GOLDILOCKS_P } else { v })
    })
}

#[inline]
fn felt4_to_bytes(f: [Felt; 4]) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, felt) in f.iter().enumerate() {
        out[i * 8..(i + 1) * 8].copy_from_slice(&felt.as_int().to_le_bytes());
    }
    out
}

/// Poseidon compression over 32-byte inputs — the same operation used inside the
/// STARK circuit.  Both the commitment tree and the circuit use this function,
/// so the on-chain tree root and the STARK anchor are the same value.
pub fn compress_bytes(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    felt4_to_bytes(compress(bytes_to_felt4(left), bytes_to_felt4(right)))
}

// Evaluate one Poseidon round transition constraint for generic field element E.
// Inputs: current 8-element state slice, next 8-element state slice, round constants,
// selector (0 or 1), and output result slice (8 elements, must start at result[0]).
pub fn eval_poseidon_round<E: FieldElement<BaseField = Felt>>(
    current: &[E],
    next: &[E],
    rc: &[E; 8],
    sel: E,
    result: &mut [E],
) {
    // S-box x^7 via multiplications (no exp7() on generic E)
    let sbox: [E; 8] = core::array::from_fn(|k| {
        let x = current[k];
        let x2 = x * x;
        let x4 = x2 * x2;
        x4 * x2 * x
    });
    // MDS total: total = sum(sbox)
    let total = sbox.iter().copied().fold(E::ZERO, |a, b| a + b);
    // Constraint: sel * (next[k] - (sbox[k] + total + rc[k])) = 0
    for k in 0..8 {
        result[k] = sel * (next[k] - (sbox[k] + total + rc[k]));
    }
}
