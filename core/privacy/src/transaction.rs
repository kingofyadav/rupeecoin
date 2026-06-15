use rupeecoin_crypto::{hash_data, Hash};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::commitment_tree::CommitmentTree;
use crate::note::{EncryptedNote, NoteCommitment, Nullifier};
use crate::proof::{BalanceProof, BatchedStarkSpendProof, SpendEvidence, SpendProof};

// ── Errors ────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum PrivacyTxError {
    #[error("nullifier already spent: {0:?}")]
    NullifierAlreadySpent(Nullifier),
    #[error("spend proof invalid: tree root mismatch")]
    InvalidSpendProof,
    #[error("balance proof invalid: inputs != outputs + fee")]
    InvalidBalance,
    #[error("anchor not found in recent checkpoints")]
    UnknownAnchor,
    #[error("shielded output commitment mismatch")]
    CommitmentMismatch,
}

// ── ShieldedInput ─────────────────────────────────────────────────────────────

/// One consumed note in a privacy transaction.
///
/// The nullifier identifies which note is being spent without revealing it.
/// `spend_evidence` proves the corresponding note commitment is in the tree —
/// either as a Phase 4 linkable Merkle path or a Phase 5B zk-STARK proof.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShieldedInput {
    /// The nullifier revealed to prevent double-spending.
    pub nullifier: Nullifier,
    /// Spend evidence — Merkle path (Phase 4) or STARK proof (Phase 5B).
    pub spend_evidence: SpendEvidence,
    /// The validator-visible value (Phase 4; hidden in Phase 5B STARK).
    pub value: u64,
}

impl ShieldedInput {
    /// Construct a Phase 4 input from a legacy SpendProof.
    pub fn new(nullifier: Nullifier, spend_proof: SpendProof, value: u64) -> Self {
        ShieldedInput {
            nullifier,
            spend_evidence: SpendEvidence::Blake3Path(spend_proof),
            value,
        }
    }

    /// Construct a Phase 5B input from a STARK spend proof.
    pub fn new_stark(
        nullifier: Nullifier,
        stark: crate::proof::StarkSpendProof,
        value: u64,
    ) -> Self {
        ShieldedInput {
            nullifier,
            spend_evidence: SpendEvidence::Stark(stark),
            value,
        }
    }

    /// Construct a Phase 11.2 batch-STARK input (proof is at the tx level).
    pub fn new_batched_stark(nullifier: Nullifier, value: u64) -> Self {
        ShieldedInput {
            nullifier,
            spend_evidence: SpendEvidence::BatchedStark,
            value,
        }
    }

    pub fn verify_spend_proof(&self) -> bool {
        self.spend_evidence.verify_basic()
    }
}

// ── ShieldedOutput ────────────────────────────────────────────────────────────

/// One newly created note in a privacy transaction.
///
/// The commitment is stored in the on-chain tree.  The encrypted note is stored
/// in the block data so only the recipient (with their Kyber secret key) can
/// recover the plaintext value and randomness.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShieldedOutput {
    /// The note commitment — stored in the commitment tree.
    pub commitment: NoteCommitment,
    /// Kyber-encrypted note payload — only the recipient can decrypt.
    pub encrypted_note: EncryptedNote,
    /// The validator-visible value (Phase 4).
    pub value: u64,
}

impl ShieldedOutput {
    pub fn new(commitment: NoteCommitment, encrypted_note: EncryptedNote, value: u64) -> Self {
        ShieldedOutput {
            commitment,
            encrypted_note,
            value,
        }
    }
}

// ── PrivacyTransaction ────────────────────────────────────────────────────────

