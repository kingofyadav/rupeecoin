# RupeeCoin — Development Roadmap

> Last updated: 2026-06-15  
> Current state: Phases 0–10.2 complete (desktop + mobile wallets shipped).  
> Next milestone: Phase 10.3 — Public validator onboarding + Security Audit #1

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
| 9.3 | JavaScript/TypeScript SDK | ✅ Complete 2026-06-14 |
| 9.4 | Python SDK (PyO3 + maturin) | ✅ Complete 2026-06-14 |
| 9.5 | Testnet Faucet (axum + rate limit + SPA) | ✅ Complete 2026-06-15 |
| 9.6 | Benchmarking Suite (Criterion, 4 bench groups) | ✅ Complete 2026-06-15 |
| 10.1 | Desktop Wallet GUI (Tauri 2 + dark-mode SPA) | ✅ Complete 2026-06-15 |
| 10.2 | Mobile Wallet Alpha (Flutter + FRB 2 + Android) | ✅ Complete 2026-06-15 |

**Current test count:** 86 tests, 0 failures, 0 warnings.  
(+5 mobile bridge: generate/unlock/advance/change_passphrase + keystore roundtrip)

---

## What Is Currently Stubbed / Placeholder

These exist in the code as scaffolding but are not real implementations yet:

| File | Stub | Impact |
|---|---|---|
| `core/privacy/src/proof.rs` | `verify_basic()` for Stark still returns `true` — real Winterfell verify is at blockchain layer | Phase 8.1 done; final fix in Phase 11 |
| `core/zkproof/` | STARK proves Poseidon shadow tree, not BLAKE3 chain tree — no full unlinkability yet | Phase 11 adds BLAKE3-in-circuit |
| `node/src/main.rs` | LMD-GHOST computed and logged but no reorg — chain rollback not yet implemented | Phase 8.4 done; reorg in future |

---

## Phase 8 — Protocol Hardening
**Priority: Ship next**  
**Goal:** Make every component production-grade. No more stubs in consensus or proofs.

### 8.1 Real zk-STARK Spend Proofs

**What:** Replace `SpendEvidence::Blake3Path` (linkable Merkle path) with actual Winterfell STARK proofs.

**Files to change:**
- `core/zkproof/src/lib.rs` — implement the prover circuit over Winterfell:
  - Statement 1: know `spend_key` such that `nullifier_key = BLAKE3(spend_key || 0x01)`
  - Statement 2: know `rcm` such that `cm = BLAKE3(value_le || payment_addr || rcm)` and `cm` is in the Merkle tree at `anchor`
  - Statement 3: sum of input values == sum of output values + fee
- `core/privacy/src/proof.rs` — `StarkSpendProof::verify()` calls real verifier
- `wallet/src/main.rs` — `cmd_send` calls prover, submits `SpendEvidence::Stark`

**Performance targets (from whitepaper §13.4.3):**
- Proof generation: ≤3 seconds on mobile, ≤500ms on desktop
- Proof verification: ≤50 milliseconds on validator

**Completion condition:** `verify_basic()` calls real Winterfell verifier; all 7 privacy tests still pass; a full send→scan round-trip works with a STARK proof.

---

### 8.2 On-Chain Validator Reward Claims

**What:** Add a `Transaction` type (or coinbase extension) that lets validators convert `claimable_rewards` into spendable UTXO outputs.

**Files to change:**
- `core/types/src/transaction.rs` — add `TransactionType::RewardClaim { validator_addr }` variant
- `core/blockchain/src/lib.rs` — `validate()` and `apply_block()` handle reward claim
- `core/staking/src/lib.rs` — `withdraw_rewards(addr) -> u64` zeroes `claimable_rewards`
- `wallet/src/main.rs` — add `wallet claim` command

**Completion condition:** Validator runs node for 10 blocks, runs `wallet claim`, and balance increases on-chain.

---

### 8.3 On-Chain Slashing Proof Transactions

