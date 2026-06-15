pub mod circuit;
pub mod hash;
pub mod prover;
pub mod types;

pub use circuit::{SpendCircuit, SpendPublicInputs};
pub use hash::compress_bytes as poseidon_compress_bytes;
pub use prover::{
    build_trace, compute_merkle_root, compute_nullifier, default_proof_options, SpendProver,
    SpendWitness,
};
pub use types::{prove_spend, verify_spend, verify_spend_raw, StarkSpendProof, ZkError};
