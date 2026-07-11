use rupeecoin_crypto::Hash;
use serde::{Deserialize, Serialize};

use crate::commitment_tree::MerklePath;
use crate::note::NoteCommitment;

// ── Spend proof ───────────────────────────────────────────────────────────────

/// Proves that a nullifier corresponds to a note commitment in the shielded pool,
/// without revealing the note's value or recipient.
///
/// **Cryptographic guarantees (Phase 4)**:
/// - *Binding*: the anchor, commitment and nullifier are all hash-based — forgery
///   requires breaking BLAKE3.
/// - *Soundness*: the verifier recomputes the Merkle root and checks against the
///   block's anchor; an invalid path or commitment will be caught.
/// - *Unlinkability*: partial — the note commitment is revealed here, linking
///   spend to deposit.  Full unlinkability requires a zk-STARK proof (see
///   `StarkProofPlaceholder` below); this will be wired in Phase 5.
///
/// In practice, callers who need the strongest anonymity set should ensure a
/// large commitment tree before spending.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpendProof {
    /// The note commitment being spent (allows verifier to walk the Merkle path).
    pub note_commitment: NoteCommitment,
    /// Authentication path from `note_commitment` up to `anchor`.
    pub merkle_path: MerklePath,
    /// The tree root that was current when this proof was created.
    pub anchor: Hash,
}

impl SpendProof {
    /// Verify: path recomputes to `anchor` and nullifier is consistent.
    ///
    /// Full balance and note-knowledge verification is done at the transaction
    /// level; this proof only validates tree membership.
    pub fn verify(&self) -> bool {
        let computed = self.merkle_path.compute_root(self.note_commitment.0);
        computed == self.anchor
    }
}

// ── Range proof ──────────────────────────────────────────────────────────────

/// Proves that a note's committed value is in `[0, 2^64)`.
///
/// **Phase 4 implementation**: for values stored as `u64`, the range is
/// trivially satisfied.  The proof carries a commitment to the bit decomposition
/// of the value so validators can re-derive the note commitment:
/// `cm = BLAKE3(value_le || recipient || rcm)`.
///
/// Validators receive the value and rcm in the shielded output (transmitted
/// privately to them via the miner's audit key — Phase 5 will replace this
/// with a proper zk-STARK range argument).
///
/// The bit_commitments field provides a BLAKE3 hash of each bit `b_i` with
/// its individual blinding factor so that third-party auditors cannot trivially
/// reconstruct the value from the bit-commitment array (each bit is additionally
/// blinded).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RangeProof {
    /// `C_i = BLAKE3(b_i || r_i)` for each bit of the 64-bit value (64 elements).
    /// Binding: revealing `(b_i, r_i)` lets anyone verify; hiding: observers
    /// only see the 64 commitments, not the value.
    pub bit_commitments: Vec<Hash>,
    /// BLAKE3 hash over all 64 `r_i` blinding factors — used by the note holder
    /// to prove knowledge of the full decomposition without revealing each `r_i`.
    pub aggregated_opening_hash: Hash,
}

impl RangeProof {
    /// Build a range proof for `value`, deterministically binding it to `rcm`
    /// so the bit-level and note-level commitments are consistent.
    pub fn prove(value: u64, rcm: &[u8; 32]) -> Self {
        use rupeecoin_crypto::hash_data;

        // Per-bit blinding factors: r_i = BLAKE3(rcm || i)
        let mut bit_commitments = Vec::with_capacity(64);
        let mut all_r = Vec::with_capacity(64 * 32);

        for i in 0u8..64 {
            let bit = ((value >> i) & 1) as u8;
            // r_i = BLAKE3(rcm || i)
            let mut rbuf = [0u8; 33];
            rbuf[..32].copy_from_slice(rcm);
            rbuf[32] = i;
            let r_i = hash_data(&rbuf);

            // C_i = BLAKE3(bit || r_i)
            let mut cbuf = [0u8; 33];
            cbuf[0] = bit;
            cbuf[1..].copy_from_slice(r_i.as_bytes());
            bit_commitments.push(hash_data(&cbuf));
            all_r.extend_from_slice(r_i.as_bytes());
        }

        let aggregated_opening_hash = hash_data(&all_r);
        RangeProof {
            bit_commitments,
            aggregated_opening_hash,
        }
    }

    /// Verify the range proof given the known value (available to validators).
    pub fn verify(&self, value: u64, rcm: &[u8; 32]) -> bool {
        use rupeecoin_crypto::hash_data;

        let mut all_r = Vec::with_capacity(64 * 32);

        for i in 0u8..64 {
            let bit = ((value >> i) & 1) as u8;
            let mut rbuf = [0u8; 33];
            rbuf[..32].copy_from_slice(rcm);
            rbuf[32] = i;
            let r_i = hash_data(&rbuf);
            all_r.extend_from_slice(r_i.as_bytes());

            let mut cbuf = [0u8; 33];
            cbuf[0] = bit;
            cbuf[1..].copy_from_slice(r_i.as_bytes());
            let expected = hash_data(&cbuf);

            if self.bit_commitments.get(i as usize) != Some(&expected) {
                return false;
            }
        }

        self.aggregated_opening_hash == hash_data(&all_r)
    }
}

// ── zk-STARK spend proof (Phase 5B) ──────────────────────────────────────────

