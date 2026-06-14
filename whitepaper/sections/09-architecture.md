# Section 9 — Three-Layer Architecture

> Architecture is the set of decisions that are expensive to change later.  
> Everything else is implementation detail.  
> This section defines the structural design of RupeeCoin —  
> the layers, their boundaries, their responsibilities, and the reasoning  
> behind every separation.

---

## 9.1 Design Philosophy

A monetary network serving a billion users cannot be a single monolithic system.  
The properties required at different levels of the stack are in direct tension with each other:

- Security requires simplicity. Complexity creates attack surface.
- Scalability requires flexibility. Flexibility creates complexity.
- Privacy requires computation. Computation requires resources.
- Programmability requires expressiveness. Expressiveness conflicts with auditability.

The solution is separation of concerns — distributing conflicting requirements across distinct layers,  
each optimized for its specific role,  
with well-defined interfaces between them.

RupeeCoin is structured as three layers:

```
┌─────────────────────────────────────────────────────────────┐
│                    LAYER 3 — APPLICATION                     │
│                                                              │
│   Consumer wallets · Developer SDKs · Merchant tooling      │
│   Mobile clients · Enterprise integrations · UPI-bridge     │
│                                                              │
│   Properties: Simple · Mobile-first · Consumer-grade        │
├─────────────────────────────────────────────────────────────┤
│                    LAYER 2 — EXECUTION                       │
│                                                              │
│   RupeeCoin VM · Smart contracts · Payment channels         │
│   Token standards · DeFi protocols · ZK rollups             │
│                                                              │
│   Properties: Fast · Programmable · Developer-friendly      │
├─────────────────────────────────────────────────────────────┤
│                    LAYER 1 — SETTLEMENT                      │
│                                                              │
│   Consensus · UTXO set · Final settlement                   │
│   Validator coordination · Quantum-resistant signatures      │
│                                                              │
│   Properties: Secure · Minimal · Immutable · Decentralized  │
└─────────────────────────────────────────────────────────────┘
```

Each layer has a single primary responsibility.  
Layers communicate through defined interfaces.  
No layer reaches into the internals of another.

---

## 9.2 Layer 1 — The Settlement Chain

### 9.2.1 Responsibility

Layer 1 has one job: **produce a tamper-proof, globally agreed, ordered record of settled transactions.**

Nothing more. Nothing less.

Layer 1 does not execute smart contracts.  
Layer 1 does not provide privacy computation.  
Layer 1 does not serve consumer APIs.  
Layer 1 does not process payment channel state updates.

Every feature that is not essential to final settlement is excluded from Layer 1 by design.  
This minimalism is a security property, not a limitation.

### 9.2.2 Block Structure

Each Layer 1 block contains:

```
Block {
    header: BlockHeader,
    transactions: Vec<Transaction>,
    attestations: Vec<Attestation>,
    slashing_evidence: Vec<SlashingProof>,    // optional
    pdf_contributions: Vec<PDFContribution>,  // optional
}

BlockHeader {
    version: u32,
    prev_block_hash: Blake3Hash,        // 32 bytes
    merkle_root: Blake3Hash,            // 32 bytes
    state_root: Blake3Hash,             // 32 bytes — UTXO set commitment
    timestamp: u64,                     // Unix timestamp
    slot: u64,                          // Slot number
    epoch: u64,                         // Epoch number
    proposer_index: u32,                // Validator index
    proposer_signature: DilithiumSig,   // ~3293 bytes
    randao_reveal: Blake3Hash,          // VRF randomness contribution
}
```

### 9.2.3 State Model — UTXO

Layer 1 uses the Unspent Transaction Output (UTXO) model, inherited from Bitcoin's design.

In the UTXO model, the state of the ledger is represented as a set of unspent outputs.  
Each output has a value and a spending condition (a public key hash or script).  
A transaction consumes one or more existing UTXOs as inputs and creates one or more new UTXOs as outputs.  
The sum of input values must equal the sum of output values plus the transaction fee.

