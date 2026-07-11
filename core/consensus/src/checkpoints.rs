//! Weak subjectivity checkpoints.
//!
//! A checkpoint is a `(height, block_hash, state_root)` triple for a finalized
//! block.  New nodes bootstrap by accepting a trusted checkpoint from a peer or
//! out-of-band, then syncing forward from that point without re-validating
//! everything before it.
//!
//! Checkpoints are saved automatically every `CHECKPOINT_INTERVAL` blocks.
use rupeecoin_crypto::Hash;
use serde::{Deserialize, Serialize};

/// 28 days at 4 s/block = 604 800 blocks between checkpoints.
pub const CHECKPOINT_INTERVAL: u64 = 604_800;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeakSubjectivityCheckpoint {
    pub height: u64,
    pub block_hash: Hash,
    pub state_root: Hash,
}

/// Manages trusted checkpoints for a running node.
pub struct CheckpointManager {
    /// Trusted checkpoints (from config or peer exchange), sorted by height.
    trusted: Vec<WeakSubjectivityCheckpoint>,
}

impl CheckpointManager {
    pub fn new() -> Self {
        CheckpointManager {
            trusted: Vec::new(),
        }
    }

    /// Add a trusted checkpoint (from `--checkpoint` flag or peer exchange).
    pub fn add_trusted(&mut self, cp: WeakSubjectivityCheckpoint) {
        // Keep sorted by height for quick lookup.
        let pos = self.trusted.partition_point(|c| c.height < cp.height);
        if self
            .trusted
            .get(pos)
            .map(|c| c.height == cp.height)
            .unwrap_or(false)
        {
            return; // already have a checkpoint at this height
        }
        self.trusted.insert(pos, cp);
    }

    /// The latest trusted checkpoint, if any.
    pub fn latest(&self) -> Option<&WeakSubjectivityCheckpoint> {
        self.trusted.last()
    }

    /// Returns `true` if `height` is strictly before the latest trusted checkpoint,
    /// meaning full block validation can be skipped for that height.
    pub fn is_before_checkpoint(&self, height: u64) -> bool {
        self.trusted
            .last()
            .map(|cp| height < cp.height)
            .unwrap_or(false)
    }

    /// Verify that a block at `height` matches the trusted checkpoint.
    ///
    /// Returns `true` if:
    ///   - No checkpoint covers `height` (nothing to verify), or
    ///   - `height` is strictly below the checkpoint (already trusted), or
    ///   - `height` equals the checkpoint height and both `block_hash` and
    ///     `state_root` match.
    ///
    /// Returns `false` if `height` equals a checkpoint height but the hashes
    /// do not match — indicating a chain fork or a tampered checkpoint.
    pub fn verify_checkpoint_block(
        &self,
        height: u64,
        block_hash: &Hash,
        state_root: &Hash,
    ) -> bool {
        let Some(cp) = self.trusted.iter().find(|c| c.height == height) else {
            return true;
        };
        &cp.block_hash == block_hash && &cp.state_root == state_root
    }

    /// Whether a checkpoint should be created at `height`.
    pub fn should_checkpoint(height: u64) -> bool {
        height > 0 && height.is_multiple_of(CHECKPOINT_INTERVAL)
    }
}

impl Default for CheckpointManager {
    fn default() -> Self {
        Self::new()
    }
}