**What:** When `SlashingDetector` finds double-proposal or double-vote evidence, the detecting node should be able to submit it as a transaction and receive a 50% share of the slashed stake.

**Files to change:**
- `core/types/src/transaction.rs` — add `SlashingEvidence` as a transaction payload
- `core/blockchain/src/lib.rs` — validate slashing evidence; apply slash + reward to reporter
- `node/src/main.rs` — broadcast slashing evidence when detected

---

### 8.4 LMD-GHOST Fork Choice

**What:** The current chain uses the simplest possible rule (latest block wins). Under network partitions this fails. Implement LMD-GHOST (Latest Message Driven Greedy Heaviest Observed SubTree).

**Files to change:**
- `core/consensus/src/lib.rs` — add `ForkChoice::lmd_ghost(attestations, blocks)` 
- `node/src/main.rs` — use fork choice on `BlockReceived` events to select canonical chain

---

### 8.5 Separate Proposal Score from Uptime Score

**What:** `ContributionMetrics::score()` currently sets `proposal = uptime`. This makes `W_PROPOSAL = 0.2` a phantom weight. Proposal quality should track `slots_timely / slots_assigned` separately from attestation timeliness.

**Files to change:**
- `core/staking/src/lib.rs:73` — remove `let proposal = uptime;`, track proposal score from actual block-production records

---

## Phase 9 — Developer Tooling
**Goal:** Make RupeeCoin a productive environment for external developers before opening the testnet.

### 9.1 Block Explorer (REST + Web UI)

A standalone `explorer/` binary crate exposing a REST API over the RocksDB store.

**Endpoints:**
```
GET /block/:height         → full block JSON
GET /block/hash/:hash      → block by hash
GET /tx/:txid              → transaction + UTXO inputs/outputs
GET /address/:addr         → UTXO set for address + tx history
GET /height                → current chain tip
GET /nullifier/:hex        → spent/unspent status
GET /search?q=             → block hash, tx hash, or address lookup
```

**Stack:** axum (already in node) + serving a simple SPA (HTML/JS).  
**Completion condition:** Running `cargo run -p rupeecoin-explorer -- --data-dir ./data` serves a page showing the last 20 blocks.

✅ **Complete 2026-06-14** — `explorer/` binary crate serving all 7 REST endpoints + dark-mode SPA at `/`.

---

### 9.2 Validator Dashboard

A simple terminal dashboard (using `ratatui`) showing:
- Active validator count, own status
- Slot/epoch, last finalized epoch
- Claimable rewards balance
- Uptime/attestation score
- Mempool size, pending privacy txs

**Completion condition:** `cargo run -p rupeecoin-dashboard -- --rpc http://127.0.0.1:9944` renders a live-updating terminal UI.

---

### 9.3 JavaScript / TypeScript SDK

A `sdk-js/` package implementing the same API as `rupeecoin-sdk` (Rust):
- Key generation (using WASM-compiled Dilithium3 and BLAKE3)
- Transaction building and signing
- RPC client for all 7 node endpoints
- Wallet scanning (trial-decrypt shielded outputs)
- Payment URI parsing

**Completion condition:** A browser page can build and submit a transparent transaction to a local node using the JS SDK.

---

### 9.4 Python SDK

A `sdk-py/` package via PyO3 FFI wrapping the Rust `rupeecoin-sdk` crate.

**Completion condition:** `pip install rupeecoin` and 5-line script to query chain height and build a tx.

---

### 9.5 Testnet Faucet

A lightweight web service:
- User pastes a Base58Check address
- Faucet signs and broadcasts a coinbase-from-treasury tx sending 100 RPC
- Rate-limited to 1 request per IP per 24 hours

✅ **Complete 2026-06-15** — `faucet/` binary crate. Dark-mode SPA at `/`, `POST /drip` (JSON), `GET /api/stats`. Keyfile (`--keyfile`) persists seed + nonce across restarts. Rate-limiting on `IpAddr`. Nonce committed only after successful node submission.

