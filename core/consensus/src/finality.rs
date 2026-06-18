use rupeecoin_crypto::Hash;
use std::collections::HashSet;

/// Simplified Casper FFG finality tracker.
///
/// Justification: an epoch checkpoint is justified when ≥ 2/3 of active stake
/// has attested to the epoch boundary block (checked by AttestationPool).
///
/// Finality: two consecutively justified epochs causes the earlier one to be
/// finalized.
pub struct FinalityTracker {
    /// Set of epoch numbers that have been justified.
    justified: HashSet<u64>,
    /// Last justified epoch boundary slot's block hash (for fork-choice).
    justified_checkpoints: Vec<(u64, u64, Hash)>, // (epoch, slot, block_hash)
    last_finalized_epoch: u64,
    last_justified_epoch: u64,
}

impl FinalityTracker {
    pub fn new() -> Self {
        FinalityTracker {
            justified: HashSet::new(),
            justified_checkpoints: Vec::new(),
            last_finalized_epoch: 0,
            last_justified_epoch: 0,
        }
    }

    /// Mark `epoch` as justified, citing `source_epoch` as the supermajority
    /// link source (Casper FFG: source → target must be a valid link).
    /// Returns `true` if this was a new justification.
    pub fn justify(&mut self, epoch: u64, slot: u64, block_hash: Hash) -> bool {
        self.justify_with_source(epoch, slot, block_hash, epoch.saturating_sub(1))
    }

    /// Core justification with explicit source epoch.
    /// Enforces the Casper FFG supermajority link rule:
    ///   source must equal last_justified_epoch (or 0 for genesis bootstrap).
    pub fn justify_with_source(
        &mut self,
        epoch: u64,
        slot: u64,
        block_hash: Hash,
        source_epoch: u64,
    ) -> bool {
        if self.justified.contains(&epoch) {
            return false;
        }
        // Supermajority link check: source must be the most recently justified epoch,
        // or 0 (bootstrap). This prevents two disjoint 2/3-subsets from each
        // justifying a different checkpoint and then both finalizing.
        let valid_source = source_epoch == 0 || source_epoch == self.last_justified_epoch;
        if !valid_source {
            return false;
        }

        self.justified.insert(epoch);
        self.justified_checkpoints.push((epoch, slot, block_hash));
        if epoch > self.last_justified_epoch {
            self.last_justified_epoch = epoch;
        }

        // Finality rule: if epoch and epoch-1 are both justified via a valid
        // supermajority link, finalize epoch-1.
        if epoch > 0 && self.justified.contains(&(epoch - 1)) {
            let candidate = epoch - 1;
            if candidate > self.last_finalized_epoch {
                self.last_finalized_epoch = candidate;
            }
        }
        true
    }

    pub fn is_justified(&self, epoch: u64) -> bool {
        self.justified.contains(&epoch)
    }

    pub fn last_justified_epoch(&self) -> u64 {
        self.last_justified_epoch
    }

    pub fn last_finalized_epoch(&self) -> u64 {
        self.last_finalized_epoch
    }

    /// Latest finalized checkpoint block hash, if any epoch beyond 0 is finalized.
    pub fn finalized_checkpoint(&self) -> Option<(u64, Hash)> {
        self.justified_checkpoints
            .iter()
            .filter(|(epoch, _, _)| *epoch <= self.last_finalized_epoch)
            .max_by_key(|(epoch, _, _)| *epoch)
            .map(|(epoch, _, hash)| (*epoch, *hash))
    }

    /// Remove checkpoint entries older than `last_finalized_epoch - 1` to
    /// prevent unbounded growth in long-running nodes.
    pub fn prune_old_checkpoints(&mut self) {
        let keep_from = self.last_finalized_epoch.saturating_sub(1);
        self.justified_checkpoints.retain(|(epoch, _, _)| *epoch >= keep_from);
        self.justified.retain(|e| *e >= keep_from);
    }
}

impl Default for FinalityTracker {
    fn default() -> Self {
        Self::new()
    }
}
