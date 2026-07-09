pub mod genesis;

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

use rupeecoin_consensus::{select_proposer, slot_to_epoch, verify_block_vrf, SLOTS_PER_EPOCH};
use rupeecoin_crypto::{hash_data, Hash, KeyPair, PublicKey, Signature, VrfProof};
use rupeecoin_economics::{
    apply_inactivity_leak, compute_block_rewards, next_base_fee, route_fees,
    treasury_address, PmfpAccumulator,
};
use rupeecoin_privacy::{CommitmentTree, Nullifier, PrivacyBlockApply, PrivacyTxError, SpendEvidence};
use rupeecoin_staking::{ValidatorRegistry, ValidatorStatus};
use rupeecoin_types::{
    block::{merkle_root, privacy_merkle_root},
    Attestation, Block, BlockHeader, OutPoint, SlashingEvidence, Transaction, TxInput, TxOutput,
    MAINNET_CHAIN_ID,
};
use rupeecoin_utxo::{UtxoError, UtxoSet};
use rupeecoin_crypto::Address;
use thiserror::Error;

pub use genesis::{genesis_block, genesis_block_with_timestamp};

/// How many recent blocks to keep in RAM.  Older blocks are retrieved from
/// `ChainStore` on demand.  This prevents unbounded heap growth on long chains.
pub const BLOCK_KEEP_LAST: usize = 512;

