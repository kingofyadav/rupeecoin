//! Benchmark: chain-sync throughput (blocks applied per second).
//!
//! Simulates a node catching up to the tip by replaying a pre-built chain onto
//! a fresh genesis.  This is purely CPU-bound (no I/O, no P2P): it measures
//! how fast `Chain::add_block` can validate and apply a sequence of blocks.
//!
//! A real network sync will be slower due to P2P latency and RocksDB writes;
//! this gives the theoretical maximum blocks/second assuming data is local.
//!
//! Run with:  cargo bench -p rupeecoin-tests --bench chain_sync

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use rupeecoin_blockchain::{build_block, genesis_block, Chain};
use rupeecoin_crypto::KeyPair;
use rupeecoin_staking::ValidatorRegistry;
use rupeecoin_types::{Block, MICRO_RPC_PER_COIN};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn fixed_keypair() -> KeyPair {
    KeyPair::from_seed(&[0x37u8; 32])
}

fn fresh_registry(kp: &KeyPair) -> ValidatorRegistry {
    let mut reg = ValidatorRegistry::new();
    reg.register(kp.public_key.clone(), 10_000 * MICRO_RPC_PER_COIN, 0)
        .expect("registration");
    reg
}

/// Pre-build a canonical chain of `n` blocks to replay in each iteration.
///
/// Returns the list of non-genesis blocks in order.  Building is done once
/// outside the timing loop, so only the application is measured.
fn prebuilt_blocks(kp: &KeyPair, n: u64) -> Vec<Block> {
    let mut registry = fresh_registry(kp);
    let mut chain = Chain::new(genesis_block());
    let mut blocks = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let block = build_block(&chain, kp, &registry, vec![], vec![], vec![]);
        chain
            .add_block(block.clone(), &mut registry)
            .expect("prebuilt add_block");
        blocks.push(block);
    }
    blocks
}

// ── Benchmarks ────────────────────────────────────────────────────────────────

fn bench_chain_sync(c: &mut Criterion) {
    let kp = fixed_keypair();
    let mut group = c.benchmark_group("chain_sync");

    for &n in &[50u64, 200, 1_000] {
        let blocks = prebuilt_blocks(&kp, n);

        group.throughput(Throughput::Elements(n));
        group.bench_with_input(BenchmarkId::new("blocks", n), &blocks, |b, blocks| {
            b.iter_batched(
                || {
                    // Setup (not timed): fresh target chain + registry.
                    let chain = Chain::new(genesis_block());
                    let registry = fresh_registry(&kp);
                    (chain, registry)
                },
                |(mut chain, mut registry)| {
                    for block in blocks {
                        chain
                            .add_block(block.clone(), &mut registry)
                            .expect("sync add_block");
                    }
                    black_box(chain.height())
                },
                criterion::BatchSize::SmallInput,
            )
        });
    }

    group.finish();
}

criterion_group!(benches, bench_chain_sync);
criterion_main!(benches);