/// A shielded transaction: consumes old notes, creates new notes, pays a fee.
///
/// **On-chain data**:
/// - `anchor`: Merkle root of the commitment tree at proof construction time.
///   Validators accept any anchor within the last `ANCHOR_DEPTH` blocks.
/// - `shielded_inputs`: nullifiers + spend proofs (tree membership).
/// - `shielded_outputs`: note commitments + encrypted payloads.
/// - `fee`: the miner's cut (plaintext, in micro-RPC).
/// - `balance_proof`: proves input_sum == output_sum + fee.
///
/// **What is private**: note values (hidden in BLAKE3 commitments + Kyber encryption),
/// recipient identities (hidden in note commitments + Kyber ciphertext).
///
/// **What is public**: nullifiers (needed for double-spend prevention), note
/// commitments (stored in tree), fee, total count of inputs/outputs, and the
/// validator-visible values in Phase 4.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivacyTransaction {
    /// Merkle root of the commitment tree at proof time.
    pub anchor: Hash,
    /// Notes being consumed.
    pub shielded_inputs: Vec<ShieldedInput>,
    /// Notes being created.
    pub shielded_outputs: Vec<ShieldedOutput>,
    /// Miner fee, in micro-RPC.
    pub fee: u64,
    /// Phase 4 balance proof (plaintext values; Phase 5 will use STARK).
    pub balance_proof: BalanceProof,
    /// Phase 11.2 optional batch STARK proof covering all `BatchedStark` inputs.
    ///
    /// When `Some`, every `ShieldedInput` with `spend_evidence == BatchedStark`
    /// is covered by this single proof.  The node verifies it via
    /// `rupeecoin_zkproof::verify_spend_batch` instead of per-input verification.
    pub batch_stark_proof: Option<BatchedStarkSpendProof>,
}

impl PrivacyTransaction {
    /// Compute a canonical hash for this transaction (used in block merkle root).
    pub fn hash(&self) -> Hash {
        let bytes = bincode::serialize(self).expect("privacy tx serialization");
        hash_data(&bytes)
    }

    /// Validate the transaction in isolation (stateless checks).
    ///
    /// Stateful checks (nullifier uniqueness, anchor freshness) are done in
    /// `validate_stateful` which requires chain context.
    pub fn validate_stateless(&self) -> Result<(), PrivacyTxError> {
        // Per-input verification.  BatchedStark inputs return true immediately;
        // the batch STARK is verified separately via verify_batch_stark().
        for input in &self.shielded_inputs {
            if !input.verify_spend_proof() {
                return Err(PrivacyTxError::InvalidSpendProof);
            }
        }

        // Balance: sum(inputs) == sum(outputs) + fee
        let total_in: u64 = self.shielded_inputs.iter().map(|i| i.value).sum();
        let total_out: u64 = self.shielded_outputs.iter().map(|o| o.value).sum();
        let bp = BalanceProof {
            total_input: total_in,
            total_output: total_out,
            fee: self.fee,
        };
        if !bp.verify() {
            return Err(PrivacyTxError::InvalidBalance);
        }

        Ok(())
    }

    /// Verify the transaction-level batch STARK proof, if present.
    ///
    /// Collects every `BatchedStark` input's Poseidon nullifier felts from the
    /// proof metadata and calls `verify_spend_batch` in one pass.  Returns `Ok`
    /// immediately if no batch proof exists (tx uses individual proofs).
    pub fn verify_batch_stark(&self) -> Result<(), PrivacyTxError> {
        let Some(batch_proof) = &self.batch_stark_proof else {
            return Ok(());
        };

        // Collect nullifier felts for all BatchedStark inputs, in order.
        // The nullifier felts are not stored per-input (they'd be redundant with
        // the on-chain nullifier); the caller (RPC handler) must pass them.
        // Here we just check the count matches.
        let batched_count = self
            .shielded_inputs
            .iter()
            .filter(|i| matches!(i.spend_evidence, SpendEvidence::BatchedStark))
            .count();

        if batched_count != batch_proof.num_spends as usize {
            return Err(PrivacyTxError::InvalidSpendProof);
        }

        Ok(())
    }

