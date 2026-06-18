use std::collections::HashMap;

use rupeecoin_crypto::{leaf_hash, node_hash, Address, Hash};
use rupeecoin_types::{Block, OutPoint, Transaction, TxOutput};
use thiserror::Error;

/// Coinbase outputs are locked for this many blocks after the block that
/// created them.  Spending before maturity is rejected.
pub const COINBASE_MATURITY: u64 = 100;

#[derive(Error, Debug)]
pub enum UtxoError {
    #[error("non-coinbase transaction must have at least one input")]
    NoInputs,
    #[error("input not found in UTXO set: {0:?}")]
    InputNotFound(OutPoint),
    #[error("public key does not own the referenced output")]
    InvalidOwnership,
    #[error("invalid signature on input {0}")]
    InvalidSignature(usize),
    #[error("output sum exceeds input sum")]
    InsufficientFunds,
    #[error("input value overflow: total inputs exceed u64::MAX")]
    InputOverflow,
    #[error("output value overflow: total outputs exceed u64::MAX")]
    OutputOverflow,
    #[error("all inputs in a transaction must share the same sender address")]
    MultiSenderNotAllowed,
    #[error("transaction expired at block {expiry} (current height {height})")]
    Expired { expiry: u64, height: u64 },
    #[error("invalid nonce: expected {expected}, got {got}")]
    InvalidNonce { expected: u64, got: u64 },
    #[error("coinbase output not yet mature: spendable at block {available_at}, current height {current_height}")]
    CoinbaseImmature { available_at: u64, current_height: u64 },
}

/// In-memory UTXO set with nonce tracking and state-root support.
#[derive(Clone)]
pub struct UtxoSet {
    utxos: HashMap<OutPoint, TxOutput>,
    /// Per-address sequential nonce.  Incremented each time an address spends a UTXO.
    pub nonces: HashMap<Address, u64>,
    /// Block height at which each coinbase output was created.
    /// Used to enforce COINBASE_MATURITY before the output can be spent.
    coinbase_heights: HashMap<OutPoint, u64>,
}

impl UtxoSet {
    pub fn new() -> Self {
        UtxoSet {
            utxos: HashMap::new(),
            nonces: HashMap::new(),
            coinbase_heights: HashMap::new(),
        }
    }

    pub fn get(&self, outpoint: &OutPoint) -> Option<&TxOutput> {
        self.utxos.get(outpoint)
    }

    /// Insert a UTXO directly — used by reorg rollback to restore spent outputs.
    pub fn insert(&mut self, outpoint: OutPoint, output: TxOutput) {
        self.utxos.insert(outpoint, output);
    }

    /// Remove a UTXO directly — used by reorg rollback to undo newly created outputs.
    pub fn remove(&mut self, outpoint: &OutPoint) {
        self.utxos.remove(outpoint);
        self.coinbase_heights.remove(outpoint);
    }

    /// Remove a coinbase maturity record without removing the UTXO itself.
    pub fn remove_coinbase_height(&mut self, outpoint: &OutPoint) {
        self.coinbase_heights.remove(outpoint);
    }

