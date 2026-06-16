# RupeeCoin — Development Roadmap

> Last updated: 2026-06-16  
> Current state: **Phases 0–14 complete.** All seven security-checklist items resolved. Mainnet-ready codebase.  
> Next milestone: Phase 15 — Ecosystem bootstrapping (3 engineering carry-overs + public validator onboarding)

---

## Where We Are Right Now

| Phase | Name | Status |
|---|---|---|
| 0 | Whitepaper (16 sections) | ✅ Complete |
| 1 | Research Prototype | ✅ Complete |
| 2 | Alpha Network (libp2p, RocksDB, gossipsub) | ✅ Complete |
| 3 | Validator Framework (Casper FFG, VRF, staking) | ✅ Complete |
| 4 | Privacy Layer (Kyber768, commitments, nullifiers) | ✅ Complete |
| 5 | Wallet CLI + Node RPC | ✅ Complete |
| 6 | Economic System (EIP-1559, PMFP, inactivity leak) | ✅ Complete |
| 7 | Transaction Hardening (chain_id, nonce, expiry, state_root) | ✅ Complete |
| — | Bug Fixes (10 critical/high/medium bugs) | ✅ Complete 2026-06-14 |
| 8 | Protocol Hardening (STARK, rewards, slashing, LMD-GHOST) | ✅ Complete 2026-06-14 |
| 9.1 | Block Explorer (axum REST + SPA) | ✅ Complete 2026-06-14 |
| 9.2 | Validator Dashboard (ratatui TUI) | ✅ Complete 2026-06-14 |
| 9.3 | JavaScript / TypeScript SDK | ✅ Complete 2026-06-14 |
| 9.4 | Python SDK (PyO3 + maturin) | ✅ Complete 2026-06-14 |
| 9.5 | Testnet Faucet (axum + rate limit + SPA) | ✅ Complete 2026-06-15 |
| 9.6 | Benchmarking Suite (Criterion, 4 bench groups) | ✅ Complete 2026-06-15 |
| 10.1 | Desktop Wallet GUI (Tauri 2 + dark-mode SPA) | ✅ Complete 2026-06-15 |
| 10.2 | Mobile Wallet Alpha (Flutter + FRB 2 + Android) | ✅ Complete 2026-06-15 |
| 11.1 | Poseidon Commitment Tree (Goldilocks field, 2²⁰ leaves) | ✅ Complete 2026-06-15 |
| 11.2 | Batched STARK Verification (≤10 shielded inputs per proof) | ✅ Complete 2026-06-15 |
| 11.3 | Incremental Merkle Root (eliminates O(N log N) bottleneck) | ✅ Complete 2026-06-16 |
| 11.4 | Mobile Prover (arm64 native + WASM-compatible) | ✅ Complete 2026-06-16 |
| 11.5 | Output Well-Formedness (STARK statement 4) | ✅ Complete 2026-06-16 |
| 12 | RupeeCoin VM (64-bit stack VM, RPC-20/721/1155, EVM transpiler) | ✅ Complete 2026-06-16 |
| 13 | Mainnet Preparation (genesis config, Tor/SOCKS5, hardware wallet, SDK v1.0) | ✅ Complete 2026-06-16 |
| 14 | Mainnet Launch Hardening (all 7 security checklist items resolved) | ✅ Complete 2026-06-16 |
| **15** | **Ecosystem Growth** | **🔲 Engineering carry-overs: ✅ Complete** |

**Current test count:** 37 test suites, 0 failures, 4 Criterion benchmark groups (network suite grew 15 → 19 tests).

---

## What Remains Stubbed / Placeholder

All three Phase 14 carry-overs have been resolved in Phase 15.

| Item | Resolution |
|---|---|
| Trezor HID (W-05b) | ✅ Wire Protocol v1 enumeration + Initialize handshake; signing awaits custom Dilithium3 firmware (Trezor partner program) |
| `CF_CONTRACT_CODE` (VM-07b) | ✅ 10th RocksDB CF added; bytecode persisted on deploy, loaded on ContractCall |
| Gossipsub allowlist (N-04) | ✅ `--peer-allowlist <file>` flag; `is_allowlisted()` in `on_event`; 4 unit tests |