The UTXO model was chosen over the account model (used by Ethereum) for Layer 1 for three reasons:

**Parallelism.**  
Transactions that consume different UTXOs have no dependencies and can be validated in parallel.  
This enables higher Layer 1 throughput without increasing block time.

**Privacy compatibility.**  
The UTXO model integrates naturally with the ZK-proof privacy system (Section 13).  
UTXO commitments can be hidden while preserving the ability to prove spending validity.

**Auditability.**  
The UTXO set is a complete, compact representation of the current state.  
It can be validated from genesis without trusting any external source.

### 9.2.4 Layer 1 Throughput

Layer 1 is not optimized for high throughput.  
Its role is settlement — the final, irrevocable recording of state changes.

Target Layer 1 throughput: **1,000–5,000 transactions per second**.

This is sufficient for:
- Opening and closing Layer 2 payment channels
- Settling Layer 2 ZK rollup batches
- Direct high-value transfers that require Layer 1 finality

It is not sufficient for:
- Consumer payment volumes (handled by Layer 2)
- Smart contract interactions (handled by Layer 2)
- Application-level traffic (handled by Layer 3)

Layer 1 throughput is deliberately constrained.  
A Layer 1 that can handle all traffic directly has no need for Layer 2 —  
and loses the security properties that come from minimalism.

### 9.2.5 Node Types on Layer 1

**Full node.**  
Stores the complete chain history and full UTXO set.  
Validates all blocks and transactions independently.  
Participates in block propagation.  
Does not require trust in any other node.

**Validator node.**  
A full node with an active stake and the ability to produce and attest to blocks.  
Requires higher availability and more consistent network connectivity than a passive full node.

**Archive node.**  
A full node that retains all historical chain data without pruning.  
Required for chain explorers, analytics, and applications that need access to historical state.

**Light node.**  
Stores only block headers and a current UTXO commitment.  
Verifies transaction inclusion using Merkle proofs.  
Suitable for mobile devices and resource-constrained environments.  
Does not independently validate all transactions — relies on the honest majority assumption.

---

## 9.3 Layer 2 — The Execution Layer

### 9.3.1 Responsibility

Layer 2 provides **programmability and scalability** on top of Layer 1's security foundation.

Layer 2 is where:
- Smart contracts execute
- Payment channels process high-frequency transactions
- ZK rollups batch transactions for efficient Layer 1 settlement
- Token standards are defined and enforced
- DeFi protocols operate

Layer 2 inherits Layer 1's security through cryptographic anchoring:  
all Layer 2 state transitions are ultimately proven or settled on Layer 1.

### 9.3.2 Payment Channels

Payment channels allow two parties to conduct an unlimited number of transactions  
with only two Layer 1 transactions: channel open and channel close.

**Channel lifecycle:**

1. **Open.** Both parties deposit RPC into a 2-of-2 multisignature UTXO on Layer 1. This is the channel funding transaction. Layer 1 finality: ~4 minutes.

2. **Operate.** Both parties exchange signed state updates off-chain. Each state update specifies the current balance distribution between the two parties. No Layer 1 interaction occurs. Latency: milliseconds.

3. **Close (cooperative).** Both parties sign the final state and submit a closing transaction to Layer 1. Funds are distributed according to the final state. Layer 1 finality: ~4 minutes.

4. **Close (unilateral).** If one party becomes unresponsive, the other can submit the latest signed state unilaterally after a challenge period. The challenge period (48 hours by default) allows the counterparty to submit a more recent state if the submitter attempted to cheat.

**Channel routing.**  
Direct channels require both parties to have an open channel.  
Routed payments allow value to flow through a network of channels:  
Alice → Bob → Carol, where Alice and Carol have no direct channel.  
Routing uses Hash Time-Locked Contracts (HTLCs) adapted for RupeeCoin's cryptographic primitives.

### 9.3.3 ZK Rollups