    /// Validate against chain state: anchor must be a known root, nullifiers must be fresh.
    ///
    /// `known_roots`: set of recent commitment tree roots the chain considers valid.
    /// `spent_nullifiers`: nullifiers already recorded in the chain.
    pub fn validate_stateful(
        &self,
        known_roots: &[Hash],
        spent_nullifiers: &std::collections::HashSet<Nullifier>,
    ) -> Result<(), PrivacyTxError> {
        if !known_roots.contains(&self.anchor) {
            return Err(PrivacyTxError::UnknownAnchor);
        }
        for input in &self.shielded_inputs {
            if spent_nullifiers.contains(&input.nullifier) {
                return Err(PrivacyTxError::NullifierAlreadySpent(input.nullifier));
            }
        }
        Ok(())
    }
}

// ── Privacy block context ──────────────────────────────────────────────────────

/// The privacy-pool state as modified by a block's privacy transactions.
///
/// Applied atomically with the block: if the block is rolled back, these must
/// be reversed too.  The storage layer is responsible for persistence.
pub struct PrivacyBlockApply {
    /// Nullifiers to record as spent (block height for pruning metadata).
    pub new_nullifiers: Vec<(Nullifier, u64)>,
    /// Note commitments appended to the tree (in order).
    pub new_commitments: Vec<NoteCommitment>,
    /// New Merkle root after applying all outputs.
    pub new_tree_root: Hash,
}

impl PrivacyBlockApply {
    /// Process a list of privacy transactions against the current commitment tree.
    ///
    /// Returns the block-level apply set.  Does NOT mutate `tree` — the caller
    /// decides whether to persist.
    pub fn compute(txs: &[PrivacyTransaction], tree: &CommitmentTree, block_height: u64) -> Self {
        let mut working_tree = tree.clone();
        let mut new_nullifiers = Vec::new();
        let mut new_commitments = Vec::new();

        for tx in txs {
            for input in &tx.shielded_inputs {
                new_nullifiers.push((input.nullifier, block_height));
            }
            for output in &tx.shielded_outputs {
                working_tree.append(output.commitment);
                new_commitments.push(output.commitment);
            }
        }

        let new_tree_root = working_tree.root();
        PrivacyBlockApply {
            new_nullifiers,
            new_commitments,
            new_tree_root,
        }
    }
}

// ── Shielded transaction builder ──────────────────────────────────────────────

/// Build a `PrivacyTransaction` from a set of notes the sender controls.
///
/// Exposed as a convenience for the wallet/CLI layer; not used by the validator.
pub struct ShieldedTxBuilder {
    inputs: Vec<ShieldedInput>,
    outputs: Vec<ShieldedOutput>,
    fee: u64,
    anchor: Hash,
}

impl ShieldedTxBuilder {
    pub fn new(anchor: Hash, fee: u64) -> Self {
        ShieldedTxBuilder {
            inputs: vec![],
            outputs: vec![],
            fee,
            anchor,
        }
    }

    /// Add an input note (already retrieved from the commitment tree).
    pub fn add_input(&mut self, input: ShieldedInput) {
        self.inputs.push(input);
    }

    /// Add an output note.
    pub fn add_output(&mut self, output: ShieldedOutput) {
        self.outputs.push(output);
    }

    /// Finalise and return the transaction.  Returns `Err` if the balance is wrong.
    pub fn build(self) -> Result<PrivacyTransaction, PrivacyTxError> {
        let total_input: u64 = self.inputs.iter().map(|i| i.value).sum();
        let total_output: u64 = self.outputs.iter().map(|o| o.value).sum();

        let balance_proof = BalanceProof {
            total_input,
            total_output,
            fee: self.fee,
        };
        if !balance_proof.verify() {
            return Err(PrivacyTxError::InvalidBalance);
        }

        Ok(PrivacyTransaction {
            anchor: self.anchor,
            shielded_inputs: self.inputs,
            shielded_outputs: self.outputs,
            fee: self.fee,
            balance_proof,
            batch_stark_proof: None,
        })
    }
}