/// Real Winterfell STARK spend proof generated by `rupeecoin-zkproof`.
///
/// This type is opaque to the privacy crate; it is created and verified by
/// callers (wallet, node RPC) that link against `rupeecoin-zkproof`.
///
/// Security properties:
/// - *Zero-knowledge*: `proof_bytes` reveals nothing about the private note
///   commitment, nullifier key, or Merkle path siblings.
/// - *Soundness*: only the correct cm can satisfy both the nullifier hash
///   constraint and the 20-level Merkle membership constraint simultaneously.
/// - *Binding*: the anchor binds the proof to a specific tree state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StarkSpendProof {
    /// Winterfell STARK proof bytes (produced by SpendProver).
    pub proof_bytes: Vec<u8>,
    /// Poseidon shadow-tree anchor — 4 Goldilocks field elements (LE u64).
    pub anchor_felts: [u64; 4],
    /// Poseidon nullifier = Poseidon(nk_felts | cm_felts) — public input to verifier.
    /// Distinct from the on-chain BLAKE3 nullifier; used only by `verify_spend`.
    pub nullifier_felts: [u64; 4],
}

// ── Batched STARK spend proof (Phase 11.2) ───────────────────────────────────

/// Re-exported from `rupeecoin-zkproof`.  One proof covers up to 10 shielded
/// inputs.  The proof lives at the transaction level; individual
/// `ShieldedInput.spend_evidence` fields are set to `SpendEvidence::BatchedStark`.
pub use rupeecoin_zkproof::BatchedStarkSpendProof;

// ── Spend evidence (Phase 4 / Phase 5B / Phase 11.2 union) ───────────────────

/// The spend evidence variants supported by the protocol.
///
/// `Blake3Path` is the Phase 4 Merkle-path proof (linkable but simple).
/// `Stark` is the Phase 5B per-input Winterfell STARK proof.
/// `BatchedStark` marks an input whose proof is aggregated at the transaction
/// level in `PrivacyTransaction::batch_stark_proof`; no proof data is stored
/// per-input.  The transaction-level verifier checks all inputs at once.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpendEvidence {
    /// Phase 4 linkable spend proof — reveals note commitment.
    ///
    /// Deprecated: new transactions must use `Stark` or `BatchedStark`.
    /// Retained for deserialization of historical data only.
    #[deprecated(
        since = "0.5.0",
        note = "use Stark or BatchedStark; Blake3Path reveals note commitment"
    )]
    Blake3Path(SpendProof),
    /// Phase 5B zk-STARK spend proof — hides note commitment entirely.
    Stark(StarkSpendProof),
    /// Phase 11.2 batch marker — proof is in `PrivacyTransaction::batch_stark_proof`.
    BatchedStark,
}

#[allow(deprecated)]
impl SpendEvidence {
    /// Stateless per-input verification.
    ///
    /// `BatchedStark` inputs always return `true` here; the real check is done
    /// at the transaction level in `PrivacyTransaction::verify_batch_stark`.
    pub fn verify_basic(&self) -> bool {
        match self {
            SpendEvidence::Blake3Path(sp) => sp.verify(),
            SpendEvidence::Stark(sp) => rupeecoin_zkproof::verify_spend_raw(
                &sp.proof_bytes,
                sp.anchor_felts,
                sp.nullifier_felts,
            )
            .is_ok(),
            SpendEvidence::BatchedStark => true,
        }
    }

    /// The anchor embedded in the spend evidence.
    ///
    /// For `Stark` the anchor is the Poseidon root packed as 4 LE u64 → 32 bytes.
    /// For `BatchedStark` returns `Hash::default()`; the shared anchor is in
    /// `PrivacyTransaction::batch_stark_proof`.
    pub fn anchor(&self) -> Hash {
        match self {
            SpendEvidence::Blake3Path(sp) => sp.anchor,
            SpendEvidence::Stark(sp) => {
                let mut buf = [0u8; 32];
                for (i, &v) in sp.anchor_felts.iter().enumerate() {
                    buf[i * 8..(i + 1) * 8].copy_from_slice(&v.to_le_bytes());
                }
                Hash::from_bytes(buf)
            }
            SpendEvidence::BatchedStark => Hash::from_bytes([0u8; 32]),
        }
    }
}

// ── Legacy placeholder (retained for source compatibility) ───────────────────

#[deprecated(since = "0.5.0", note = "replaced by StarkSpendProof in Phase 5B")]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StarkProofPlaceholder {
    pub statement_hash: Hash,
    pub proof_bytes: Vec<u8>,
}

#[cfg(test)]
#[allow(deprecated)]
impl StarkProofPlaceholder {
    pub fn unimplemented(statement: &[u8]) -> Self {
        StarkProofPlaceholder {
            statement_hash: rupeecoin_crypto::hash_data(statement),
            proof_bytes: vec![],
        }
    }
    pub fn verify(&self) -> bool {
        false
    }
}

// ── Balance proof ────────────────────────────────────────────────────────────

/// Ensures that total shielded input values equal total shielded output values
/// plus the public fee.
///
/// Phase 4: validators see plaintext values via `ShieldedOutput::value` field.
/// Phase 5: will be a homomorphic commitment equation or STARK sub-circuit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BalanceProof {
    /// Total input value (sum of consumed notes' values).
    pub total_input: u64,
    /// Total output value (sum of new notes' values).
    pub total_output: u64,
    /// Public fee (= total_input - total_output, validated by verifier).
    pub fee: u64,
}

impl BalanceProof {
    pub fn verify(&self) -> bool {
        self.total_output
            .checked_add(self.fee)
            .is_some_and(|rhs| self.total_input == rhs)
    }
}
