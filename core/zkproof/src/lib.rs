pub mod batch;
pub mod circuit;
pub mod hash;
pub mod prover;
pub mod types;

pub use batch::{BatchedSpendPublicInputs, BatchedSpendCircuit, BatchSpendProver};
pub use circuit::{SpendCircuit, SpendPublicInputs};
pub use hash::compress_bytes as poseidon_compress_bytes;
pub use prover::{
    build_trace, compute_merkle_root, compute_nullifier, default_proof_options, SpendProver,
    SpendWitness,
};
pub use types::{
    prove_spend, prove_spend_batch, verify_spend, verify_spend_batch, verify_spend_raw,
    BatchedStarkSpendProof, BatchSpendInput, StarkSpendProof, ZkError,
};
