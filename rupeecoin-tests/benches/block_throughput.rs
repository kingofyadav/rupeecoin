//! Benchmark: block construction and validation throughput.
//!
//! Measures how many blocks per second the chain can produce and accept
//! under a coinbase-only (empty mempool) workload.  This is the lower bound
//! on throughput; add-tx numbers scale down from here as mempool processing
//! and UTXO lookups add per-tx cost.
//!
//! Run with:  cargo bench -p rupeecoin-tests --bench block_throughput

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use rupeecoin_blockchain::{build_block, genesis_block, Chain};
use rupeecoin_crypto::KeyPair;
use rupeecoin_staking::ValidatorRegistry;
use rupeecoin_types::MICRO_RPC_PER_COIN;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn fixed_keypair() -> KeyPair {
    KeyPair::from_seed(&[0x42u8; 32])
}

fn validator_registry(kp: &KeyPair) -> ValidatorRegistry {
    let mut reg = ValidatorRegistry::new();
    reg.register(kp.public_key.clone(), 10_000 * MICRO_RPC_PER_COIN, 0)
        .expect("registration");
    reg
}

fn build_chain(kp: &KeyPair, n_blocks: u64) -> (Chain, ValidatorRegistry) {
    let mut registry = validator_registry(kp);
    let mut chain = Chain::new(genesis_block());
    for _ in 0..n_blocks {
        let block = build_block(&chain, kp, &registry, vec![], vec![], vec![]);
        chain.add_block(block, &mut registry).expect("add_block");
    }
    (chain, registry)
}

// ── Benchmarks ────────────────────────────────────────────────────────────────

fn bench_build_block(c: &mut Criterion) {
    let kp = fixed_keypair();
    let (chain, registry) = build_chain(&kp, 0);

    c.bench_function("build_block/single", |b| {
        b.iter(|| black_box(build_block(&chain, &kp, &registry, vec![], vec![], vec![])))
    });
}

fn bench_add_block(c: &mut Criterion) {
    let kp = fixed_keypair();

    let mut group = c.benchmark_group("add_blocks");

    for &n in &[10u64, 50, 200] {
        group.throughput(Throughput::Elements(n));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || {
                    // Setup (not timed): fresh chain + registry.
                    let registry = validator_registry(&kp);
                    let chain = Chain::new(genesis_block());
                    (chain, registry)
                },
                |(mut chain, mut registry)| {
                    for _ in 0..n {
                        let block = build_block(&chain, &kp, &registry, vec![], vec![], vec![]);
                        chain.add_block(block, &mut registry).expect("add_block");
                    }
                    black_box(chain.height())
                },
                criterion::BatchSize::SmallInput,
            )
        });
    }

    group.finish();
}

criterion_group!(benches, bench_build_block, bench_add_block);
criterion_main!(benches);