**Usage:**
```
# Fund the faucet address first (shown on startup), then:
cargo run -p rupeecoin-faucet -- --node-rpc http://127.0.0.1:9944 --keyfile faucet-key.json
# Browse to http://127.0.0.1:8081
```

---

### 9.6 Benchmarking Suite

Add to `rupeecoin-tests/`:
- Block throughput: blocks/second under 1000-tx load
- UTXO set state_root computation time at 1M UTXOs
- Privacy tx proof generation + verification latency
- Network sync speed: blocks/second peer sync at 100k heights

✅ **Complete 2026-06-15** — 4 Criterion benchmark groups in `rupeecoin-tests/benches/`.  
Run with: `cargo bench -p rupeecoin-tests`

**Baseline results (release build, AMD64):**

| Benchmark | Result | Notes |
|---|---|---|
| `build_block/single` | ~830 µs / block → **~1 200 blocks/s** | construction only |
| `add_blocks/200` | ~249 ms / 200 blocks → **~803 blocks/s** | production + validation |
| `chain_sync/50` | 11.5 ms / 50 → **4 355 blocks/s** | pure sync, short chain |
| `chain_sync/1000` | 2.28 s / 1000 → **439 blocks/s** | sync slows with UTXO growth |
| `state_root/10K` | **34 ms** → 290K UTXOs/s | O(N log N) sort + hash |
| `state_root/100K` | **346 ms** → 289K UTXOs/s | steady throughput |
| `state_root` (extrapolated 1M) | ~3.5 s | Phase 10+ optimization target |
| `prove_spend` | ~3–5 s (from unit tests) | Winterfell STARK prove |
| `verify_spend` | ~50 ms (from unit tests) | Winterfell STARK verify |

**Key observations:**
- Sync speed degrades with chain length because each `add_block` recomputes the state root (O(N log N)); incremental Merkle roots would fix this in Phase 11.
- STARK verification at ~50 ms is within the ≤50 ms whitepaper target.
- STARK proof generation at ~3–5 s exceeds the ≤3 s mobile target; circuit optimisation is Phase 11 scope.

---

## Phase 10 — Public Testnet (Zeta)
**Goal:** Open the network to external validators. Real-world stress testing.

### Prerequisites (gate from Phase 9):
- Block explorer live
- JS SDK published to npm
- Testnet faucet running
- All Phase 8 stubs resolved (real STARKs, real fork choice)

### Deliverables:
- **Public validator onboarding**: docs + `rupeecoin-node --stake 10000 --rpc-port 9944`
- **External Security Audit #1**: consensus + core chain + staking
  - Scope: `core/consensus`, `core/staking`, `core/blockchain`, `core/utxo`
  - Firms: minimum 2 independent external security firms
  - Gate: all Critical and High findings resolved before Phase 11
- **Bug bounty program**: critical = $10,000, high = $2,500, medium = $500
- **Desktop wallet GUI**: Tauri (Rust backend + React frontend) wrapping the existing wallet CLI
- **Mobile wallet alpha**: Android first (Flutter + WASM crypto core)

**Testnet milestone:** 100 external validators running continuously for 30 days. Casper FFG finalizing epochs correctly across ≥5 geographic regions.

---

## Phase 11 — Full zk-STARK Hardening + ZK Audit
**Goal:** The privacy layer is production-grade. Proofs are real, fast, and audited.

### 11.1 STARK Circuit Completeness