Remaining known limitations (accepted design or future phases):
- Trezor Dilithium3 signing: requires custom firmware (Trezor partner program, no ETA)
- Privacy commitment tree not rolled back on reorg > REORG_DEPTH (append-only by design)
- No light-client / SPV proof protocol

---

## Phase 14 — Mainnet Launch Hardening

**Complete as of 2026-06-16.** All items from `audit/SECURITY_CHECKLIST.md` resolved.

| # | Security Item | Implementation |
|---|---|---|
| CS-02 | LMD-GHOST reorg | `chain.rollback(count)` + `chain.apply_fork(blocks, registry)`; `REORG_DEPTH = 32` |
| T-06 | Coinbase UTXO 100-block maturity | `COINBASE_MATURITY = 100`; enforced in `UtxoSet::validate_transfer` |
| N-03 | Per-peer gossipsub rate limiting | Token bucket: 100 msgs / 10 s window, applied in `network::on_event` |
| VM-07 | Contract storage persistence | `CF_CONTRACT_STORAGE` — 9th RocksDB CF; 16-byte key `addr_u64_le ++ slot_u64_le` |
| G-02 | Configurable genesis timestamp | `--genesis-timestamp <unix_secs>` flag; `genesis_block_with_timestamp(ts)` |
| G-03 | Configurable treasury address | `--treasury-address <40-hex>` flag; module-level `OnceLock` override in `rewards.rs` |
| W-05 | Ledger hardware wallet | `LedgerHardwareWallet` via Linux hidraw ioctl + APDU-over-HID; CLA=0xA5 |
| N-02 | Tor SOCKS5 transport | `SocksTransport` implements libp2p `Transport`; `--tor-proxy` flag; noise+yamux stacked |

**Key architectural decisions made in Phase 14:**
- Reorg uses delta-based UTXO rollback (`BlockDelta`). The Poseidon commitment tree and nullifier set are **not** rolled back — they are append-only by design. A reorg deeper than `REORG_DEPTH = 32` requires a privacy state resync. This is an accepted Phase 14 trade-off.
- `SocksTransport::from_option(None)` is always wired into `SwarmBuilder::with_other_transport()`. When disabled, it declines all dials with `MultiaddrNotSupported`, keeping the code path unified.
- `CF_CONTRACT_STORAGE` key = `addr_u64_le(8B) ++ slot_u64_le(8B)`, value = `u64_le(8B)`. `CF_CONTRACT_CODE` (bytecode) is a Phase 15 carry-over.

---

## Phase 15 — Ecosystem Growth

**Goal:** Open the network to external validators, close the three engineering carry-overs, and launch the developer ecosystem.

### 15.1 Engineering Carry-overs (from security checklist) — ✅ ALL COMPLETE

#### N-04 — Validator allow-list for gossipsub peers ✅

**Implemented 2026-06-16.** `core/network/src/lib.rs`: `is_allowlisted()` helper; `allowlist: Option<HashSet<PeerId>>` parameter through `start()` → `run()` → `on_event()`; allowlist check before rate-limit drops non-listed peers with `trace!()`. 4 unit tests in `allowlist_tests`. `node/src/main.rs`: `--peer-allowlist <file>` CLI flag reads one PeerId per line (supports `#` comments).

---

#### W-05b — Trezor HID Wire Protocol v1 ✅

**Implemented 2026-06-16.** `wallet/src/hw_trezor.rs`: HID enumeration over `/dev/hidraw0..15` for VID 0x534C (Model One) and VID 0x1209 + PID {0x53C1, 0x53C3, 0x53C5} (T/Safe 3/Safe 5). `trezor_write()` / `trezor_read()` implement Wire Protocol v1 framing (`?##` magic + msg_type + msg_len). `open()` sends Initialize (msg_type=0) and validates Features response (msg_type=17). Signing returns an actionable error about the missing Dilithium3 firmware app. `wallet/src/main.rs`: `--hardware-wallet ledger|trezor` replaces the old boolean `--hardware-wallet` flag.