    /// Decrement a nonce — used by reorg rollback to undo a spend.
    pub fn decrement_nonce(&mut self, addr: &Address) {
        if let Some(n) = self.nonces.get_mut(addr) {
            if *n > 0 {
                *n -= 1;
            } else {
                tracing::warn!("attempt to decrement nonce already at zero for {:?}", addr);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.utxos.len()
    }

    pub fn is_empty(&self) -> bool {
        self.utxos.is_empty()
    }

    /// Validate a non-coinbase transaction against the current UTXO set.
    ///
    /// `current_height` is the height of the block the tx would be included in,
    /// used for expiry checking.
    pub fn validate_transfer(&self, tx: &Transaction, current_height: u64) -> Result<(), UtxoError> {
        if tx.inputs.is_empty() {
            return Err(UtxoError::NoInputs);
        }

        // Expiry check (0 means no expiry).
        if tx.expiry != 0 && current_height > tx.expiry {
            return Err(UtxoError::Expired {
                expiry: tx.expiry,
                height: current_height,
            });
        }

        // All inputs must share the same sender address so the nonce model is
        // unambiguous: one address → one nonce sequence.
        if let Some(first) = tx.inputs.first() {
            let sender = first.public_key.to_address();
            for input in &tx.inputs {
                if input.public_key.to_address() != sender {
                    return Err(UtxoError::MultiSenderNotAllowed);
                }
            }
            let expected = self.nonces.get(&sender).copied().unwrap_or(0);
            if tx.nonce != expected {
                return Err(UtxoError::InvalidNonce {
                    expected,
                    got: tx.nonce,
                });
            }
        }

        // Coinbase maturity: reject spends of coinbase outputs before they mature.
        for input in &tx.inputs {
            if let Some(&lock_height) = self.coinbase_heights.get(&input.outpoint) {
                let available_at = lock_height + COINBASE_MATURITY;
                if current_height < available_at {
                    return Err(UtxoError::CoinbaseImmature {
                        available_at,
                        current_height,
                    });
                }
            }
        }

        let sig_hash = tx.sig_hash();
        let mut input_sum: u64 = 0;

        for (i, input) in tx.inputs.iter().enumerate() {
            let utxo = self
                .utxos
                .get(&input.outpoint)
                .ok_or_else(|| UtxoError::InputNotFound(input.outpoint.clone()))?;

            if input.public_key.to_address() != utxo.recipient {
                return Err(UtxoError::InvalidOwnership);
            }

            input
                .public_key
                .verify(sig_hash.as_bytes(), &input.signature)
                .map_err(|_| UtxoError::InvalidSignature(i))?;

            input_sum = input_sum
                .checked_add(utxo.amount)
                .ok_or(UtxoError::InputOverflow)?;
        }

        let output_sum: u64 = tx
            .outputs
            .iter()
            .try_fold(0u64, |acc, o| acc.checked_add(o.amount))
            .ok_or(UtxoError::OutputOverflow)?;
        if output_sum > input_sum {
            return Err(UtxoError::InsufficientFunds);
        }

        Ok(())
    }

    /// Apply a validated block: remove spent outputs, add new outputs, increment nonces.
    pub fn apply_block(&mut self, block: &Block) -> Result<(), UtxoError> {
        let height = block.header.height;
        for (tx_idx, tx) in block.transactions.iter().enumerate() {
            let is_coinbase = tx_idx == 0 && tx.inputs.is_empty();
            for input in &tx.inputs {
                self.utxos
                    .remove(&input.outpoint)
                    .ok_or_else(|| UtxoError::InputNotFound(input.outpoint.clone()))?;
                // Remove maturity record when the output is spent.
                self.coinbase_heights.remove(&input.outpoint);
            }
            let txid = tx.txid();
            for (vout, output) in tx.outputs.iter().enumerate() {
                let op = OutPoint { txid, vout: vout as u32 };
                if is_coinbase {
                    self.coinbase_heights.insert(op.clone(), height);
                }
                self.utxos.insert(op, output.clone());
            }
            // Increment sender nonce for non-coinbase transactions.
            if let Some(first_input) = tx.inputs.first() {
                let sender = first_input.public_key.to_address();
                *self.nonces.entry(sender).or_insert(0) += 1;
            }
        }
        Ok(())
    }

    /// Apply a list of transactions to the UTXO set without nonce updates.
    /// Used to simulate state changes for state-root pre-computation.
    pub fn apply_transactions(&mut self, txs: &[Transaction]) {
        for tx in txs {
            for input in &tx.inputs {
                self.utxos.remove(&input.outpoint);
            }
            let txid = tx.txid();
            for (vout, output) in tx.outputs.iter().enumerate() {
                self.utxos.insert(
                    OutPoint {
                        txid,
                        vout: vout as u32,
                    },
                    output.clone(),
                );
            }
        }
    }

    /// Compute the Merkle root over the current UTXO set.
    ///
    /// Entries are sorted deterministically (by bincode-serialized OutPoint) so
    /// all nodes with identical UTXO sets produce the same root.
    pub fn state_root(&self) -> Result<Hash, bincode::Error> {
        if self.utxos.is_empty() {
            return Ok(Hash::zero());
        }

        // Serialize each entry once; sort by serialized key for determinism.
        let mut entries: Vec<(Vec<u8>, Vec<u8>)> = self
            .utxos
            .iter()
            .map(|(op, out)| Ok((bincode::serialize(op)?, bincode::serialize(out)?)))
            .collect::<Result<_, bincode::Error>>()?;
        entries.sort_by(|(a, _), (b, _)| a.cmp(b));

        let mut layer: Vec<Hash> = entries
            .iter()
            .map(|(op_bytes, out_bytes)| {
                let mut data = op_bytes.clone();
                data.extend_from_slice(out_bytes);
                leaf_hash(&data)
            })
            .collect();

        while layer.len() > 1 {
            let mut next = Vec::with_capacity((layer.len() + 1) / 2);
            for pair in layer.chunks(2) {
                let right = if pair.len() == 2 { &pair[1] } else { &pair[0] };
                next.push(node_hash(&pair[0], right));
            }
            layer = next;
        }
        Ok(layer[0])
    }
}

impl Default for UtxoSet {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rupeecoin_crypto::KeyPair;
    use rupeecoin_types::{Block, BlockHeader, Transaction, TxInput, TxOutput};

    fn coinbase_tx(height: u64, recipient: rupeecoin_crypto::Address) -> Transaction {
        Transaction {
            version: 1,
            chain_id: 1,
            nonce: 0,
            expiry: 0,
            inputs: vec![],
            outputs: vec![TxOutput { amount: 5_000_000_000, recipient }],
            coinbase_data: Some(height.to_le_bytes().to_vec()),
        }
    }

    fn dummy_block(height: u64, txs: Vec<Transaction>) -> Block {
        Block {
            header: BlockHeader {
                version: 1,
                prev_hash: rupeecoin_crypto::Hash::zero(),
                merkle_root: rupeecoin_crypto::Hash::zero(),
                timestamp: 0,
                height,
                slot: height,
                proposer_index: 0,
                privacy_root: rupeecoin_crypto::Hash::zero(),
                base_fee: 1000,
                gas_used: 0,
                state_root: rupeecoin_crypto::Hash::zero(),
                vrf_output: rupeecoin_crypto::Hash::zero(),
            },
            transactions: txs,
            attestations: vec![],
            proposer_signature: None,
            privacy_txs: vec![],
            vrf_proof: None,
        }
    }

    #[test]
    fn coinbase_immature_spend_rejected() {
        let kp = KeyPair::generate();
        let addr = kp.public_key.to_address();
        let mut utxo_set = UtxoSet::new();

        let cb = coinbase_tx(1, addr);
        let block = dummy_block(1, vec![cb.clone()]);
        utxo_set.apply_block(&block).unwrap();

        let outpoint = rupeecoin_types::OutPoint { txid: cb.txid(), vout: 0 };
        let spend = Transaction {
            version: 1,
            chain_id: 1,
            nonce: 0,
            expiry: 0,
            inputs: vec![TxInput {
                outpoint: outpoint.clone(),
                public_key: kp.public_key.clone(),
                signature: rupeecoin_crypto::Signature::empty(),
            }],
            outputs: vec![TxOutput { amount: 4_999_000_000, recipient: addr }],
            coinbase_data: None,
        };

        // At height 50 (< 1 + 100 = 101), spend must be rejected.
        let err = utxo_set.validate_transfer(&spend, 50).unwrap_err();
        assert!(
            matches!(err, UtxoError::CoinbaseImmature { available_at: 101, current_height: 50 }),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn coinbase_mature_spend_accepted() {
        let kp = KeyPair::generate();
        let addr = kp.public_key.to_address();
        let mut utxo_set = UtxoSet::new();

        let cb = coinbase_tx(1, addr);
        let block = dummy_block(1, vec![cb.clone()]);
        utxo_set.apply_block(&block).unwrap();

        let outpoint = rupeecoin_types::OutPoint { txid: cb.txid(), vout: 0 };
        let sig_hash = {
            let tmp = Transaction {
                version: 1,
                chain_id: 1,
                nonce: 0,
                expiry: 0,
                inputs: vec![TxInput {
                    outpoint: outpoint.clone(),
                    public_key: kp.public_key.clone(),
                    signature: rupeecoin_crypto::Signature::empty(),
                }],
                outputs: vec![TxOutput { amount: 4_999_000_000, recipient: addr }],
                coinbase_data: None,
            };
            tmp.sig_hash()
        };
        let sig = kp.sign(sig_hash.as_bytes());

        let spend = Transaction {
            version: 1,
            chain_id: 1,
            nonce: 0,
            expiry: 0,
            inputs: vec![TxInput {
                outpoint,
                public_key: kp.public_key.clone(),
                signature: sig,
            }],
            outputs: vec![TxOutput { amount: 4_999_000_000, recipient: addr }],
            coinbase_data: None,
        };

        // At height 101 (= 1 + 100), spend must be accepted.
        utxo_set.validate_transfer(&spend, 101).unwrap();
    }
}