#[derive(Error, Debug)]
pub enum ChainError {
    #[error("block height {0} does not follow chain tip {1}")]
    InvalidHeight(u64, u64),
    #[error("prev_hash does not match tip hash")]
    InvalidPrevHash,
    #[error("merkle root mismatch")]
    InvalidMerkleRoot,
    #[error("block has no transactions")]
    EmptyBlock,
    #[error("first transaction must be coinbase")]
    MissingCoinbase,
    #[error("coinbase reward {0} exceeds limit {1}")]
    ExcessiveCoinbaseReward(u64, u64),
    #[error("invalid coinbase distribution: {0}")]
    InvalidCoinbaseDistribution(String),
    #[error("transaction fee {paid} below required base fee {required}")]
    InsufficientFee { paid: u64, required: u64 },
    #[error("non-coinbase transaction at index 0")]
    UnexpectedCoinbase,
    #[error("transaction error: {0}")]
    Utxo(#[from] UtxoError),
    #[error("proposer index {0} does not match expected {1} for slot {2}")]
    InvalidProposerIndex(u32, u32, u64),
    #[error("missing proposer signature for non-genesis block")]
    MissingProposerSignature,
    #[error("proposer signature verification failed")]
    InvalidProposerSignature,
    #[error("proposer index out of range: {0} >= active set size {1}")]
    ProposerIndexOutOfRange(u32, usize),
    #[error("privacy root mismatch")]
    InvalidPrivacyRoot,
    #[error("privacy transaction invalid: {0}")]
    PrivacyTx(#[from] PrivacyTxError),
    #[error("base fee mismatch: got {0}, expected {1}")]
    InvalidBaseFee(u64, u64),
    #[error("gas_used mismatch: got {0}, expected {1}")]
    InvalidGasUsed(u64, u64),
    #[error("state root mismatch: header claims {0:?}, computed {1:?}")]
    InvalidStateRoot(Hash, Hash),
    #[error("VRF proof verification failed for slot {0}")]
    InvalidVrfProof(u64),
    #[error("STARK spend proof invalid: {0}")]
    InvalidStarkProof(String),
    #[error("invalid reward claim: {0}")]
    InvalidRewardClaim(String),
    #[error("invalid slashing report: {0}")]
    InvalidSlashingReport(String),
    #[error("invalid attestation signature in block")]
    InvalidAttestationSignature,
}

/// Maximum depth at which a chain reorg will be attempted.
pub const REORG_DEPTH: usize = 32;

/// Reversible UTXO changes made by a single block.
#[derive(Debug, Clone)]
pub struct BlockDelta {
    /// UTXOs created by this block (removed on rollback).
    pub added: Vec<OutPoint>,
    /// UTXOs consumed by this block (restored on rollback).
    pub removed: Vec<(OutPoint, TxOutput)>,
    /// Coinbase outputs whose maturity records must be removed on rollback.
    pub coinbase_added: Vec<OutPoint>,
    /// Addresses whose nonce was incremented (decremented on rollback).
    pub nonce_incremented: Vec<Address>,
    /// Nullifiers inserted by shielded txs in this block (removed on rollback).
    pub inserted_nullifiers: Vec<Nullifier>,
}

pub struct Chain {
    pub blocks: Vec<Block>,
    /// Parallel to `blocks`: UTXO delta for each block, used for reorg rollback.
    block_deltas: VecDeque<BlockDelta>,
    pub utxo_set: UtxoSet,
    pub commitment_tree: CommitmentTree,
    pub spent_nullifiers: HashSet<Nullifier>,
    pub recent_roots: Vec<Hash>,
    pub pmfp: PmfpAccumulator,
}

pub const ANCHOR_DEPTH: usize = 100;

impl Chain {
    pub fn new(genesis: Block) -> Self {
        let mut utxo_set = UtxoSet::new();
        utxo_set
            .apply_block(&genesis)
            .expect("genesis block must be valid");
        let commitment_tree = CommitmentTree::new();
        let genesis_root = commitment_tree.root();
        // Genesis delta is empty — we never roll back past genesis.
        let genesis_delta = BlockDelta {
            added: vec![],
            removed: vec![],
            coinbase_added: vec![],
            nonce_incremented: vec![],
            inserted_nullifiers: vec![],
        };
        Chain {
            blocks: vec![genesis],
            block_deltas: VecDeque::from([genesis_delta]),
            utxo_set,
            commitment_tree,
            spent_nullifiers: HashSet::new(),
            recent_roots: vec![genesis_root],
            pmfp: PmfpAccumulator::new(),
        }
    }

    pub fn tip(&self) -> &Block {
        self.blocks.last().expect("chain always has genesis")
    }

    pub fn height(&self) -> u64 {
        self.tip().header.height
    }

    /// Return `true` if a block with `hash` is in the canonical chain.
    pub fn contains_hash(&self, hash: &Hash) -> bool {
        self.blocks.iter().any(|b| b.hash() == *hash)
    }

    pub fn add_block(
        &mut self,
        block: Block,
        registry: &mut ValidatorRegistry,
    ) -> Result<(), ChainError> {
        self.validate(&block, registry)?;

        // Measure fees while the spent UTXOs are still present, before apply.
        let total_fees = self.collected_fees(&block);

        // Compute the delta before applying (so we know which UTXOs existed).
        let mut delta = self.compute_delta(&block);
        self.utxo_set.apply_block(&block)?;

        // Pre-compute privacy apply so we can include the new tree root in the
        // state root, without mutating committed state yet.
        let height = block.header.height;
        let apply = PrivacyBlockApply::compute(&block.privacy_txs, &self.commitment_tree, height);

        let utxo_root = self.utxo_set.state_root().expect("UTXO serialization failed");
        let computed = hash_data(
            &[utxo_root.as_bytes().as_slice(), apply.new_tree_root.as_bytes().as_slice()].concat(),
        );
        if block.header.state_root != computed {
            // Undo the UTXO apply before returning the error.
            self.undo_utxo_delta(&delta);
            return Err(ChainError::InvalidStateRoot(block.header.state_root, computed));
        }

        // ── Privacy ───────────────────────────────────────────────────────────
        for cm in &apply.new_commitments {
            self.commitment_tree.append(*cm);
        }
        let mut inserted_nullifiers = Vec::new();
        for (nullifier, _) in &apply.new_nullifiers {
            self.spent_nullifiers.insert(*nullifier);
            inserted_nullifiers.push(*nullifier);
        }
        delta.inserted_nullifiers = inserted_nullifiers;
        self.recent_roots.push(apply.new_tree_root);
        if self.recent_roots.len() > ANCHOR_DEPTH + 1 {
            self.recent_roots.remove(0);
        }

        self.apply_economics(&block, registry, total_fees);
        self.apply_special_txs(&block, registry);
        // Push delta and block together so block_deltas[i] always matches blocks[i].
        self.block_deltas.push_back(delta);
        self.blocks.push(block);
        // Evict old blocks beyond the sliding window; trim both vecs atomically.
        if self.blocks.len() > BLOCK_KEEP_LAST {
            let excess = self.blocks.len() - BLOCK_KEEP_LAST;
            for _ in 0..excess {
                self.block_deltas.pop_front();
            }
            self.blocks.drain(..excess);
        }
        Ok(())
    }

    /// Re-apply a block from the local, previously-validated store on startup.
    ///
    /// Unlike [`add_block`], this does **not** re-run consensus validation
    /// (proposer / VRF) or registry economics.  Two reasons:
    ///   1. The block was already fully validated when it was first accepted and
    ///      persisted, so re-verifying it against the local store is redundant.
    ///   2. Consensus verification needs the validator set as it was at each
    ///      historical slot, but validator-set membership is established
    ///      off-chain (node stake config + gossip announcements) and only the
    ///      *tip* registry is persisted — the historical set is not
    ///      reconstructable from block data alone.  The authoritative registry
    ///      is restored separately from persisted metadata.
    ///
    /// The UTXO/privacy state is still rebuilt deterministically from the block,
    /// and the committed `state_root` is recomputed and checked as an integrity
    /// guard against store corruption.
    pub fn replay_block(&mut self, block: Block) -> Result<(), ChainError> {
        let mut delta = self.compute_delta(&block);
        self.utxo_set.apply_block(&block)?;

        let height = block.header.height;
        let apply = PrivacyBlockApply::compute(&block.privacy_txs, &self.commitment_tree, height);

        let utxo_root = self.utxo_set.state_root().expect("UTXO serialization failed");
        let computed = hash_data(
            &[utxo_root.as_bytes().as_slice(), apply.new_tree_root.as_bytes().as_slice()].concat(),
        );
        if block.header.state_root != computed {
            self.undo_utxo_delta(&delta);
            return Err(ChainError::InvalidStateRoot(block.header.state_root, computed));
        }

        for cm in &apply.new_commitments {
            self.commitment_tree.append(*cm);
        }
        let mut inserted_nullifiers = Vec::new();
        for (nullifier, _) in &apply.new_nullifiers {
            self.spent_nullifiers.insert(*nullifier);
            inserted_nullifiers.push(*nullifier);
        }
        delta.inserted_nullifiers = inserted_nullifiers;
        self.recent_roots.push(apply.new_tree_root);
        if self.recent_roots.len() > ANCHOR_DEPTH + 1 {
            self.recent_roots.remove(0);
        }

        self.block_deltas.push_back(delta);
        self.blocks.push(block);
        if self.blocks.len() > BLOCK_KEEP_LAST {
            let excess = self.blocks.len() - BLOCK_KEEP_LAST;
            for _ in 0..excess {
                self.block_deltas.pop_front();
            }
            self.blocks.drain(..excess);
        }
        Ok(())
    }

    /// Compute the UTXO delta for `block` without applying it.
    fn compute_delta(&self, block: &Block) -> BlockDelta {
        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut coinbase_added = Vec::new();
        let mut nonce_incremented = Vec::new();

        for (tx_idx, tx) in block.transactions.iter().enumerate() {
            let is_coinbase = tx_idx == 0 && tx.inputs.is_empty();
            for input in &tx.inputs {
                if let Some(out) = self.utxo_set.get(&input.outpoint) {
                    removed.push((input.outpoint.clone(), out.clone()));
                }
                if let Some(first) = tx.inputs.first() {
                    let addr = first.public_key.to_address();
                    if !nonce_incremented.contains(&addr) {
                        nonce_incremented.push(addr);
                    }
                }
            }
            let txid = tx.txid();
            for (vout, _) in tx.outputs.iter().enumerate() {
                let op = OutPoint { txid, vout: vout as u32 };
                added.push(op.clone());
                if is_coinbase {
                    coinbase_added.push(op);
                }
            }
        }

        BlockDelta { added, removed, coinbase_added, nonce_incremented, inserted_nullifiers: vec![] }
    }

    /// Undo a block's UTXO and nullifier changes.  Used for reorg rollback.
    fn undo_utxo_delta(&mut self, delta: &BlockDelta) {
        for op in &delta.added {
            self.utxo_set.remove(op);
        }
        for (op, out) in &delta.removed {
            self.utxo_set.insert(op.clone(), out.clone());
        }
        for op in &delta.coinbase_added {
            self.utxo_set.remove_coinbase_height(op);
        }
        for addr in &delta.nonce_incremented {
            self.utxo_set.decrement_nonce(addr);
        }
        // Restore spent nullifiers so the shielded notes can be re-spent on the new fork.
        for nullifier in &delta.inserted_nullifiers {
            self.spent_nullifiers.remove(nullifier);
        }
    }

    /// Roll back `count` blocks from the chain tip, undoing UTXO state.
    /// Returns the blocks that were removed (tip first).
    pub fn rollback(&mut self, count: usize) -> Vec<Block> {
        let mut removed = Vec::new();
        for _ in 0..count.min(self.blocks.len().saturating_sub(1)) {
            if let Some(delta) = self.block_deltas.pop_back() {
                self.undo_utxo_delta(&delta);
            }
            if let Some(block) = self.blocks.pop() {
                removed.push(block);
            }
        }
        removed
    }

    /// Apply a sequence of fork blocks starting just after the current tip.
    /// All blocks must form a valid chain from current tip forward.
    pub fn apply_fork(
        &mut self,
        fork_blocks: Vec<Block>,
        registry: &mut ValidatorRegistry,
    ) -> Result<(), ChainError> {
        for block in fork_blocks {
            self.add_block(block, registry)?;
        }
        Ok(())
    }

    fn apply_special_txs(&self, block: &Block, registry: &mut ValidatorRegistry) {
        for tx in &block.transactions[1..] {
            if tx.is_reward_claim() {
                if let Some(addr) = tx.reward_claim_addr() {
                    let claimed: u64 = tx.outputs.iter().map(|o| o.amount).sum();
                    let _ = registry.withdraw_rewards(&addr, claimed);
                }
            } else if tx.is_slashing_report() {
                if let Some(evidence) = tx.slashing_evidence() {
                    let _ = registry.slash(&slashed_validator_addr(&evidence));
                }
            }
        }
    }

    fn apply_economics(&mut self, block: &Block, registry: &mut ValidatorRegistry, total_fees: u64) {
        let height = block.header.height;
        let slot = block.header.slot;
        let epoch = slot_to_epoch(slot);

        for att in &block.attestations {
            let addr = att.validator_pubkey.to_hash();
            let att_epoch = slot_to_epoch(att.slot);
            registry.mark_attested(&addr, att_epoch);
        }

        let proposer_addr = self.proposer_address(block, registry);
        let split = compute_block_rewards(height, registry, &proposer_addr);
        for (addr, amount) in &split.validator_credits {
            registry.credit_rewards(addr, *amount);
        }

        let fee_split = route_fees(total_fees);
        self.pmfp.add_fees(fee_split.pmfp_contribution);

        if height > 0 && slot % SLOTS_PER_EPOCH == 0 {
            // Process the epoch that just completed, not the one starting now.
            let completed_epoch = epoch.saturating_sub(1);
            registry.process_epoch_participation(completed_epoch);
            apply_inactivity_leak(registry, completed_epoch);
            self.pmfp.distribute_epoch(registry);
        }
    }

    fn proposer_address(&self, block: &Block, registry: &ValidatorRegistry) -> Hash {
        registry
            .active_set()
            .get(block.header.proposer_index as usize)
            .copied()
            .unwrap_or_else(Hash::zero)
    }

    fn validate(&self, block: &Block, registry: &ValidatorRegistry) -> Result<(), ChainError> {
        let tip = self.tip();
        let expected_height = tip.header.height + 1;

        if block.header.height != expected_height {
            return Err(ChainError::InvalidHeight(
                block.header.height,
                tip.header.height,
            ));
        }
        if block.header.prev_hash != tip.hash() {
            return Err(ChainError::InvalidPrevHash);
        }
        if block.header.merkle_root != block.compute_merkle_root() {
            return Err(ChainError::InvalidMerkleRoot);
        }
        if block.transactions.is_empty() {
            return Err(ChainError::EmptyBlock);
        }
        if !block.transactions[0].is_coinbase() {
            return Err(ChainError::MissingCoinbase);
        }

        // ── Fee-market checks ─────────────────────────────────────────────────
        let expected_gas = block.transactions.len().saturating_sub(1) as u64;
        if block.header.gas_used != expected_gas {
            return Err(ChainError::InvalidGasUsed(
                block.header.gas_used,
                expected_gas,
            ));
        }
        let expected_base_fee = next_base_fee(tip.header.base_fee, tip.header.gas_used);
        if block.header.base_fee != expected_base_fee {
            return Err(ChainError::InvalidBaseFee(
                block.header.base_fee,
                expected_base_fee,
            ));
        }

        // ── Coinbase reward ───────────────────────────────────────────────────
        // The coinbase must pay *exactly* the protocol reward split — the
        // proposer's proportional share plus the treasury (PDF) cut — and nothing
        // more. Bounding only the sum would let a low-weight proposer mint the
        // full gross reward to itself while every other validator still accrues
        // its `validator_credits`, nearly doubling the intended emission.
        let active_count = registry.active_count();
        self.validate_coinbase(block, registry, active_count)?;

        // Accumulate claims/reports across the block so a proposer cannot mint
        // more than the available rewards by including the same claim twice, nor
        // slash the same validator twice within one block.
        let mut claimed_per_validator: HashMap<Hash, u64> = HashMap::new();
        let mut slashed_in_block: HashSet<Hash> = HashSet::new();
        for tx in &block.transactions[1..] {
            if tx.is_coinbase() {
                return Err(ChainError::UnexpectedCoinbase);
            } else if tx.is_reward_claim() {
                self.validate_reward_claim(tx, registry)?;
                let addr = tx.reward_claim_addr().expect("validated reward-claim addr");
                let claimed: u64 = tx.outputs.iter().map(|o| o.amount).sum();
                let entry = claimed_per_validator.entry(addr).or_insert(0);
                *entry = entry.saturating_add(claimed);
                let available = registry.get(&addr).map(|v| v.claimable_rewards).unwrap_or(0);
                if *entry > available {
                    return Err(ChainError::InvalidRewardClaim(format!(
                        "cumulative claim {} exceeds available {} for {addr}",
                        *entry, available
                    )));
                }
            } else if tx.is_slashing_report() {
                self.validate_slashing_report(tx, registry)?;
                let slashed = slashed_validator_addr(
                    &tx.slashing_evidence().expect("validated slashing evidence"),
                );
                if !slashed_in_block.insert(slashed) {
                    return Err(ChainError::InvalidSlashingReport(format!(
                        "duplicate slashing report for validator {slashed} in same block"
                    )));
                }
            } else {
                self.utxo_set.validate_transfer(tx, block.header.height)?;
                // A transfer must pay at least the block's base fee so the
                // fee that later flows into the PMFP/burn split is real value
                // actually surrendered by the sender (audit: fee accounting).
                let fee = self.tx_fee(tx);
                if fee < block.header.base_fee {
                    return Err(ChainError::InsufficientFee {
                        paid: fee,
                        required: block.header.base_fee,
                    });
                }
            }
        }

        // ── Attestation signature verification ────────────────────────────────
        for att in &block.attestations {
            let sig_data = Attestation::sig_data(att.slot, &att.block_hash);
            att.validator_pubkey
                .verify(sig_data.as_bytes(), &att.signature)
                .map_err(|_| ChainError::InvalidAttestationSignature)?;
        }

        // ── Privacy validation ────────────────────────────────────────────────
        if block.header.privacy_root != block.compute_privacy_root() {
            return Err(ChainError::InvalidPrivacyRoot);
        }
        for tx in &block.privacy_txs {
            tx.validate_stateless()?;
            // Verify any real STARK spend proofs via Winterfell verifier.
            for input in &tx.shielded_inputs {
                if let SpendEvidence::Stark(sp) = &input.spend_evidence {
                    let zk = rupeecoin_zkproof::StarkSpendProof {
                        proof_bytes: sp.proof_bytes.clone(),
                        anchor_felts: sp.anchor_felts,
                    };
                    rupeecoin_zkproof::verify_spend(&zk, sp.nullifier_felts)
                        .map_err(|e| ChainError::InvalidStarkProof(e.to_string()))?;
                }
            }
            tx.validate_stateful(&self.recent_roots, &self.spent_nullifiers)?;
        }

        // ── Consensus validation ──────────────────────────────────────────────
        let slot = block.header.slot;

        if block.header.height > 0 {
            if active_count > 0 {
                // Active validator set: proposer signature AND VRF proof are both
                // mandatory.  A block with either absent is unconditionally rejected.
                let sig = block
                    .proposer_signature
                    .as_ref()
                    .ok_or(ChainError::MissingProposerSignature)?;

                let idx = block.header.proposer_index as usize;
                if idx >= active_count {
                    return Err(ChainError::ProposerIndexOutOfRange(
                        block.header.proposer_index,
                        active_count,
                    ));
                }
                let expected_idx =
                    select_proposer(slot, &tip.header.vrf_output, active_count) as u32;
                if block.header.proposer_index != expected_idx {
                    return Err(ChainError::InvalidProposerIndex(
                        block.header.proposer_index,
                        expected_idx,
                        slot,
                    ));
                }
                let &proposer_addr = registry.active_set().get(idx).unwrap();
                let proposer = registry
                    .get(&proposer_addr)
                    .ok_or(ChainError::InvalidProposerSignature)?;

                proposer
                    .pubkey
                    .verify(block.hash().as_bytes(), sig)
                    .map_err(|_| ChainError::InvalidProposerSignature)?;

                // vrf_proof must be present when validators are active.
                if block.vrf_proof.is_none() {
                    return Err(ChainError::InvalidVrfProof(slot));
                }
                if !verify_block_vrf(block, &proposer.pubkey, &tip.header.vrf_output) {
                    return Err(ChainError::InvalidVrfProof(slot));
                }
            } else {
                // Bootstrap (no validators yet): vrf_output must equal
                // BLAKE3(prev_vrf_output || slot) so peers cannot freely choose it.
                let mut buf = [0u8; 40];
                buf[..32].copy_from_slice(tip.header.vrf_output.as_bytes());
                buf[32..].copy_from_slice(&slot.to_le_bytes());
                let expected_vrf = hash_data(&buf);
                if block.header.vrf_output != expected_vrf {
                    return Err(ChainError::InvalidVrfProof(slot));
                }
            }
        }

        Ok(())
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn slashed_validator_addr(evidence: &SlashingEvidence) -> Hash {
    match evidence {
        SlashingEvidence::DoubleProposal { proposer_pubkey, .. } => proposer_pubkey.to_hash(),
        SlashingEvidence::DoubleVote { attestation_a, .. } => attestation_a.validator_pubkey.to_hash(),
    }
}

impl Chain {
    fn validate_reward_claim(
        &self,
        tx: &Transaction,
        registry: &ValidatorRegistry,
    ) -> Result<(), ChainError> {
        let addr = tx.reward_claim_addr().ok_or_else(|| {
            ChainError::InvalidRewardClaim("missing validator address in claim".into())
        })?;

        // ── Authenticate the claim ────────────────────────────────────────────
        // The claim must carry the validator's Dilithium3 public key and a
        // signature over (domain ‖ validator_addr ‖ outputs). Without this,
        // any block producer could redirect another validator's rewards to
        // an address they control (audit H-10).
        let (pk_bytes, sig_bytes) = tx.reward_claim_auth().ok_or_else(|| {
            ChainError::InvalidRewardClaim("missing claim authorization".into())
        })?;
        let pk = PublicKey::from_bytes(pk_bytes)
            .map_err(|_| ChainError::InvalidRewardClaim("invalid claim public key".into()))?;
        if pk.to_hash() != addr {
            return Err(ChainError::InvalidRewardClaim(
                "claim public key does not match validator address".into(),
            ));
        }
        let sig = Signature::from_bytes(sig_bytes)
            .map_err(|_| ChainError::InvalidRewardClaim("invalid claim signature".into()))?;
        let msg = reward_claim_sig_message(&addr, &tx.outputs);
        pk.verify(&msg, &sig).map_err(|_| {
            ChainError::InvalidRewardClaim("claim signature verification failed".into())
        })?;

        let v = registry
            .get(&addr)
            .ok_or_else(|| ChainError::InvalidRewardClaim(format!("validator {addr} not found")))?;
        let claimable = v.claimable_rewards;
        let claimed: u64 = tx.outputs.iter().map(|o| o.amount).sum();
        if claimed > claimable {
            return Err(ChainError::InvalidRewardClaim(format!(
                "claimed {claimed} but only {claimable} available"
            )));
        }
        Ok(())
    }

    fn validate_slashing_report(
        &self,
        tx: &Transaction,
        registry: &ValidatorRegistry,
    ) -> Result<(), ChainError> {
        let evidence = tx.slashing_evidence().ok_or_else(|| {
            ChainError::InvalidSlashingReport("malformed slashing evidence".into())
        })?;

        // Verify cryptographic signatures.
        match &evidence {
            SlashingEvidence::DoubleProposal {
                proposer_pubkey,
                block_hash_a,
                sig_a,
                block_hash_b,
                sig_b,
                ..
            } => {
                if block_hash_a == block_hash_b {
                    return Err(ChainError::InvalidSlashingReport(
                        "double-proposal blocks must be distinct".into(),
                    ));
                }
                proposer_pubkey
                    .verify(block_hash_a.as_bytes(), sig_a)
                    .map_err(|_| ChainError::InvalidSlashingReport("invalid sig_a".into()))?;
                proposer_pubkey
                    .verify(block_hash_b.as_bytes(), sig_b)
                    .map_err(|_| ChainError::InvalidSlashingReport("invalid sig_b".into()))?;
            }
            SlashingEvidence::DoubleVote {
                attestation_a,
                attestation_b,
            } => {
                if attestation_a.validator_pubkey != attestation_b.validator_pubkey {
                    return Err(ChainError::InvalidSlashingReport(
                        "attestations from different validators".into(),
                    ));
                }
                let sd_a = Attestation::sig_data(attestation_a.slot, &attestation_a.block_hash);
                attestation_a
                    .validator_pubkey
                    .verify(sd_a.as_bytes(), &attestation_a.signature)
                    .map_err(|_| ChainError::InvalidSlashingReport("invalid attestation_a sig".into()))?;
                let sd_b = Attestation::sig_data(attestation_b.slot, &attestation_b.block_hash);
                attestation_b
                    .validator_pubkey
                    .verify(sd_b.as_bytes(), &attestation_b.signature)
                    .map_err(|_| ChainError::InvalidSlashingReport("invalid attestation_b sig".into()))?;
            }
        }

        let slashed_addr = slashed_validator_addr(&evidence);
        let v = registry.get(&slashed_addr).ok_or_else(|| {
            ChainError::InvalidSlashingReport(format!("validator {slashed_addr} not found"))
        })?;
        if v.status == ValidatorStatus::Slashed {
            return Err(ChainError::InvalidSlashingReport(
                "validator already slashed".into(),
            ));
        }
        let reporter_reward = v.stake / 2;
        let output_sum: u64 = tx.outputs.iter().map(|o| o.amount).sum();
        if output_sum > reporter_reward {
            return Err(ChainError::InvalidSlashingReport(format!(
                "output {output_sum} exceeds 50% of stake ({reporter_reward})"
            )));
        }
        Ok(())
    }

    /// Enforce that the coinbase pays exactly the protocol reward split: output 0
    /// is the proposer's proportional share, output 1 is the treasury (PDF) cut.
    /// Rejecting anything else stops a proposer from minting more than the
    /// emission schedule allows.
    fn validate_coinbase(
        &self,
        block: &Block,
        registry: &ValidatorRegistry,
        active_count: usize,
    ) -> Result<(), ChainError> {
        let coinbase = &block.transactions[0];
        if coinbase.outputs.len() != 2 {
            return Err(ChainError::InvalidCoinbaseDistribution(format!(
                "coinbase must have exactly 2 outputs (proposer + treasury), got {}",
                coinbase.outputs.len()
            )));
        }

        // Resolve the proposer address exactly as build_coinbase / apply_economics.
        let proposer_hash = if active_count > 0 {
            registry
                .active_set()
                .get(block.header.proposer_index as usize)
                .copied()
                .unwrap_or_else(Hash::zero)
        } else {
            Hash::zero()
        };
        let split = compute_block_rewards(block.header.height, registry, &proposer_hash);

        let proposer_out = &coinbase.outputs[0];
        let treasury_out = &coinbase.outputs[1];

        if proposer_out.amount != split.proposer_reward {
            return Err(ChainError::InvalidCoinbaseDistribution(format!(
                "proposer reward {} does not match expected {}",
                proposer_out.amount, split.proposer_reward
            )));
        }
        if treasury_out.amount != split.treasury_amount {
            return Err(ChainError::InvalidCoinbaseDistribution(format!(
                "treasury amount {} does not match expected {}",
                treasury_out.amount, split.treasury_amount
            )));
        }
        if treasury_out.recipient != treasury_address() {
            return Err(ChainError::InvalidCoinbaseDistribution(
                "treasury output must pay the canonical treasury address".into(),
            ));
        }
        // With an active set the proposer's identity — and thus payout address —
        // is fixed; pin output 0 to it. (In bootstrap there is no registered
        // proposer to check against.)
        if active_count > 0 {
            if let Some(v) = registry.get(&proposer_hash) {
                if proposer_out.recipient != v.pubkey.to_address() {
                    return Err(ChainError::InvalidCoinbaseDistribution(
                        "proposer output must pay the selected proposer's address".into(),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Fee paid by a single transfer: Σ input amounts − Σ output amounts.
    /// Only valid once the tx has passed `validate_transfer` (inputs exist and
    /// input sum ≥ output sum) and while its inputs are still in the UTXO set.
    fn tx_fee(&self, tx: &Transaction) -> u64 {
        let in_sum: u64 = tx
            .inputs
            .iter()
            .filter_map(|i| self.utxo_set.get(&i.outpoint))
            .map(|o| o.amount)
            .sum();
        let out_sum: u64 = tx.outputs.iter().map(|o| o.amount).sum();
        in_sum.saturating_sub(out_sum)
    }

    /// Total fees collected by a block. Coinbase, reward-claim and
    /// slashing-report transactions carry no inputs and contribute nothing.
    /// Must be called before the block is applied, while spent UTXOs still exist.
    fn collected_fees(&self, block: &Block) -> u64 {
        let mut fees: u64 = 0;
        for tx in &block.transactions {
            if tx.inputs.is_empty() {
                continue;
            }
            fees = fees.saturating_add(self.tx_fee(tx));
        }
        fees
    }
}

// ── Transaction builders ──────────────────────────────────────────────────────

pub fn build_coinbase(
    height: u64,
    proposer_pubkey: &PublicKey,
    proposer_addr: &Hash,
    registry: &ValidatorRegistry,
) -> Transaction {
    let split = compute_block_rewards(height, registry, proposer_addr);
    Transaction {
        version: 1,
        chain_id: MAINNET_CHAIN_ID,
        nonce: 0,
        expiry: 0,
        inputs: vec![],
        outputs: vec![
            TxOutput {
                amount: split.proposer_reward,
                recipient: proposer_pubkey.to_address(),
            },
            TxOutput {
                amount: split.treasury_amount,
                recipient: treasury_address(),
            },
        ],
        coinbase_data: Some(height.to_le_bytes().to_vec()),
    }
}

/// Build a reward-claim transaction.  The proposer claims all their accrued
/// rewards and receives them as a UTXO at `output_addr`.
/// The message a validator signs to authorise a reward claim. Binds the claim
/// to the validator address and the exact payout outputs so it cannot be
/// replayed with a different recipient.
fn reward_claim_sig_message(validator_addr: &Hash, outputs: &[TxOutput]) -> Vec<u8> {
    let mut m = b"rupeecoin-reward-claim-v1".to_vec();
    m.extend_from_slice(validator_addr.as_bytes());
    for o in outputs {
        m.extend_from_slice(o.recipient.as_bytes());
        m.extend_from_slice(&o.amount.to_le_bytes());
    }
    m
}

pub fn build_reward_claim(
    validator_addr: Hash,
    amount: u64,
    output_addr: rupeecoin_crypto::Address,
    keypair: &KeyPair,
) -> Transaction {
    let outputs = vec![TxOutput { amount, recipient: output_addr }];
    let sig = keypair.sign(&reward_claim_sig_message(&validator_addr, &outputs));
    let pk = keypair.public_key.as_bytes();

    let mut data = b"RC\x01\x00\x00\x00\x00\x00\x00".to_vec();
    data.extend_from_slice(validator_addr.as_bytes());
    data.extend_from_slice(&(pk.len() as u32).to_le_bytes());
    data.extend_from_slice(pk);
    data.extend_from_slice(sig.as_bytes());
    Transaction {
        version: 1,
        chain_id: MAINNET_CHAIN_ID,
        nonce: 0,
        expiry: 0,
        inputs: vec![],
        outputs,
        coinbase_data: Some(data),
    }
}

/// Build a slashing-report transaction.  The reporter (`reporter_addr`) will
/// receive 50% of the slashed validator's stake as a UTXO output.
pub fn build_slash_report(
    evidence: &SlashingEvidence,
    reporter_addr: rupeecoin_crypto::Address,
    registry: &ValidatorRegistry,
) -> Option<Transaction> {
    let slashed_addr = slashed_validator_addr(evidence);
    let stake = registry.get(&slashed_addr)?.stake;
    if stake == 0 {
        return None;
    }
    let reporter_reward = stake / 2;
    let mut data = b"SR\x01\x00\x00\x00\x00\x00\x00".to_vec();
    data.extend_from_slice(&bincode::serialize(evidence).ok()?);
    Some(Transaction {
        version: 1,
        chain_id: MAINNET_CHAIN_ID,
        nonce: 0,
        expiry: 0,
        inputs: vec![],
        outputs: vec![TxOutput { amount: reporter_reward, recipient: reporter_addr }],
        coinbase_data: Some(data),
    })
}

pub fn build_transfer(
    keypair: &KeyPair,
    outpoints: Vec<OutPoint>,
    outputs: Vec<TxOutput>,
    chain_id: u32,
    nonce: u64,
    expiry: u64,
) -> Transaction {
    let mut tx = Transaction {
        version: 1,
        chain_id,
        nonce,
        expiry,
        inputs: outpoints
            .into_iter()
            .map(|op| TxInput {
                outpoint: op,
                public_key: keypair.public_key.clone(),
                signature: Signature::empty(),
            })
            .collect(),
        outputs,
        coinbase_data: None,
    };
    let sig_hash = tx.sig_hash();
    let sig = keypair.sign(sig_hash.as_bytes());
    for input in &mut tx.inputs {
        input.signature = sig.clone();
    }
    tx
}

pub fn build_block(
    chain: &Chain,
    proposer_keypair: &KeyPair,
    registry: &ValidatorRegistry,
    attestations: Vec<Attestation>,
    extra_txs: Vec<Transaction>,
    privacy_txs: Vec<rupeecoin_privacy::PrivacyTransaction>,
) -> Block {
    let tip = chain.tip();
    let height = tip.header.height + 1;
    let slot = height;
    let prev_hash = tip.hash();

    let active_count = registry.active_count();
    let proposer_index =
        select_proposer(slot, &tip.header.vrf_output, active_count) as u32;
    let proposer_addr = registry
        .active_set()
        .get(proposer_index as usize)
        .copied()
        .unwrap_or_else(Hash::zero);

    let base_fee = next_base_fee(tip.header.base_fee, tip.header.gas_used);
    let gas_used = extra_txs.len() as u64;

    let coinbase = build_coinbase(height, &proposer_keypair.public_key, &proposer_addr, registry);
    let mut transactions = vec![coinbase];
    transactions.extend(extra_txs);
    let mr = merkle_root(&transactions);

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(tip.header.timestamp + 4);

    let privacy_root = privacy_merkle_root(&privacy_txs);

    // ── VRF ───────────────────────────────────────────────────────────────────
    let (vrf_output, vrf_proof) = if active_count > 0 {
        let proof = VrfProof::generate(proposer_keypair, slot, &tip.header.vrf_output);
        let output = proof.output;
        (output, Some(proof.signature))
    } else {
        // Bootstrap: no validators registered yet — derive output deterministically.
        let mut buf = [0u8; 40];
        buf[..32].copy_from_slice(tip.header.vrf_output.as_bytes());
        buf[32..].copy_from_slice(&slot.to_le_bytes());
        (rupeecoin_crypto::hash_data(&buf), None)
    };

    // ── State root ────────────────────────────────────────────────────────────
    // Combined: BLAKE3(utxo_root || post-privacy-tree-root) so the state root
    // commits to both transparent and shielded pool state.
    let state_root = {
        let mut snap = chain.utxo_set.clone();
        snap.apply_transactions(&transactions);
        let utxo_root = snap.state_root().expect("UTXO serialization failed");
        let future_apply =
            PrivacyBlockApply::compute(&privacy_txs, &chain.commitment_tree, height);
        hash_data(
            &[utxo_root.as_bytes().as_slice(), future_apply.new_tree_root.as_bytes().as_slice()]
                .concat(),
        )
    };

    let header = BlockHeader {
        version: 1,
        prev_hash,
        merkle_root: mr,
        timestamp,
        height,
        slot,
        proposer_index,
        privacy_root,
        base_fee,
        gas_used,
        state_root,
        vrf_output,
    };

    let mut block = Block {
        header,
        transactions,
        attestations,
        proposer_signature: None,
        privacy_txs,
        vrf_proof,
    };

    if active_count >= 1 {
        let block_hash = block.hash();
        block.proposer_signature = Some(proposer_keypair.sign(block_hash.as_bytes()));
    }

    block
}
