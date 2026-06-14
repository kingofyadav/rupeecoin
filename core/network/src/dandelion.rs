/// Dandelion++ transaction propagation protocol.
///
/// Whitepaper §13.6.1:
///   Stem phase  — forward the transaction to a *single* randomly chosen peer
///                 for N hops, where N ∈ [STEM_MIN_HOPS, STEM_MAX_HOPS].
///   Fluff phase — standard gossipsub broadcast to all peers.
///
/// Implementation invariant: every node that participates in a stem chain
/// tracks the transaction with a timeout.  If the tx is not seen via gossipsub
/// (fluffed by some downstream node) within STEM_TIMEOUT, this node fluffs it
/// directly, preventing silent drops.
use std::collections::{HashMap, HashSet};
use std::io;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures::{AsyncRead, AsyncWrite};
use libp2p::request_response;
use libp2p::{PeerId, StreamProtocol};
use rand::Rng;
use serde::{Deserialize, Serialize};

use rupeecoin_crypto::Hash;

// ── Dandelion++ parameters ────────────────────────────────────────────────────

/// Minimum number of stem hops before fluffing (inclusive).
pub const STEM_MIN_HOPS: u8 = 2;
/// Maximum number of stem hops before fluffing (inclusive).
pub const STEM_MAX_HOPS: u8 = 10;
/// A stem transaction that has not been fluffed within this window is fluffed
/// by the tracking node as a safety fallback.
pub const STEM_TIMEOUT: Duration = Duration::from_secs(10);

// ── Wire types ────────────────────────────────────────────────────────────────

/// A stem-phase forwarding message: the transaction bytes plus the number of
/// additional hops remaining.  When `remaining_hops` reaches zero the
/// receiving node transitions to fluff (gossipsub broadcast).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DandelionStemReq {
    pub tx_bytes: Vec<u8>,
    /// Additional stem hops remaining.  Receiving node fluffs when this is 0.
    pub remaining_hops: u8,
}

/// Empty acknowledgement returned to the stem sender.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DandelionStemAck;

// ── Codec ─────────────────────────────────────────────────────────────────────

/// Length-prefixed bincode codec for the Dandelion++ stem protocol.
#[derive(Clone, Default)]
pub struct StemCodec;

#[async_trait]
impl request_response::Codec for StemCodec {
    type Protocol = StreamProtocol;
    type Request = DandelionStemReq;
    type Response = DandelionStemAck;

    async fn read_request<T>(
        &mut self,
        _: &StreamProtocol,
        io: &mut T,
    ) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        use futures::AsyncReadExt;
        let mut len = [0u8; 4];
        io.read_exact(&mut len).await?;
        let mut buf = vec![0u8; u32::from_be_bytes(len) as usize];
        io.read_exact(&mut buf).await?;
        bincode::deserialize(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    async fn write_request<T>(
        &mut self,
        _: &StreamProtocol,
        io: &mut T,
        req: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        use futures::AsyncWriteExt;
        let bytes =
            bincode::serialize(&req).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        io.write_all(&(bytes.len() as u32).to_be_bytes()).await?;
        io.write_all(&bytes).await
    }

    async fn read_response<T>(
        &mut self,
        _: &StreamProtocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        use futures::AsyncReadExt;
        let mut sentinel = [0u8; 1];
        io.read_exact(&mut sentinel).await?;
        Ok(DandelionStemAck)
    }

    async fn write_response<T>(
        &mut self,
        _: &StreamProtocol,
        io: &mut T,
        _: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        use futures::AsyncWriteExt;
        io.write_all(&[1u8]).await
    }
}

// ── DandelionRouter ───────────────────────────────────────────────────────────

struct StemEntry {
    tx_bytes: Vec<u8>,
    entered_at: Instant,
}

/// Per-node Dandelion++ routing state.
pub struct DandelionRouter {
    /// txid → (tx_bytes, stem_entry_time).
    pending: HashMap<Hash, StemEntry>,
}

impl DandelionRouter {
    pub fn new() -> Self {
        DandelionRouter {
            pending: HashMap::new(),
        }
    }

    /// Sample an initial hop count for a freshly originated transaction.
    pub fn initial_hops<R: Rng>(rng: &mut R) -> u8 {
        rng.gen_range(STEM_MIN_HOPS..=STEM_MAX_HOPS)
    }

    /// Record a transaction entering the stem phase (either originated locally
    /// or received from a peer via the stem protocol).  Idempotent: calling
    /// this a second time for the same txid has no effect.
    pub fn enter_stem(&mut self, txid: Hash, tx_bytes: Vec<u8>) {
        self.pending.entry(txid).or_insert(StemEntry {
            tx_bytes,
            entered_at: Instant::now(),
        });
    }

    /// Remove a transaction from tracking (called when it arrives via gossipsub,
    /// meaning a downstream node already fluffed it).
    pub fn remove(&mut self, txid: &Hash) {
        self.pending.remove(txid);
    }

    #[cfg(test)]
    pub fn is_pending(&self, txid: &Hash) -> bool {
        self.pending.contains_key(txid)
    }

