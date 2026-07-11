use winterfell::math::fields::f64::BaseElement as Felt;
use winterfell::math::FieldElement;

// Goldilocks prime p = 2^64 - 2^32 + 1.
// All inputs to Poseidon must be in [0, P).
const GOLDILOCKS_P: u64 = 0xFFFF_FFFF_0000_0001;

// Goldilocks Poseidon-7: t=8 state, alpha=7 S-box, 7 full rounds.
// MDS is a Cauchy matrix for branch number t+1 = 9 (see `get_mds_matrix`).
// Round constants derived from BLAKE3("rupeecoin-poseidon-rc-r{r}-c{c}").

// ── Goldilocks arithmetic helpers ─────────────────────────────────────────────

#[inline]
fn goldilocks_mul(a: u64, b: u64) -> u64 {
    debug_assert!(a < GOLDILOCKS_P);
    debug_assert!(b < GOLDILOCKS_P);
    ((a as u128 * b as u128) % GOLDILOCKS_P as u128) as u64
}

#[inline]
fn goldilocks_add(a: u64, b: u64) -> u64 {
    debug_assert!(a < GOLDILOCKS_P);
    debug_assert!(b < GOLDILOCKS_P);
    // a,b ∈ [0,P); sum ∈ [0, 2P).
    // If overflow: true sum = wrapped + 2^64; 2^64 ≡ (2^32-1) mod P.
    let (sum, overflow) = a.overflowing_add(b);
    if overflow {
        // sum + (2^32 - 1) < P since wrapped < 2P - 2^64 < 2^64 - 2^33 + 2
        sum.wrapping_add(0xFFFF_FFFFu64)
    } else if sum >= GOLDILOCKS_P {
        sum - GOLDILOCKS_P
    } else {
        sum
    }
}

// Fermat's little theorem: a^(p-2) mod p.
fn goldilocks_inv(a: u64) -> u64 {
    debug_assert!(
        a != 0 && a < GOLDILOCKS_P,
        "goldilocks_inv: input must be in (0, P)"
    );
    goldilocks_pow(a, GOLDILOCKS_P - 2)
}

fn goldilocks_pow(mut base: u64, mut exp: u64) -> u64 {
    let mut result = 1u64;
    base %= GOLDILOCKS_P;
    while exp > 0 {
        if exp & 1 == 1 {
            result = goldilocks_mul(result, base);
        }
        base = goldilocks_mul(base, base);
        exp >>= 1;
    }
    result
}

// ── MDS matrix (Cauchy, branch number 9) ─────────────────────────────────────
//
// M[i][j] = inv(x_i - y_j) mod P where x = [0..7], y = [8..15].
// Since x_i < y_j always, the denominator is (x_i - y_j) mod P = P - (y_j - x_i).
// Every square submatrix of a Cauchy matrix is non-singular (Cauchy determinant
// formula), so this matrix achieves branch number t+1 = 9.
pub fn get_mds_matrix() -> &'static [[u64; 8]; 8] {
    use std::sync::OnceLock;
    static MDS: OnceLock<[[u64; 8]; 8]> = OnceLock::new();
    MDS.get_or_init(|| {
        let mut m = [[0u64; 8]; 8];
        for i in 0..8usize {
            for j in 0..8usize {
                // denom = (i - (j+8)) mod P = P - (j+8-i)
                let denom = GOLDILOCKS_P - (j + 8 - i) as u64;
                m[i][j] = goldilocks_inv(denom);
            }
        }
        m
    })
}

// ── Round constants ────────────────────────────────────────────────────────────

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
                // Reduce into [0, P): one subtraction suffices since raw < 2^64 < 2P.
                let val = if raw >= GOLDILOCKS_P {
                    raw - GOLDILOCKS_P
                } else {
                    raw
                };
                rc[r][c] = Felt::new(val);
            }
        }
        rc
    })
}

// ── Poseidon permutation ───────────────────────────────────────────────────────

// Apply one Poseidon round in-place to the first 8 elements of `state`.
// Accepts a slice so callers with 12-column traces can pass &mut state[..8].
pub fn poseidon_round(state: &mut [Felt], round: usize) {
    let rc = get_round_constants();
    let mds = get_mds_matrix();

    // S-box: x^7
    for s in &mut state[..8] {
        *s = s.exp7();
    }

    // MDS matrix multiply (Cauchy matrix, branch number 9).
    // Accumulate in Goldilocks u64 arithmetic to avoid Felt overhead.
    let input: [u64; 8] = core::array::from_fn(|k| state[k].as_int());
    for i in 0..8 {
        let mut acc = 0u64;
        for j in 0..8 {
            let prod = goldilocks_mul(mds[i][j], input[j]);
            acc = goldilocks_add(acc, prod);
        }
        state[i] = Felt::new(acc);
    }

    // Add round constants
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

#[inline]
fn bytes_to_felt4(b: &[u8; 32]) -> [Felt; 4] {
    core::array::from_fn(|i| {
        let v = u64::from_le_bytes(b[i * 8..(i + 1) * 8].try_into().unwrap());
        // Inputs must be in [0, P); reduce by one subtraction if needed.
        Felt::new(if v >= GOLDILOCKS_P {
            v - GOLDILOCKS_P
        } else {
            v
        })
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
    let mds = get_mds_matrix();

    // S-box x^7 via multiplications (no exp7() on generic E)
    let sbox: [E; 8] = core::array::from_fn(|k| {
        let x = current[k];
        let x2 = x * x;
        let x4 = x2 * x2;
        x4 * x2 * x
    });

    // MDS matrix multiply using the same Cauchy matrix as poseidon_round.
    // Constraint: sel * (next[i] - (sum_j(M[i][j] * sbox[j]) + rc[i])) = 0
    for i in 0..8 {
        let mut mds_out = E::ZERO;
        for j in 0..8 {
            mds_out = mds_out + E::from(Felt::new(mds[i][j])) * sbox[j];
        }
        result[i] = sel * (next[i] - (mds_out + rc[i]));
    }
}
