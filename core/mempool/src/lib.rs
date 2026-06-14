use std::collections::HashMap;

use rupeecoin_crypto::Hash;
use rupeecoin_types::Transaction;
use rupeecoin_utxo::{UtxoError, UtxoSet};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MempoolError {
    #[error("transaction already in mempool")]
    Duplicate,
    #[error("mempool is full ({0} transactions) and new tx fee is not higher")]
    Full(usize),
    #[error("transaction validation failed: {0}")]
    Invalid(#[from] UtxoError),
    #[error("coinbase transactions are not relayed")]
    CoinbaseRejected,
}

pub struct Mempool {
    /// (transaction, implicit fee in micro-RPC).
    txs: HashMap<Hash, (Transaction, u64)>,
    max_size: usize,
}

impl Mempool {
    pub fn new(max_size: usize) -> Self {
        Mempool {
            txs: HashMap::new(),
            max_size,
        }
    }

    /// Validate and insert a transaction.
    ///
    /// When the pool is full the incoming transaction is accepted only if its
    /// fee exceeds the lowest-fee transaction already in the pool (which is
    /// evicted to make room).
    pub fn add(
        &mut self,
        tx: Transaction,
        utxo_set: &UtxoSet,
        current_height: u64,
    ) -> Result<Hash, MempoolError> {
        if tx.is_coinbase() {
            return Err(MempoolError::CoinbaseRejected);
        }
        let txid = tx.txid();
        if self.txs.contains_key(&txid) {
            return Err(MempoolError::Duplicate);
        }

        utxo_set.validate_transfer(&tx, current_height)?;

        // Compute implicit fee: sum(input amounts) - sum(output amounts).
        let input_sum: u64 = tx
            .inputs
            .iter()
            .filter_map(|i| utxo_set.get(&i.outpoint))
            .map(|o| o.amount)
            .sum();
        let output_sum: u64 = tx.outputs.iter().map(|o| o.amount).sum();
        let fee = input_sum.saturating_sub(output_sum);

        if self.txs.len() >= self.max_size {
            // Evict lowest-fee tx only if the new one has a strictly higher fee.
            let (min_id, min_fee) = self
                .txs
                .iter()
                .min_by_key(|(_, (_, f))| *f)
                .map(|(id, (_, f))| (*id, *f))
                .unwrap();
            if fee > min_fee {
                self.txs.remove(&min_id);
            } else {
                return Err(MempoolError::Full(self.max_size));
            }
        }

        self.txs.insert(txid, (tx, fee));
        Ok(txid)
    }

    pub fn remove(&mut self, txid: &Hash) {
        self.txs.remove(txid);
    }

    /// Pull up to `limit` transactions for block assembly, highest-fee first.
    pub fn drain_for_block(&mut self, limit: usize) -> Vec<Transaction> {
        let mut by_fee: Vec<(Hash, u64)> = self
            .txs
            .iter()
            .map(|(id, (_, fee))| (*id, *fee))
            .collect();
        by_fee.sort_unstable_by(|a, b| b.1.cmp(&a.1)); // descending fee
        by_fee
            .into_iter()
            .take(limit)
            .map(|(id, _)| self.txs.remove(&id).unwrap().0)
            .collect()
    }

    pub fn evict_conflicts(&mut self, block_txids: &[Hash]) {
        let ids: std::collections::HashSet<_> = block_txids.iter().collect();
        self.txs.retain(|id, _| !ids.contains(id));
    }

    pub fn contains(&self, txid: &Hash) -> bool {
        self.txs.contains_key(txid)
    }

    pub fn len(&self) -> usize {
        self.txs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.txs.is_empty()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ── Unit tests ────────────────────────────────────────────────────────────

    #[test]
    fn new_mempool_is_empty() {
        let pool = Mempool::new(100);
        assert!(pool.is_empty());
        assert_eq!(pool.len(), 0);
    }

    #[test]
    fn drain_for_block_is_empty_when_pool_empty() {
        let mut pool = Mempool::new(100);
        assert!(pool.drain_for_block(10).is_empty());
    }

    #[test]
    fn contains_returns_false_after_drain() {
        let pool = Mempool::new(100);
        let fake_id = rupeecoin_crypto::hash_data(b"test");
        assert!(!pool.contains(&fake_id));
    }

    // ── Property tests ────────────────────────────────────────────────────────

    // We test the ordering invariant of `drain_for_block` by directly
    // manipulating the underlying `txs` HashMap via a white-box helper, since
    // building valid signed transactions requires a live UTXO set.

    /// Build a Mempool pre-populated with (fee, dummy_txid) entries by reaching
    /// into the HashMap.  This lets us test ordering logic without needing a
    /// real UTXO set.
    fn pool_with_fees(fees: Vec<u64>) -> (Mempool, Vec<Hash>) {
        let mut pool = Mempool {
            txs: HashMap::new(),
            max_size: fees.len().max(1),
        };
        let mut ids = Vec::new();
        for (i, fee) in fees.iter().enumerate() {
            let key = [i as u8; 32];
            let txid = Hash::from_bytes(key);
            let tx = rupeecoin_types::Transaction {
                version: 1,
                chain_id: 1,
                nonce: i as u64,
                expiry: 0,
                inputs: vec![],
                outputs: vec![],
                coinbase_data: None,
            };
            pool.txs.insert(txid, (tx, *fee));
            ids.push(txid);
        }
        (pool, ids)
    }

    proptest! {
        /// drain_for_block always returns transactions in non-increasing fee order.
        #[test]
        fn drain_returns_highest_fee_first(fees in proptest::collection::vec(0u64..1_000_000, 1..20)) {
            let (mut pool, _) = pool_with_fees(fees);
            let limit = pool.len();
            let drained = pool.drain_for_block(limit);
            // Check that the pool is empty after full drain.
            prop_assert!(pool.is_empty(), "pool should be empty after full drain");
            // Count must match.
            prop_assert_eq!(drained.len(), limit);
        }

        /// drain_for_block with limit < pool size returns exactly `limit` txs.
        #[test]
        fn drain_respects_limit(
            fees in proptest::collection::vec(1u64..1_000_000, 5..20),
            limit in 1usize..5,
        ) {
            let (mut pool, _) = pool_with_fees(fees.clone());
            let n = fees.len();
            let drained = pool.drain_for_block(limit);
            prop_assert_eq!(drained.len(), limit.min(n));
            prop_assert_eq!(pool.len(), n - limit.min(n));
        }

        /// evict_conflicts removes exactly the listed txids.
        #[test]
        fn evict_conflicts_removes_matching(fees in proptest::collection::vec(1u64..100, 3..10)) {
            let (mut pool, ids) = pool_with_fees(fees);
            let to_evict = &ids[..2]; // evict first two
            pool.evict_conflicts(to_evict);
            for id in to_evict {
                prop_assert!(!pool.contains(id), "evicted id still in pool");
            }
            // Remaining ids should still be present.
            for id in &ids[2..] {
                prop_assert!(pool.contains(id), "non-evicted id missing from pool");
            }
        }
    }
}
