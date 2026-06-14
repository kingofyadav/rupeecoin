# ₹ RupeeCoin

> **From India. For the World. Owned by No One.**

A quantum-resistant, privacy-native decentralized monetary network built for the century ahead.

Bitcoin introduced trustless money.  
Ethereum introduced programmable money.  
Both changed the world.  
Both are now showing their age.

RupeeCoin is an attempt to build what comes next —  
born in India, the nation that already proved payments can scale to a billion people.

---

## Why RupeeCoin Exists

The world still lacks a monetary network that is simultaneously:

- Quantum-resistant
- Privacy-preserving
- Programmable
- Energy-efficient
- Massively scalable
- Fully decentralized

Existing systems solve parts of the problem.  
RupeeCoin aims to solve the entire problem.

---

## Core Principles

**1. Quantum-Safe From Birth**  
Security should not depend on cryptography that future quantum computers may break.  
RupeeCoin is designed around post-quantum standards — not retrofitted later.

**2. Privacy As A Fundamental Right**  
Financial privacy should be the default, not a premium feature.  
Transactions are private unless the user chooses otherwise.

**3. Security Before Features**  
A secure monetary base layer matters more than rapid feature expansion.  
We do not ship features that compromise the foundation.

**4. Decentralization Above Everything**  
No government. No corporation. No founder control.  
The protocol belongs to its users.

**5. Built For Billions**  
The protocol is designed with global adoption in mind from day one.  
India runs 10 billion UPI transactions per month. We know what scale looks like.

**6. Open Source Forever**  
All protocol development happens in public.  
No closed research. No private roadmaps.

---

## RupeeCoin Compared

| Property | Bitcoin | Ethereum | RupeeCoin |
|---|---|---|---|
| Quantum Resistant | ❌ | ❌ | ✅ |
| Privacy By Default | ❌ | ❌ | ✅ (Phase 4 partial; full in Phase 5B) |
| Smart Contracts | ❌ | ✅ | Planned (Phase 12) |
| Energy Efficient | ❌ | ✅ | ✅ |
| Social Key Recovery | ❌ | ❌ | Planned |
| Programmable Currency Design | ❌ | ⚠️ | ✅ |
| Post-Quantum Architecture | ❌ | ❌ | ✅ |
| Perpetual Validator Incentive | ❌ | ⚠️ | ✅ |
| Founder Control (End Goal) | N/A | N/A | None |

---

## Architecture

```
┌──────────────────────────────────────────────────────────┐
│                  LAYER 3 — APPLICATION                    │
│        Wallets · SDKs · Mobile · Merchant · Onboarding    │
│                  Simple · Mobile-First · Global           │
├──────────────────────────────────────────────────────────┤
│                  LAYER 2 — EXECUTION                      │
│     Smart Contracts · Payment Channels · DeFi · Rollups   │
│                  Fast · Extensible · Developer-Friendly   │
├──────────────────────────────────────────────────────────┤
│                  LAYER 1 — SETTLEMENT                     │
│      Consensus · UTXO · Quantum-Resistant Signatures      │
│                  Minimal · Secure · Immutable             │
└──────────────────────────────────────────────────────────┘

  Signatures:  CRYSTALS-Dilithium (NIST Post-Quantum Standard)
  Hashing:     BLAKE3
  Privacy:     zk-STARKs (no trusted setup, quantum-safe)
  Consensus:   Proof of Stake + Proof of Contribution (Hybrid)
  VM:          RupeeCoin Virtual Machine (RVM) — planned
```

**Layer 1 — Settlement Chain** *(Bitcoin DNA)*  
The immutable source of truth. Does one job: final, secure settlement.  
UTXO model · Quantum-resistant signatures · Validator consensus · Maximum security.

**Layer 2 — Execution Layer** *(Ethereum DNA)*  
Programmability and scalability on top of the secure base.  
RupeeCoin VM · Smart contracts · Payment channels · DeFi · Token standards.

