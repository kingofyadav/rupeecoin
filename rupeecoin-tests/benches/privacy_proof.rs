//! Benchmark: STARK spend proof generation and verification latency.
//!
//! These are the two most latency-sensitive operations in the privacy layer:
//!
//!   • `prove_spend`  — run by the spending wallet (mobile target: ≤3s)
//!   • `verify_spend` — run by every validating node (target: ≤50ms)
//!
//! `prove_spend` samples are intentionally small (10) because each call takes
//! several seconds.  `verify_spend` can use a larger sample (30).
//!
//! Run with:  cargo bench -p rupeecoin-tests --bench privacy_proof

use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, Criterion};

use rupeecoin_zkproof::{
    prove_spend, prove_spend_batch, verify_spend, verify_spend_batch, BatchSpendInput,
};

// Fixed witness values (same as the existing round-trip unit test).
const NK: [u64; 4] = [1, 2, 3, 4];
const CM: [u64; 4] = [5, 6, 7, 8];
const SIBLINGS: [[u64; 4]; 20] = [[9, 10, 11, 12]; 20];
const IS_RIGHT: [bool; 20] = [false; 20];

fn bench_prove(c: &mut Criterion) {
    let mut group = c.benchmark_group("privacy_proof");
    // Proof generation is slow; use small sample + generous wall-clock budget.
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(90));

    group.bench_function("prove_spend", |b| {
        b.iter(|| black_box(prove_spend(NK, CM, SIBLINGS, IS_RIGHT).expect("prove_spend")))
    });

    group.finish();
}

fn bench_verify(c: &mut Criterion) {
    // Pre-generate one proof to reuse across verification samples.
    let (proof, nullifier) = prove_spend(NK, CM, SIBLINGS, IS_RIGHT).expect("setup prove_spend");

    let mut group = c.benchmark_group("privacy_proof");
    group.sample_size(30);
    group.measurement_time(Duration::from_secs(30));

    group.bench_function("verify_spend", |b| {
        b.iter(|| {
            verify_spend(&proof, nullifier).expect("verify_spend");
            black_box(())
        })
    });

    group.finish();
}

fn make_batch_input(nk: [u64; 4], cm: [u64; 4]) -> BatchSpendInput {
    BatchSpendInput {
        nullifier_key: nk,
        note_commitment: cm,
        siblings: SIBLINGS,
        is_right: IS_RIGHT,
    }
}

fn bench_batch_prove(c: &mut Criterion) {
    let mut group = c.benchmark_group("privacy_proof");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(120));

    // 2-input batch (512-row trace)
    let inputs2 = [
        make_batch_input(NK, CM),
        make_batch_input([10, 20, 30, 40], [50, 60, 70, 80]),
    ];
    group.bench_function("prove_spend_batch_n2", |b| {
        b.iter(|| black_box(prove_spend_batch(&inputs2).expect("batch prove n=2")))
    });

    group.finish();
}

fn bench_batch_verify(c: &mut Criterion) {
    let inputs2 = [
        make_batch_input(NK, CM),
        make_batch_input([10, 20, 30, 40], [50, 60, 70, 80]),
    ];
    let (proof2, _) = prove_spend_batch(&inputs2).expect("setup batch proof n=2");

    let mut group = c.benchmark_group("privacy_proof");
    group.sample_size(30);
    group.measurement_time(Duration::from_secs(30));

    group.bench_function("verify_spend_batch_n2", |b| {
        b.iter(|| {
            verify_spend_batch(&proof2).expect("batch verify n=2");
            black_box(())
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_prove,
    bench_verify,
    bench_batch_prove,
    bench_batch_verify
);
criterion_main!(benches);