ZK rollups batch large numbers of transactions off-chain and submit a single proof of their validity to Layer 1.

**Rollup operation:**

1. A rollup operator collects transactions from users.
2. The operator executes the transactions off-chain, updating the rollup state.
3. The operator generates a zk-STARK proof that all state transitions were valid.
4. The operator submits the proof and the new state root to Layer 1 as a single transaction.
5. Layer 1 verifies the proof. If valid, the new state root is recorded.

**Properties:**

- Throughput: A single Layer 1 transaction can prove thousands of rollup transactions.
- Security: The zk-STARK proof guarantees that no invalid state transition can be included. The operator cannot steal funds even if malicious — they can only withhold transactions (data availability attack), which is mitigated by data availability mechanisms.
- Finality: Rollup transactions achieve Layer 1 finality when the proof is included in a finalized Layer 1 block.

**Rollup throughput target: 100,000+ transactions per second** across all active rollup operators.

### 9.3.4 RupeeCoin Virtual Machine (RVM)

The RVM is the execution environment for smart contracts on Layer 2.

**Design principles:**

**Determinism.**  
Given the same input state and the same transaction, the RVM must produce the same output on every node, everywhere, always.  
Non-deterministic operations (random numbers, timestamps, external data) are provided through defined oracle interfaces, not built-in operations.

**Resource metering.**  
Every RVM operation consumes a defined quantity of gas.  
Transactions specify a gas limit and a gas price.  
Execution halts if the gas limit is reached.  
This prevents infinite loops and denial-of-service through computational exhaustion.

**Formal verifiability.**  
The RVM instruction set is designed to be amenable to formal verification.  
Smart contracts written for the RVM can be formally verified against a specification.  
This is a property that Ethereum's EVM does not have — its complexity makes complete formal verification impractical.

**Isolation.**  
Each contract executes in a sandboxed environment.  
Contracts cannot directly access the state of other contracts except through defined interface calls.  
This limits the blast radius of a compromised contract.

**RVM vs EVM.**  
The RVM is not compatible with Ethereum's EVM.  
This is a deliberate choice. EVM compatibility would import EVM's legacy design decisions, attack surface, and constraints.  
Developer tooling for the RVM is built from scratch, optimized for the RVM's design goals.  
Transpilation tools from Solidity/EVM to RVM will be provided to ease migration.

### 9.3.5 Token Standards

RupeeCoin defines native token standards at the protocol level rather than as conventions:

**RPC-20: Fungible tokens.**  
The standard interface for fungible token contracts — analogous to Ethereum's ERC-20 but formally specified and enforced by the RVM.

**RPC-721: Non-fungible tokens.**  
The standard interface for unique asset tokens.

**RPC-1155: Multi-tokens.**  
A combined standard for contracts managing multiple token types.

Standards are enforced by the RVM, not by convention.  
A contract claiming to implement RPC-20 that does not satisfy the standard's interface is rejected by the protocol.

---

## 9.4 Layer 3 — The Application Layer

### 9.4.1 Responsibility

Layer 3 is the interface between the protocol and its users.  
It is not a protocol layer in the technical sense — it does not have consensus rules or cryptographic guarantees of its own.  
It is the ecosystem of software that makes the protocol accessible to people who are not cryptographers.

Layer 3 includes:
- Consumer wallets (mobile, desktop, web)
- Merchant payment integration toolkits
- Developer SDKs
- Block explorers
- Analytics platforms
- Enterprise integration middleware

### 9.4.2 Mobile-First Design

India has more smartphone users than desktop computer users by an order of magnitude.  
The global south — RupeeCoin's primary adoption target — is overwhelmingly mobile-first.

Layer 3 is designed for mobile from the beginning, not as an afterthought.

The light node implementation is optimized for mobile constraints:
- Minimal storage: only block headers and relevant UTXOs
- Bandwidth-efficient: Merkle proof verification, not full block download
- Battery-aware: sync scheduling that respects power constraints
- Offline-capable: signed transactions can be queued for broadcast when connectivity returns