---

#### VM-07b — Contract bytecode persistence (`CF_CONTRACT_CODE`) ✅

**Implemented 2026-06-16.** `core/storage/src/lib.rs`: `CF_CONTRACT_CODE` is the 10th RocksDB column family; `save_contract_code(addr, bytecode)` / `load_contract_code(addr)` with key = `addr_u64_le(8B)`. `node/src/main.rs`: deploy path now calls `store.save_contract_code()` after a successful constructor run; ContractCall path loads bytecode from DB, builds `ExecContext`, calls `run()`, and saves the resulting storage.

---

### 15.2 Public Validator Onboarding

- Written guide: how to join the Zeta testnet (`--stake 10000 --rpc-port 9944`)
- Publish bootnode multiaddrs and latest checkpoint hash
- Target: 100 external validators running continuously for 30 days
- Deliverable: `docs/testnet-validator.md`

---

### 15.3 External Security Audit #1

- **Scope:** `core/consensus`, `core/staking`, `core/blockchain`, `core/utxo`
- **Firms:** Minimum 2 independent external security firms
- **Gate:** All Critical and High findings resolved before Phase 16
- **Bug bounty:** Active during audit. Critical = $10,000 / High = $2,500 / Medium = $500

---

### 15.4 Ecosystem Grants (Year 1)

- First cohort of 10 funded external projects
- Developer hackathon series: 3 events in Year 1
- DeFi integrations: DEX, lending protocol, stablecoin bridge
- Cross-chain bridges: Bitcoin (HTLC), Ethereum (ZK bridge)
- Mobile wallet v2.0 (consumer-grade; iOS + Android; send via optimised STARK)
- Block explorer v2.0 (full RVM contract interaction UI, ABI viewer)

**Testnet milestone:** 100+ external validators, Casper FFG finalising epochs across ≥5 geographic regions for 30 consecutive days.

---

## Phase 16 — Global Scale + Full Decentralisation

- 1,000,000 active users
- 50,000+ active validators across ≥50 countries
- On-chain governance fully operational
- Founding team governance participation < 5%
- Protocol Development Fund under community governance
- No founding team special privileges

**This is the terminal state. The protocol governs itself.**

---

## Technical Debt Log

| Item | Location | Severity | Notes |
|---|---|---|---|
| `KEYSTORE_AD = b"rupeecoin-keystore-v2"` for v3 files | `wallet/src/keystore.rs:28` | Low | Intentional — changing breaks existing keystores |
| Slot collision in RPC-20 allowance mapping | `core/rvm/src/token_rpc20.rs` | Low ⚠️ | Asymmetric hash; negligible but unproven collision probability |
| Privacy tree not rolled back on reorg | `core/blockchain/src/lib.rs` | Low | Append-only by design; accepted Phase 14 trade-off |
| Trezor Dilithium3 signing | `wallet/src/hw_trezor.rs` | Medium | HID enumeration + Initialize handshake works; signing requires custom firmware (Trezor partner program) |
| No light-client / SPV protocol | — | Low | Full node required; future phase |

---

## Versioning Policy

| Version | Meaning |
|---|---|
| `0.x.y` | Pre-testnet. Breaking changes without notice. |
| `0.9.x` | Public testnet. Breaking changes with 2-week notice. |
| `1.0.0` | Mainnet. No breaking changes without governance vote. |

**Current version:** `0.9.0` — Phase 14 complete; public testnet ready  
**Next version:** `1.0.0` — when Phase 15 security audit clears and ≥500 validators are active at genesis

---

*A roadmap is a commitment, not a forecast. The milestones are not dates — they are verifiable states of the world.*