    #[cfg(test)]
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Drain all entries whose stem timeout has elapsed.  The caller must
    /// publish each returned `(txid, tx_bytes)` via gossipsub (fluff fallback).
    pub fn drain_timed_out(&mut self) -> Vec<(Hash, Vec<u8>)> {
        let now = Instant::now();
        let expired: Vec<Hash> = self
            .pending
            .iter()
            .filter(|(_, e)| now.duration_since(e.entered_at) >= STEM_TIMEOUT)
            .map(|(id, _)| *id)
            .collect();
        expired
            .into_iter()
            .filter_map(|id| {
                self.pending
                    .remove(&id)
                    .map(|e| (id, e.tx_bytes))
            })
            .collect()
    }

}

// ── Peer selection ────────────────────────────────────────────────────────────

/// Choose a random peer from `peers`, optionally excluding one (the peer we
/// received a stem message from, so we don't forward back to sender).
///
/// Returns `None` if no eligible peer is available.
pub fn pick_stem_peer<R: Rng>(
    rng: &mut R,
    peers: &HashSet<PeerId>,
    exclude: Option<&PeerId>,
) -> Option<PeerId> {
    let candidates: Vec<PeerId> = peers
        .iter()
        .filter(|p| exclude.map_or(true, |e| *p != e))
        .copied()
        .collect();
    if candidates.is_empty() {
        return None;
    }
    Some(candidates[rng.gen_range(0..candidates.len())])
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::SmallRng;
    use rand::SeedableRng;

    fn fake_hash(byte: u8) -> Hash {
        Hash::from_bytes([byte; 32])
    }

    #[test]
    fn initial_hops_in_range() {
        let mut rng = SmallRng::seed_from_u64(42);
        for _ in 0..1_000 {
            let h = DandelionRouter::initial_hops(&mut rng);
            assert!(
                h >= STEM_MIN_HOPS && h <= STEM_MAX_HOPS,
                "hop count {h} out of range [{STEM_MIN_HOPS}, {STEM_MAX_HOPS}]"
            );
        }
    }

    #[test]
    fn initial_hops_distribution_covers_range() {
        let mut rng = SmallRng::seed_from_u64(7);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..10_000 {
            seen.insert(DandelionRouter::initial_hops(&mut rng));
        }
        // Every value in [STEM_MIN_HOPS, STEM_MAX_HOPS] should appear.
        for h in STEM_MIN_HOPS..=STEM_MAX_HOPS {
            assert!(seen.contains(&h), "hop count {h} never seen in 10 000 samples");
        }
    }

    #[test]
    fn enter_stem_and_is_pending() {
        let mut router = DandelionRouter::new();
        let txid = fake_hash(1);
        assert!(!router.is_pending(&txid));
        router.enter_stem(txid, vec![0xde, 0xad]);
        assert!(router.is_pending(&txid));
        assert_eq!(router.pending_count(), 1);
    }

    #[test]
    fn enter_stem_is_idempotent() {
        let mut router = DandelionRouter::new();
        let txid = fake_hash(2);
        router.enter_stem(txid, vec![1, 2, 3]);
        router.enter_stem(txid, vec![9, 9, 9]); // second call should not overwrite
        assert_eq!(router.pending_count(), 1);
    }

    #[test]
    fn remove_clears_entry() {
        let mut router = DandelionRouter::new();
        let txid = fake_hash(3);
        router.enter_stem(txid, vec![1]);
        router.remove(&txid);
        assert!(!router.is_pending(&txid));
        assert_eq!(router.pending_count(), 0);
    }

    #[test]
    fn new_entries_not_timed_out() {
        let mut router = DandelionRouter::new();
        for i in 0u8..5 {
            router.enter_stem(fake_hash(i), vec![i]);
        }
        // Entries were just added — none should be timed out yet.
        let drained = router.drain_timed_out();
        assert!(
            drained.is_empty(),
            "fresh entries should not time out immediately"
        );
        assert_eq!(router.pending_count(), 5);
    }

    #[test]
    fn pick_stem_peer_empty_set() {
        let mut rng = SmallRng::seed_from_u64(0);
        let peers = HashSet::new();
        assert!(pick_stem_peer(&mut rng, &peers, None).is_none());
    }

    #[test]
    fn pick_stem_peer_single_peer_no_exclusion() {
        let mut rng = SmallRng::seed_from_u64(0);
        let peer = PeerId::random();
        let peers: HashSet<PeerId> = std::iter::once(peer).collect();
        assert_eq!(pick_stem_peer(&mut rng, &peers, None), Some(peer));
    }

    #[test]
    fn pick_stem_peer_excludes_sender() {
        let mut rng = SmallRng::seed_from_u64(0);
        let sender = PeerId::random();
        let other = PeerId::random();
        let peers: HashSet<PeerId> = [sender, other].into_iter().collect();
        // With sender excluded, we must always get `other`.
        for _ in 0..50 {
            let selected = pick_stem_peer(&mut rng, &peers, Some(&sender));
            assert_eq!(selected, Some(other));
        }
    }

    #[test]
    fn pick_stem_peer_only_sender_returns_none() {
        let mut rng = SmallRng::seed_from_u64(0);
        let sender = PeerId::random();
        let peers: HashSet<PeerId> = std::iter::once(sender).collect();
        assert!(pick_stem_peer(&mut rng, &peers, Some(&sender)).is_none());
    }

    #[test]
    fn pick_stem_peer_uniform_distribution() {
        let mut rng = SmallRng::seed_from_u64(99);
        let peers: HashSet<PeerId> = (0..4).map(|_| PeerId::random()).collect();
        let mut counts: HashMap<PeerId, usize> = peers.iter().map(|&p| (p, 0)).collect();
        let n = 4_000;
        for _ in 0..n {
            let p = pick_stem_peer(&mut rng, &peers, None).unwrap();
            *counts.get_mut(&p).unwrap() += 1;
        }
        // Each peer should be chosen roughly n/4 = 1000 times.
        // Allow ±30% tolerance.
        let expected = n / peers.len();
        for (peer, count) in &counts {
            let lo = expected * 70 / 100;
            let hi = expected * 130 / 100;
            assert!(
                *count >= lo && *count <= hi,
                "peer {peer} count {count} outside [{lo}, {hi}]"
            );
        }
    }
}