### 9.4.3 Developer SDK

The RupeeCoin SDK provides:

**Rust SDK** — full protocol access, suitable for validator nodes and infrastructure.  
**JavaScript / TypeScript SDK** — web and Node.js integration.  
**Python SDK** — scripting, analytics, and research applications.  
**Mobile SDK** — Android (Kotlin/Java) and iOS (Swift) native integration.

All SDKs expose the same conceptual API with language-appropriate idioms:
- Key generation and management
- Transaction construction and signing
- Network broadcast
- Layer 2 channel management
- Smart contract interaction
- ZK proof generation (where client-side proving is required)

### 9.4.4 Merchant Integration

For RupeeCoin to function as a currency, merchants must be able to accept it.

The merchant toolkit provides:
- Payment request generation (QR code and deep link formats)
- Payment verification (lightweight, no full node required)
- Settlement in RPC or Layer 2 stable tokens
- Invoice management
- Reconciliation export (CSV, JSON, API)

Payment confirmation time for merchant purposes:  
Layer 2 channel payment: **< 1 second**  
Layer 1 direct payment: **~4 minutes** (one finality period)  
For point-of-sale, Layer 2 is used by default.

---

## 9.5 Inter-Layer Communication

### 9.5.1 Layer 1 ↔ Layer 2

Layer 2 interacts with Layer 1 through defined transaction types:

**Channel open transaction.** Locks RPC in a UTXO controlled by the channel contract.  
**Channel close transaction.** Releases RPC from the channel UTXO according to final channel state.  
**Rollup batch transaction.** Submits a ZK proof and new state root to Layer 1.  
**Rollup deposit.** Moves RPC from Layer 1 into a rollup.  
**Rollup withdrawal.** Moves RPC from a rollup back to Layer 1 (subject to challenge period).

Layer 2 cannot modify Layer 1 state except through these defined transaction types.  
Layer 1 cannot reach into Layer 2 state — it only records proofs and commitments.

### 9.5.2 Layer 2 ↔ Layer 3

Layer 3 interacts with Layer 2 through the SDK's RPC interface.  
All Layer 2 nodes expose a standardized JSON-RPC API.  
Layer 3 applications are clients of this API — they do not participate in Layer 2 consensus.

### 9.5.3 Security Boundaries

The security guarantee of each layer is:

**Layer 1:** Secure as long as honest validators control more than 2/3 of staked weight.  
**Layer 2 (ZK rollups):** Secure as long as Layer 1 is secure and zk-STARK proofs are sound.  
**Layer 2 (payment channels):** Secure as long as at least one party remains online during the challenge period.  
**Layer 3:** No protocol-level security guarantees — security depends on implementation quality.

This is why Layer 3 is not considered part of the core protocol.  
Applications built on Layer 3 are responsible for their own security.  
The protocol's security guarantees extend only to Layer 1 and, derivatively, to Layer 2.

---

## 9.6 Architecture Summary

| Property | Layer 1 | Layer 2 | Layer 3 |
|---|---|---|---|
| Primary role | Settlement | Execution | Adoption |
| Throughput target | 1,000–5,000 TPS | 100,000+ TPS | Unlimited |
| Finality | ~4 minutes | ~4 minutes (anchored) | N/A |
| Smart contracts | No | Yes (RVM) | N/A |
| Privacy | Commitment-based | ZK-proof native | N/A |
| State model | UTXO | Account + UTXO hybrid | N/A |
| Consensus | PoS + PoC | Inherited from L1 | N/A |
| Node requirements | Full / Validator | Operator node | Light client |
| Security source | Staked validators | ZK proofs + L1 | Implementation quality |

The three-layer architecture ensures that RupeeCoin can simultaneously be:  
the most secure settlement layer possible,  
the most capable execution environment possible,  
and the most accessible consumer product possible —  
without any layer compromising the others.