**Layer 3 — Application Layer** *(India DNA)*  
Where a billion users actually interact with the protocol.  
Consumer wallets · Developer SDKs · Mobile-first · Merchant tooling.

---

## Technology Stack

| Component | Technology | Reason |
|---|---|---|
| Core Language | Rust | Memory safety + performance |
| Networking | libp2p | Battle-tested, used by Ethereum |
| Storage | RocksDB | High-performance chain state |
| Hashing | BLAKE3 | Fast + quantum-resistant |
| Signatures | CRYSTALS-Dilithium | NIST post-quantum standard |
| Privacy | zk-STARKs (Winterfell) | No trusted setup, quantum-safe |
| Smart Contracts | RupeeCoin VM (RVM) | Purpose-built, not retrofitted |
| APIs | Rust / TypeScript / Python | Widest developer reach |
| Mobile | Android / iOS | India-first mobile population |

---

## Repository Structure

```
rupeecoin/
│
├── whitepaper/                  # Protocol specification (16 sections)
│   ├── sections/                # Individual section drafts
│   └── assets/                  # Diagrams and references
│
├── core/                        # Layer 1 — Settlement Chain (12 crates)
│   ├── blockchain/              # Block and chain logic
│   ├── consensus/               # Casper FFG + VRF + LMD-GHOST
│   ├── crypto/                  # Dilithium3, BLAKE3, HD keys, VRF
│   ├── economics/               # EIP-1559, PMFP, supply schedule, inactivity leak
│   ├── mempool/                 # Transaction pool (EIP-1559 eviction)
│   ├── network/                 # libp2p gossipsub + Dandelion++
│   ├── privacy/                 # Shielded UTXO pool, notes, nullifiers, Kyber768
│   ├── staking/                 # ValidatorRegistry, slashing, reward claims
│   ├── storage/                 # RocksDB chain state (8 column families)
│   ├── types/                   # Block, Transaction, Attestation types
│   ├── utxo/                    # UTXO set + state root (BLAKE3 Merkle)
│   └── zkproof/                 # Winterfell STARK prover/verifier (real)
│
├── node/                        # Full node binary (tokio + axum RPC)
├── wallet/                      # CLI wallet (HD keys + shielded pool + STARK)
├── explorer/                    # Block explorer (axum REST + dark-mode SPA)
├── dashboard/                   # Validator dashboard (ratatui TUI)
├── faucet/                      # Testnet faucet (axum + rate limiting + SPA)
├── rupeecoin-sdk/               # Rust SDK for external developers
├── sdk-js/                      # JavaScript/TypeScript SDK (bincode, RPC, tx)
├── sdk-py/                      # Python SDK (PyO3 + maturin wheel)
└── rupeecoin-tests/             # Integration tests + Criterion benchmarks
```

---

## Development Roadmap

---

> **Current state (2026-06-15):** Phases 0–9.6 complete (81 tests, 4 benchmark groups).  
> Next: Phase 10 — Public Testnet (Zeta).

### Phase 0 — Foundation
**Status:** `Complete` · **Goal:** Build the intellectual foundation before writing production code.

- [x] Section 1 — Abstract
- [x] Section 2 — Introduction
- [x] Section 3 — Problems With Existing Systems
- [x] Section 4 — Vision and Philosophy
- [x] Section 5 — Learning From Bitcoin
- [x] Section 6 — Learning From Ethereum
- [x] Section 7 — Cryptographic Foundation
- [x] Section 8 — Consensus Mechanism
- [x] Section 9 — Three-Layer Architecture
- [x] Section 10 — Transaction Model
- [x] Section 11 — Tokenomics
- [x] Section 12 — Security Analysis
- [x] Section 13 — Privacy Model
- [x] Section 14 — Governance
- [x] Section 15 — Roadmap
- [x] Section 16 — Conclusion
- [ ] Threat model completed
- [ ] Repository launched (public)
- [ ] Community channels launched

**Milestone:** Whitepaper v1.0 published.

---

### Phase 1 — Research Prototype
**Status:** `Complete` · **Goal:** Validate core assumptions in code.

