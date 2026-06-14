use std::collections::HashMap;

use rupeecoin_crypto::{leaf_hash, node_hash, Address, Hash};
use rupeecoin_types::{Block, OutPoint, Transaction, TxOutput};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum UtxoError {
    #[error("input not found in UTXO set: {0:?}")]
    InputNotFound(OutPoint),
    #[error("public key does not own the referenced output")]
    InvalidOwnership,
    #[error("invalid signature on input {0}")]
    InvalidSignature(usize),
    #[error("output sum exceeds input sum")]
    InsufficientFunds,
    #[error("transaction expired at block {expiry} (current height {height})")]
    Expired { expiry: u64, height: u64 },
    #[error("invalid nonce: expected {expected}, got {got}")]
    InvalidNonce { expected: u64, got: u64 },
}

/// In-memory UTXO set with nonce tracking and state-root support.
#[derive(Clone)]
pub struct UtxoSet {
    utxos: HashMap<OutPoint, TxOutput>,
    /// Per-address sequential nonce.  Incremented each time an address spends a UTXO.
    pub nonces: HashMap<Address, u64>,
}

impl UtxoSet {
    pub fn new() -> Self {
        UtxoSet {
            utxos: HashMap::new(),
            nonces: HashMap::new(),
        }
    }

    pub fn get(&self, outpoint: &OutPoint) -> Option<&TxOutput> {
        self.utxos.get(outpoint)
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
        // Expiry check (0 means no expiry).
        if tx.expiry != 0 && current_height > tx.expiry {
            return Err(UtxoError::Expired {
                expiry: tx.expiry,
                height: current_height,
            });
        }

        // Nonce check: sender is the address of the first input's public key.
        if let Some(first) = tx.inputs.first() {
            let sender = first.public_key.to_address();
            let expected = self.nonces.get(&sender).copied().unwrap_or(0);
            if tx.nonce != expected {
                return Err(UtxoError::InvalidNonce {
                    expected,
                    got: tx.nonce,
                });
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
                .expect("input sum overflow");
        }

        let output_sum: u64 = tx
            .outputs
            .iter()
            .try_fold(0u64, |acc, o| acc.checked_add(o.amount))
            .expect("output sum overflow");
        if output_sum > input_sum {
            return Err(UtxoError::InsufficientFunds);
        }

        Ok(())
    }

    /// Apply a validated block: remove spent outputs, add new outputs, increment nonces.
    pub fn apply_block(&mut self, block: &Block) -> Result<(), UtxoError> {
        for tx in &block.transactions {
            for input in &tx.inputs {
                self.utxos
                    .remove(&input.outpoint)
                    .ok_or_else(|| UtxoError::InputNotFound(input.outpoint.clone()))?;
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
    pub fn state_root(&self) -> Hash {
        if self.utxos.is_empty() {
            return Hash::zero();
        }

        let mut entries: Vec<(&OutPoint, &TxOutput)> = self.utxos.iter().collect();
        entries.sort_by_key(|(op, _)| bincode::serialize(op).expect("outpoint serialize"));

        let mut layer: Vec<Hash> = entries
            .iter()
            .map(|(op, out)| {
                let mut data = bincode::serialize(op).expect("outpoint serialize");
                data.extend(bincode::serialize(out).expect("txoutput serialize"));
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
        layer[0]
    }
}

impl Default for UtxoSet {
    fn default() -> Self {
        Self::new()
    }
}
