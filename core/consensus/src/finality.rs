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

    /// Mark `epoch` as justified (called by ConsensusState when 2/3 attesting
    /// stake is reached).  Returns `true` if this was a new justification.
    pub fn justify(&mut self, epoch: u64, slot: u64, block_hash: Hash) -> bool {
        if self.justified.contains(&epoch) {
            return false;
        }
        self.justified.insert(epoch);
        self.justified_checkpoints.push((epoch, slot, block_hash));
        if epoch > self.last_justified_epoch {
            self.last_justified_epoch = epoch;
        }

        // Finality rule: if epoch and epoch-1 are both justified, finalize epoch-1.
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
}

impl Default for FinalityTracker {
    fn default() -> Self {
        Self::new()
    }
}