- [x] Rust workspace setup
- [x] Block and transaction structure
- [x] BLAKE3 integration
- [x] CRYSTALS-Dilithium integration (Dilithium3, seeded keypair C shim)
- [x] HD key derivation scheme (BLAKE3-based, m/account/index)
- [x] Address format (Base58Check)
- [x] UTXO model implemented
- [x] Merkle tree with BLAKE3
- [x] Chain state management (RocksDB, 8 column families)
- [x] Single-machine multi-node simulation
- [x] Block production without network
- [x] Transaction validity verification
- [x] UTXO set management
- [x] Test infrastructure in place (81 tests, 0 failures)
- [ ] zk-STARK library evaluation and selection (Winterfell selected; integration in progress)
- [ ] CI/CD pipeline established

**Milestone:** Single-machine blockchain prototype running.

---

### Phase 2 — Alpha Network
**Status:** `Complete (core)` · **Goal:** First distributed network.

- [x] libp2p networking layer (gossipsub + request-response)
- [x] Peer discovery
- [x] Block propagation
- [x] Transaction broadcast
- [x] Dandelion++ transaction propagation
- [x] Validator registration
- [x] Basic PoS block production
- [x] Attestation collection
- [x] Casper FFG finality prototype
- [x] Transaction mempool (4096 cap, EIP-1559 eviction)
- [x] Fee-based transaction ordering
- [x] Mempool eviction policy
- [x] Genesis block specification
- [x] Genesis validator set bootstrapping
- [ ] Fork choice rule implemented (LMD-GHOST — Phase 8.4)
- [ ] Testnet faucet (Phase 9.5)
- [ ] 10-node internal testnet running
- [ ] Sustained block production over 72 hours
- [ ] Fork recovery demonstrated

**Milestone:** 10-node functioning network.

---

### Phase 3 — Validator Testnet
**Status:** `Partial` · **Goal:** Stress test decentralization and consensus.

- [x] Proof of Contribution scoring system (uptime 30%, attestation 40%, proposal 20%, propagation 10%)
- [x] Slashing conditions — equivocation (double proposal)
- [x] Slashing conditions — double voting
- [x] Inactivity leak mechanism
- [x] Validator exit and unstaking (604,800-slot delay)
- [x] Block reward distribution (halving schedule)
- [x] Transaction fee distribution (EIP-1559 + PMFP)
- [x] Perpetual Micro-Fee Pool prototype
- [x] Contribution multiplier in reward calculation
- [x] Validator monitoring dashboard (ratatui TUI)
- [x] Performance benchmarks (Criterion suite — Phase 9.6)
- [ ] Network health metrics
- [ ] Slashing evidence explorer
- [ ] Internal security review completed
- [ ] Known attack vectors tested
- [ ] Security Audit #1 (external, core chain + consensus) — Phase 10

**Milestone:** 100 validators running on testnet.

---

### Phase 4 — Privacy Layer
**Status:** `Partial` · **Goal:** Integrate zero-knowledge privacy.

