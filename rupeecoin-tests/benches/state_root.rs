//! Benchmark: UTXO set state-root (Merkle root) computation at scale.
//!
//! `state_root()` sorts all UTXOs, hashes each leaf, then reduces the tree.
//! This is O(N log N) in UTXO count.  The benchmark sweeps from 100 to 100 000
//! UTXOs to give a clear scaling curve; 1 M UTXOs can be extrapolated from the
//! slope (typically ~2× per decade, consistent with O(N log N)).
//!
//! Run with:  cargo bench -p rupeecoin-tests --bench state_root

use criterion::{
    black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput,
};

use rupeecoin_crypto::Address;
use rupeecoin_types::{Transaction, TxOutput, MICRO_RPC_PER_COIN};
use rupeecoin_utxo::UtxoSet;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build a UtxoSet pre-populated with `n` synthetic UTXOs.
///
/// Each UTXO comes from a synthetic coinbase transaction whose `coinbase_data`
/// encodes the index, ensuring unique txids and therefore unique OutPoints.
fn make_utxo_set(n: usize) -> UtxoSet {
    let recipient = Address([0xABu8; 20]);
    let chunk = 256; // outputs per tx to keep tx count manageable

    let txs: Vec<Transaction> = (0..n.div_ceil(chunk))
        .map(|tx_idx| {
            let outputs_in_this_tx = if (tx_idx + 1) * chunk <= n {
                chunk
            } else {
                n % chunk
            };
            Transaction {
                version: 1,
                chain_id: 2,
                nonce: 0,
                expiry: 0,
                inputs: vec![],
                outputs: (0..outputs_in_this_tx)
                    .map(|_| TxOutput {
                        amount: MICRO_RPC_PER_COIN,
                        recipient,
                    })
                    .collect(),
                // Unique coinbase_data per tx → unique txid → unique OutPoints.
                coinbase_data: Some((tx_idx as u64).to_le_bytes().to_vec()),
            }
        })
        .collect();

    let mut set = UtxoSet::new();
    set.apply_transactions(&txs);
    assert_eq!(set.len(), n, "UTXO count mismatch");
    set
}

// ── Benchmarks ────────────────────────────────────────────────────────────────

fn bench_state_root(c: &mut Criterion) {
    let mut group = c.benchmark_group("state_root");

    for &n in &[100usize, 1_000, 10_000, 100_000] {
        let utxo_set = make_utxo_set(n);

        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(
            BenchmarkId::new("utxos", n),
            &utxo_set,
            |b, set| b.iter(|| black_box(set.state_root().expect("UTXO serialization failed"))),
        );
    }

    group.finish();
}

criterion_group!(benches, bench_state_root);
criterion_main!(benches);
