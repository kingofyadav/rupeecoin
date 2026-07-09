# ADR 0001 — VRF uniqueness for proposer selection

- **Status:** Accepted — direction: **Option D (RANDAO commit–reveal beacon)**; implementation tracked as the VRF-hardening milestone (pre-mainnet blocker)
- **Date:** 2026-07-09
- **Components:** `core/crypto/src/vrf.rs`, `core/consensus/src/lib.rs` (`select_proposer`, `verify_block_vrf`), `core/blockchain/src/lib.rs` (`build_block`, `validate`)

## Context

Proposer election chains off a per-block VRF: the proposer for slot `N+1` is
`select_proposer(N+1, vrf_output_N, active_count)`, and `vrf_output_N` is derived
from the slot-`N` proposer's **Dilithium3 signature** over the VRF input
(`vrf_output(sig, slot, prev)` = `BLAKE3("…" ‖ sig ‖ prev ‖ slot)`).

A secure VRF must be **unique**: for a fixed `(public_key, input)` exactly one
output is verifier-acceptable. **Dilithium3 signatures are not unique** — many
distinct signatures verify for the same message and key. Verifiers here accept
*any* valid signature (`pubkey.verify(msg, sig)`), so the output is not pinned by
the key and input alone.

### Impact

A malicious slot-`N` proposer can locally generate many valid signatures over the
fixed VRF input (each Dilithium sign is sub-millisecond; a 4 s slot budgets
thousands), compute each candidate `vrf_output_N`, and **choose the one that makes
`select_proposer(N+1)` land on themselves or a colluder**. Iterated, this is
stake-grinding on proposer election: disproportionate block production, reward
capture, and — with enough grinding leverage over a modest active set — a path to
chain control. It also weakens any downstream use of `vrf_output` as randomness.

This is a consensus-security defect, not a liveness bug, and is a **mainnet
blocker**.

### What is *not* broken

Honest nodes agree on the output because PQClean Dilithium3 signs
deterministically. This is asserted by the regression test
`vrf::tests::dilithium3_signing_is_deterministic` (added with this ADR). The guard
protects only the honest path; it does nothing against a signer who deliberately
randomises. If a future dependency bump enables randomised signing, even honest
proposers would disagree on `vrf_output` and the chain would fork — the test
fails loudly if that day comes.

## Options considered

| Option | Unique? | Post-quantum? | Verdict |
|--------|---------|---------------|---------|
| **A. Keep signature-derived VRF** (status quo) | ❌ | ✅ | Reject — grindable |
| **B. Deterministic Dilithium only** | ❌ (still many valid sigs) | ✅ | Reject — determinism ≠ uniqueness; a malicious signer still grinds |
| **C. ECVRF (RFC 9381, ristretto255)** | ✅ | ❌ | Reject for a PQ chain — reintroduces a classical assumption into consensus randomness; a quantum adversary could forge and bias election |
| **D. Commit–reveal RANDAO beacon + non-reveal slashing, VRF as liveness fallback** | ✅ (of the beacon) | ✅ | **Recommended** — no unique-signature dependency; residual last-revealer bias closed by penalties and/or a VDF |
| **E. Standardised PQ-VRF** | ✅ | ✅ | Defer — none standardised/audited yet; revisit when available |

## Decision

Adopt **Option D** (accepted 2026-07-09). Concretely, before mainnet:

1. **Epoch randomness = commit–reveal beacon.** Each active validator commits
   `H(r_i)` in epoch `e` and reveals `r_i` in epoch `e+1`; the epoch seed is
   `BLAKE3(‖ sorted reveals)`. `select_proposer` consumes the **epoch seed**, not a
   proposer's fresh per-block signature output.
2. **Slash non-reveal / late-reveal** so withholding to bias the seed is costly;
   this reduces last-revealer grinding to a single bit of influence at bounded
   economic cost.
3. **Optionally chain a VDF** over the combined seed to remove even the last-bit
   grinding advantage (defer if the penalty in (2) is judged sufficient).
4. **Retain the current VRF only as an intra-epoch liveness fallback** (e.g. when a
   reveal set is unavailable), never as the sole election randomness.

### Interim mitigations shipped with this ADR (no consensus change)

- Regression guard `dilithium3_signing_is_deterministic` (fails if the honest path
  ever becomes non-deterministic).
- Prominent `⚠` warning in `core/crypto/src/vrf.rs` module docs pointing here.

### Cheap pre-mainnet hardening (optional, if D slips)

Mix election entropy the proposer cannot freely regrind — e.g. seed
`select_proposer` with `BLAKE3(prev_block_hash ‖ vrf_output ‖ epoch)` where
`prev_block_hash` is fixed once the parent is final. This does not achieve
uniqueness but raises grinding cost and shrinks the attacker's control surface.

## Consequences

- **Positive:** PQ-safe, uniqueness-secure election randomness; grinding closed.
- **Negative:** Commit–reveal adds an epoch-scoped message flow, state, and new
  slashing conditions; more consensus surface to test.
- **Follow-up work:** beacon message types + gossip, commit/reveal validation,
  non-reveal slashing evidence, `select_proposer` reseeding, and migration/
  activation logic. Track as the VRF-hardening milestone.