- [x] zk-STARK library selected (Winterfell) and integrated (placeholder)
- [x] Private UTXO commitment scheme (BLAKE3 note commitments)
- [x] Nullifier scheme (BLAKE3(nk ∥ cm), RocksDB + in-memory HashSet)
- [x] Commitment tree (BLAKE3 binary Merkle tree)
- [x] Shielded transaction structure (PrivacyTransaction)
- [x] Note encryption (Kyber768 KEM + XChaCha20-Poly1305)
- [x] Wallet scanner (trial-decryption of all shielded outputs)
- [ ] Real zk-STARK circuit for transaction validity (Phase 8.1)
- [ ] Proof generation — prover circuit (Phase 8.1)
- [ ] Proof verification — real Winterfell verifier (Phase 8.1)
- [ ] Selective disclosure — transaction viewing key
- [ ] Selective disclosure — address viewing key
- [ ] Encrypted memo field
- [ ] Proof generation benchmarks (target: ≤3s mobile, ≤500ms desktop)
- [ ] Proof verification benchmarks (target: ≤50ms validator)
- [ ] Throughput impact assessment
- [ ] Privacy audit (Security Audit #2)

**Milestone:** Private transactions verifiable on testnet.

---

### Phase 5 — RupeeCoin VM
**Status:** `Not started` · **Goal:** Make the protocol programmable.

- [ ] RVM specification finalized
- [ ] VM runtime implementation
- [ ] Smart contract compiler
- [ ] Contract deployment flow
- [ ] Token standards (RPC-20, RPC-721, RPC-1155)
- [ ] Developer tooling alpha

**Milestone:** First smart contract deployed on testnet.

---

### Phase 6 — Public Testnet (Zeta)
**Status:** `In progress` · **Goal:** Open the network to the world.

- [x] Block explorer live (Phase 9.1)
- [x] Testnet faucet running (Phase 9.5)
- [x] SDK alpha (Rust, JS, Python — Phases 9.3, 9.4)
- [x] Developer documentation complete
- [ ] Public validator onboarding (Phase 10)
- [ ] Consumer wallet release GUI (Phase 10)
- [ ] Bug bounty program (Phase 10)
- [ ] External Security Audit #1 (Phase 10)

**Milestone:** 100 external validators running 30 days continuously.

---

### Phase 7 — Mainnet Launch
**Status:** `Not started` · **Goal:** Launch the live network.

- [ ] Genesis block finalized
- [ ] Validator onboarding (target: 1,000 at launch)
- [ ] Wallet release (CLI + desktop)
- [ ] Block explorer mainnet
- [ ] Independent security audit passed
- [ ] Mainnet activation

**Milestone:** RupeeCoin Mainnet live.

---

### Phase 8 — Ecosystem Growth
**Status:** `Not started` · **Goal:** Build the developer ecosystem.

- [ ] Grants program launched
- [ ] Developer hackathon series
- [ ] SDK maturity (v1.0)
- [ ] Educational content and courses
- [ ] DeFi protocol integrations

**Milestone:** 100+ projects building on RupeeCoin.

---

### Phase 9 — Global Scale
**Status:** `Not started` · **Goal:** International adoption.

- [ ] Hardware wallet support (Ledger, Trezor)
- [ ] Cross-chain interoperability bridges
- [ ] Mobile ecosystem (Android + iOS apps)
- [ ] Merchant payment integrations
- [ ] Institutional-grade infrastructure
- [ ] Multi-language documentation

**Milestone:** 1,000,000 active users.

---

### Phase 10 — Full Decentralization
**Status:** `Not started` · **Goal:** The protocol becomes ownerless.

- [ ] On-chain governance activated
- [ ] Founder privileges removed and timelocked
- [ ] Community-led upgrade process live
- [ ] Long-term community treasury model
- [ ] Protocol fully self-governing

**Milestone:** Protocol governed entirely by its community. No founder control. Mission complete.

---

## Security Philosophy

Priority order — non-negotiable:

1. **Security**
2. **Decentralization**
3. **Privacy**
4. **Scalability**

No feature is worth compromising the first three.

For vulnerability disclosures, read [SECURITY.md](./SECURITY.md) before going public.

---

## Contributing

RupeeCoin is built by the community, for the community.  
No corporation. No government. No single owner.

We welcome contributors in:

- Cryptography and post-quantum research
- Rust and distributed systems engineering
- Zero-knowledge proof design
- Economics and game theory
- Security auditing
- Technical writing and documentation
- Community building and education

Read [CONTRIBUTING.md](./CONTRIBUTING.md) to get started.

---

## License

MIT License — free to use, free to modify, free to distribute.  
See [LICENSE](./LICENSE) for full terms.

---

## The Mission

> Build a monetary network that remains secure in the quantum era,  
> protects financial privacy by default,  
> and belongs to no government, corporation, or founder —  
> for generations to come.

Built in India.  
Open to the world.  
Owned by no one.