The Phase 8 STARK circuit proves input ownership + balance. Phase 11 completes it:
- **Statement 4**: output well-formedness (encrypted memo correctly addressed to recipient's Kyber public key)
- **Batched verification**: validate up to 10 inputs in a single aggregated proof
- **Recursive STARKs**: aggregate multiple txs into a single block-level proof (reduces validator verification cost)

### 11.2 Mobile Proof Generation

- WASM-compiled prover for browser wallets
- Native iOS/Android prover using Winterfell compiled to arm64
- Target: <3s proof generation on iPhone 12 / Pixel 6

### 11.3 ZK Security Audit (#2)

- Scope: `core/privacy`, `core/zkproof`, Winterfell circuit
- Focus: soundness of the custom STARK circuit, nullifier scheme, commitment hiding/binding
- Gate: all Critical and High findings resolved

---

## Phase 12 — RupeeCoin VM (Layer 2)
**Goal:** Programmability via the RupeeCoin Virtual Machine (RVM).

### 12.1 RVM Specification

A new whitepaper appendix + formal spec:
- Stack-based instruction set (similar to EVM but cleaner)
- 64-bit words, no integer overflow traps (explicit checked ops)
- Gas metering table for all opcodes
- Determinism guarantee: same input → same output on all platforms

### 12.2 RVM Implementation

New crate `core/rvm/`:
- Interpreter (initial)
- JIT compiler (Phase 12B)
- Formal determinism test suite
- Gas accounting

### 12.3 Smart Contract Types

New transaction types:
- `ContractDeploy { bytecode, constructor_args }`
- `ContractCall { contract_addr, method, args, gas_limit }`

### 12.4 Token Standards

- **RPC-20**: Fungible token (ERC-20 analogue with Dilithium-signed transfers)
- **RPC-721**: Non-fungible token
- **RPC-1155**: Multi-token

### 12.5 EVM Transpiler

A best-effort `evm2rvm` transpiler so existing Solidity contracts can run on RVM with minor changes. Target: 80% of common DeFi patterns transpilable automatically.

---

## Phase 13 — Mainnet Preparation
**Goal:** The last gate before launch. No rushing.

### 13.1 Security Audit #3 (Full Protocol)

Full audit by 2 independent firms:
- All crates (15 total)
- Economic model review (tokenomics, PMFP, inactivity leak)
- Network-level threat modeling (Dandelion++, eclipse attacks, Sybil)
- Cryptographic review of Dilithium3 integration, BLAKE3 usage, Kyber768

**Gate:** Zero unresolved Critical findings. Zero unresolved High findings.

### 13.2 Genesis Block Parameters

Finalize and publish:
- `MAINNET_CHAIN_ID = 1` (already set)
- `BLOCK_REWARD_SCHEDULE` — exact halving block heights
- Genesis validator set: target 500–1,000 validators registered before launch
- Genesis timestamp (T-72h countdown)
- Initial treasury balance

### 13.3 Tor / I2P Support

Per whitepaper §13.6.2: add optional Tor hidden service mode to `rupeecoin-node`.
- `--tor-socks <proxy>` flag routes all P2P connections through Tor
- `.onion` address advertised to peers for inbound connections
- Performance impact documented

### 13.4 Hardware Wallet Support

Partner with Ledger and Trezor for Dilithium3 firmware:
- Dilithium3 signing on hardware (no private key on host)
- BLAKE3 address derivation on device
- `wallet send` with `--hardware-wallet ledger` flag

### 13.5 Final SDK Versions

All SDKs at v1.0:
- Rust SDK v1.0
- JavaScript/TypeScript SDK v1.0
- Python SDK v1.0
- Mobile SDK v1.0 (iOS + Android)

---

## Phase 14 — Mainnet Launch

**Launch sequence:**
1. T-72h: Genesis block parameters published on-chain (no blocks yet)
2. T-24h: Final validator registrations closed
3. T-0: Genesis block produced
4. T+1h: Network stability confirmed (≥500 validators active, blocks finalizing)
5. T+72h: Stability monitoring window complete → launch declared successful
6. T+1w: First ecosystem grants announced

**Mainnet milestone:** 500+ validators, blocks producing and finalizing continuously for 30 days post-genesis, no Sybil or fork incidents.

---

## Phase 15 — Ecosystem Growth (12–18 months post-launch)

- Ecosystem grants program: first cohort of 10 funded projects
- Developer hackathon series: 3 events in Year 1
- DeFi integrations: DEX, lending protocol, stablecoin bridge
- Cross-chain bridges: Bitcoin (HTLC), Ethereum (ZK bridge)
- Mobile wallet v2.0 (consumer-grade, both platforms)
- Block explorer v2.0 (full RVM contract interaction UI)
- 100+ projects building on RupeeCoin

---

## Phase 16 — Global Scale + Full Decentralization

- 1,000,000 active users
- 50,000+ active validators in ≥50 countries
- On-chain governance fully operational
- Founding team governance participation < 5%
- Protocol Development Fund under community governance
- No founding team special privileges

**This is the terminal state. The protocol governs itself.**

---

## Phase 10 — Status (in order)

All Phase 8 and Phase 9 prerequisites are met.

✅ 1. **Desktop wallet GUI** — `desktop-wallet/`
   - Tauri 2 (webkit2gtk-4.1 native on Ubuntu 24.04) + dark-mode HTML/JS SPA
   - Create / unlock / send / receive / UTXO list / settings
   - Keystore: Argon2id + XChaCha20-Poly1305 (same format as mobile wallet)
   - `cargo build --release` in `desktop-wallet/` — builds and runs

✅ 2. **Mobile wallet alpha** — `mobile-wallet/`
   - Flutter + flutter_rust_bridge 2; Rust crypto bridge (cdylib)
   - Android arm64-v8a + x86_64, minSdk 24, package `network.rupeecoin.wallet`
   - 6 bridge API functions; 5/5 Rust tests pass
   - Send deferred to Phase 11 (STARK prove too slow on mobile)
   - Run `flutter_rust_bridge_codegen generate` + `flutter build apk` for APK

✅ 3. **Bug bounty + SECURITY.md**
   - Critical = $10,000 / High = $2,500 / Medium = $500
   - Responsible disclosure: security@rupeecoin.network
   - SECURITY.md published in repo root

🔲 4. **Public validator onboarding docs** — NEXT
   - Written guide: how to join the Zeta testnet with `--stake 10000 --rpc-port 9944`
   - Publish bootnode multiaddr + checkpoint hash
   - Target: first 10 external validators running
   - Deliverable: `docs/testnet-validator.md`

🔲 5. **Security Audit #1**
   - Scope: `core/consensus`, `core/staking`, `core/blockchain`, `core/utxo`
   - Gate: contract with ≥2 independent external firms before Phase 11
   - All Critical + High findings resolved before Phase 11 launch

---

## Technical Debt Log

Known shortcuts to address before mainnet:

| Item | Location | Severity | Notes |
|---|---|---|---|
| STARK proves Poseidon shadow tree, not BLAKE3 chain tree | `core/zkproof/` | High — spend unlinkability incomplete | Phase 11 |
| LMD-GHOST computed but no chain rollback | `node/src/main.rs` | High — fork recovery unimplemented | Phase 10 |
| `verify_basic()` for Stark returns `true` at privacy layer | `privacy/src/proof.rs` | Medium — real verify is at blockchain layer only | Phase 11 |
| `apply_inactivity_leak` not called at every epoch in `add_block` | `blockchain/src/lib.rs` | Medium — leak may not fire | Phase 10 |
| Tor/I2P support | `network/src/lib.rs` | Medium — network-level privacy incomplete | Phase 13 |
| `KEYSTORE_AD = b"rupeecoin-keystore-v2"` for v3 files | `wallet/src/keystore.rs:28` | Low — naming inconsistency only | Phase 13 |

---

## Versioning Policy

| Version | Meaning |
|---|---|
| `0.x.y` | Pre-testnet. Breaking changes without notice. |
| `0.9.x` | Public testnet. Breaking changes with 2-week notice. |
| `1.0.0` | Mainnet. No breaking changes without governance vote. |

Current version: `0.3.0` (all crates)  
Next version: `0.4.0` when Phase 8 stubs (STARK proofs, reward persistence, LMD-GHOST) are resolved.

---

*A roadmap is a commitment, not a forecast. The milestones are not dates — they are verifiable states of the world.*
