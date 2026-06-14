# Section 12 — Security Analysis

> Security is not a feature.  
> It is the absence of successful attacks.  
> This section does not claim RupeeCoin is unbreakable.  
> It defines the threat model, analyzes each threat category rigorously,  
> and demonstrates that the protocol's design makes each attack  
> either cryptographically infeasible, economically irrational, or both.

---

## 12.1 Security Philosophy

RupeeCoin's security model is built on four principles:

**Defense in depth.**  
No single mechanism provides all security. Multiple independent layers must fail simultaneously  
for an attacker to succeed. The cryptographic layer, the consensus layer, the economic layer,  
and the network layer each provide independent security guarantees.

**Economic alignment.**  
The cost of attacking the network must exceed the benefit of a successful attack  
by a margin large enough to deter rational adversaries.  
Irrational adversaries — those willing to attack at a loss — are addressed through  
cryptographic and network-level defenses that make attacks expensive regardless of motive.

**Minimal trust.**  
Security guarantees should rest on the fewest possible trust assumptions.  
A system that is secure only if a specific set of actors behaves honestly  
is not a decentralized system — it is a distributed system with extra steps.

**Fail-safe defaults.**  
When components fail, they should fail in ways that preserve safety.  
The protocol should never be in a state where an attacker's action produces  
a worse outcome than doing nothing.

---

## 12.2 Threat Model

We consider adversaries in the following categories:

**Rational adversary.**  
Acts to maximize economic gain. Will attack if expected value of attack > expected cost.  
Will not attack if the cost exceeds the benefit, even if attack is technically possible.

**Byzantine adversary.**  
Acts arbitrarily, potentially at economic loss, to disrupt the protocol.  
May be motivated by competitive interests, ideological opposition, or state-level objectives.

**Quantum adversary.**  
Possesses a cryptographically relevant quantum computer.  
Can run Shor's algorithm against elliptic curve cryptography.  
Cannot efficiently break post-quantum cryptographic primitives.

**Network adversary.**  
Controls a significant portion of network infrastructure.  
Can delay, reorder, or drop messages between nodes.  
Cannot forge valid cryptographic signatures or proofs.

**Colluding adversary.**  
Controls a coalition of validators and/or token holders  
working together toward a common malicious objective.

---

## 12.3 Consensus Layer Security

### 12.3.1 The 33% Attack Threshold

RupeeCoin's PoS + Casper FFG consensus requires a Byzantine fault tolerance threshold of 1/3.

**Liveness attack (preventing finality):**  
An adversary controlling >1/3 of staked weight can prevent the protocol from achieving finality  
by withholding attestations. The chain continues to produce blocks but no block is finalized.

**Safety attack (finalizing conflicting blocks):**  
An adversary controlling >2/3 of staked weight can finalize conflicting blocks,  
enabling double-spending. This is the catastrophic failure case.

**Economic cost of 33% attack:**  
At any meaningful level of protocol adoption, acquiring 1/3 of staked supply requires:
- Purchasing 1/3 of all staked RPC on open markets.
- Absorbing the market price impact of this purchase (significant upward price pressure against the attacker).
- Staking the acquired tokens and waiting through the activation queue.
- Accepting the risk that the attack is detected and the stake is slashed.

The expected cost is a substantial fraction of the protocol's total market capitalization.  
The expected value of a successful liveness attack is near zero — the attacker cannot profit from preventing finality.  
The expected value of a successful safety attack is bounded by what can be double-spent in one finalized epoch.

For a rational adversary: cost >> benefit.  
For a Byzantine adversary with sufficient resources: the attack is possible but the stake is destroyed upon detection.

### 12.3.2 Slashing as Deterrent

Slashing makes equivocation — the primary attack vector — catastrophically expensive for the attacker.

An attacker who attempts to finalize two conflicting blocks must sign conflicting attestations.  
Conflicting attestations are the slashing evidence.  
Any network participant can submit slashing evidence.  
Upon verification, 100% of the attacker's stake is destroyed.

The attacker's stake destruction has two security effects:
1. It punishes the attacker directly.
2. It reduces the total staked supply, making future attacks more expensive relative to the remaining supply.

An attacker who fails a safety attack loses their entire stake.  
This makes safety attacks negative expected value for any rational adversary.

### 12.3.3 Long-Range Attack Analysis

A long-range attack attempts to rewrite history from a point far in the past,  
using keys from validators that have since exited and unstaked.

**Why this is harder than it appears:**

Old validator keys have no current stake.  
Building an alternative chain from an old checkpoint requires producing valid blocks  
at every slot from that point forward.  
Without stake, the attacker's blocks have no attestations from the current active validator set.  
A node syncing from genesis would never accept a chain with no current validator attestations.

The weak subjectivity mechanism requires new nodes to obtain a recent checkpoint  
(within the last 28 days — the unstaking delay) from a trusted source before syncing.  
A chain diverging more than 28 days ago has no legitimate path to acceptance by current nodes.

**Residual risk:**  
Long-range attacks can mislead newly joining nodes that do not obtain a recent checkpoint.  
This is mitigated through checkpoint distribution via multiple independent sources  
and the protocol's built-in checkpoint mechanism.

### 12.3.4 Nothing-at-Stake

In naive PoS, validators have an incentive to attest to every fork simultaneously  
since there is no cost to attesting to the losing fork.  
This creates instability — validators never commit to a single chain.

RupeeCoin eliminates nothing-at-stake through slashing:  
attesting to conflicting forks produces slashable evidence.  
The cost of attesting to every fork is the certain destruction of the validator's entire stake.  
Rational validators will only attest to the fork they believe will succeed.

---

## 12.4 Cryptographic Security

### 12.4.1 Dilithium Security Analysis

CRYSTALS-Dilithium3 provides NIST security level 3:  
security equivalent to AES-192 against both classical and quantum adversaries.

The security rests on two problems:

**Module Learning With Errors (MLWE):**  
Given a matrix A and vector b = As + e (where s is a secret and e is a small error vector),  
find s. This problem is believed hard for both classical and quantum computers.  
The best known quantum algorithms provide no significant speedup over classical algorithms.

**Module Short Integer Solution (MSIS):**  
Find a short non-zero vector x such that Ax = 0 mod q.  
Similarly believed hard for quantum computers.

No attack on either problem faster than exponential in the security parameter is known.  
The NIST standardization process subjected Dilithium to years of public cryptanalysis  
by the global cryptographic research community. No significant weaknesses were found.

**Key exposure model:**  
Unlike ECDSA, Dilithium does not expose private key information through signature generation.  
An attacker who observes any number of valid signatures for a Dilithium key pair  
gains no information that accelerates private key recovery.  
This is a significant security improvement over ECDSA, which has known vulnerabilities  
to nonce reuse and related attacks.

### 12.4.2 BLAKE3 Security Analysis

BLAKE3 provides 256-bit output with the following security properties:

**Collision resistance:** Finding two inputs with the same output requires ~2^128 operations (Grover-reduced).  
**Preimage resistance:** Finding an input for a given output requires ~2^128 operations.  
**Second-preimage resistance:** Finding a different input with the same output as a given input requires ~2^128 operations.

128-bit post-quantum security is considered sufficient by current NIST standards.  
BLAKE3 inherits the security analysis of BLAKE2, which was a SHA-3 finalist  
subjected to extensive public cryptanalysis. The core construction is well-understood.

### 12.4.3 zk-STARK Security Analysis

zk-STARK security rests entirely on the collision resistance of the underlying hash function (BLAKE3).  
There are no additional cryptographic assumptions — no hardness of discrete logs, no pairing assumptions,  
no trusted setup requirements.

This is the most conservative possible security assumption for a ZK proof system.  
If BLAKE3 is collision-resistant (which requires only that BLAKE3 behaves as a random oracle),  
then zk-STARK proofs are sound: no prover can generate a valid proof for a false statement.

**Soundness:** The probability that a cheating prover can generate a valid proof for an invalid statement  
is negligible — bounded by the security parameter of the hash function.

**Zero-knowledge:** A valid proof reveals nothing about the private inputs beyond what is  
explicitly revealed in the public inputs (in RupeeCoin's case: the fee amount and the nullifiers).

---

## 12.5 Network Layer Security

### 12.5.1 Eclipse Attacks

An eclipse attack isolates a node from the honest network  
by surrounding it with adversary-controlled peers.  
The isolated node receives only the adversary's view of the chain.

**Mitigations:**
- Nodes maintain diverse peer connections across multiple IP ranges and ASNs.
- Inbound and outbound connection limits are balanced to prevent monopolization by a single peer group.
- Nodes cross-validate chain state from multiple peers before accepting it.
- Light clients can validate block headers and Merkle proofs independently of peer trust.

### 12.5.2 Sybil Attacks

A Sybil attack floods the network with adversary-controlled nodes  
to gain disproportionate influence over peer selection and message propagation.

**Mitigations:**
- Validator status requires staked RPC — a financial commitment that makes Sybil attacks expensive.
- Non-validator nodes have no influence on consensus — they can propagate messages but cannot vote.
- Peer selection uses diversification heuristics to avoid clustering connections among related IPs.

### 12.5.3 Denial of Service

An adversary floods nodes with invalid transactions or malformed messages  
to exhaust processing resources and prevent legitimate traffic from being processed.

**Mitigations:**
- All transactions require a valid signature before processing. Signature verification is the first step,  
  and Dilithium verification is fast — invalid messages are rejected quickly.
- Transactions below the minimum fee are rejected without mempool entry.  
  Spam transactions cost real RPC.
- Message size limits prevent oversized messages from consuming bandwidth.
- Rate limiting per peer connection prevents a single adversary connection from monopolizing resources.
- Nodes maintain separate processing queues for transactions, blocks, and protocol messages.

### 12.5.4 Transaction Malleability

Transaction malleability — the ability to modify a valid transaction's bytes  
without invalidating the signature, producing a different transaction ID for the same transaction —  
was a significant weakness in early Bitcoin.

RupeeCoin eliminates transaction malleability by design:  
the transaction ID is the BLAKE3 hash of all transaction data including signatures.  
Any modification to any byte of the transaction changes the ID.  
There is no subset of transaction data that can be modified while preserving signature validity.

---

## 12.6 Smart Contract Security

### 12.6.1 RVM Isolation

Each RVM contract executes in an isolated sandbox.  
A contract cannot access the memory or storage of another contract  
except through explicitly defined interface calls.  
A compromised or buggy contract cannot corrupt the state of other contracts  
or the Layer 1 settlement layer.

### 12.6.2 Formal Verification Support

The RVM instruction set is designed to support formal verification.  
Developers can specify the intended behavior of their contracts as formal properties  
and use automated verification tools to prove that the contract code satisfies those properties.

This is a capability the EVM does not provide — EVM's complexity makes  
complete formal verification of non-trivial contracts impractical.

### 12.6.3 Upgrade Mechanisms

Smart contracts are immutable by default — deployed code cannot be changed.  
Immutability is a security property: users can verify the exact code they are interacting with.

Upgradeable contracts require an explicit upgrade mechanism declared at deployment.  
Upgrades are subject to time-locks defined in the contract.  
Users who disagree with an upgrade have the time-lock period to exit.

### 12.6.4 Gas Limits as DoS Prevention

All RVM operations consume gas.  
Transactions specify a maximum gas limit.  
A contract with an infinite loop is terminated when its gas limit is exhausted.  
The sender pays for all gas consumed up to the point of termination.

This makes computational denial-of-service attacks against the RVM economically costly  
and ensures that the RVM never enters an infinite loop that consumes validator resources indefinitely.

---

## 12.7 Human Layer Security

Protocol-level security is necessary but not sufficient.  
The majority of value lost in cryptocurrency systems is lost at the human layer:  
lost keys, stolen credentials, phishing attacks, custodial failures.

RupeeCoin addresses the human layer through protocol-level mechanisms.

### 12.7.1 Social Recovery

Social recovery allows a user to recover access to their funds  
if they lose their private key, without any central authority.

**Mechanism:**
1. The user designates a set of guardians — trusted contacts whose addresses are stored in a recovery contract.
2. If the user loses their key, they can initiate a recovery request from a new address.
3. A threshold of guardians (e.g., 3 of 5) must approve the recovery within a defined window (72 hours default).
4. Upon threshold approval, the recovery contract transfers control of the user's funds to the new address.
5. The original key is invalidated — if the user still has access to the original key, they can cancel the recovery.

**Security properties:**
- No single guardian can steal funds — threshold approval is required.
- The user can cancel a fraudulent recovery during the window if they still have key access.
- Guardians do not need to know each other — each approves independently.
- The recovery contract code is immutable and publicly auditable.

### 12.7.2 Multi-Signature by Default

High-value wallets are encouraged — and enterprise wallets are required — to use multi-signature spending conditions.

A 2-of-3 multisig wallet requires any two of three designated keys to sign a transaction.  
An attacker who compromises one key cannot steal funds.  
A user who loses one key can still access funds with the remaining two.

Multi-signature is a native feature of the Layer 1 script system, not an add-on.

### 12.7.3 Hardware Security Module Support

The SDK supports hardware security modules (HSMs) and hardware wallets  
for key storage and signing.  
Private keys never leave the hardware device.  
The device signs transactions locally in response to user confirmation.

Hardware wallet firmware updates for Dilithium key support will be coordinated  
with major hardware wallet manufacturers as part of the Phase 9 ecosystem development.

---

## 12.8 Security Audit Requirements

Before each major protocol milestone, independent security audits are required:

| Milestone | Audit Scope | Auditor Requirement |
|---|---|---|
| Testnet Alpha | Core chain, consensus, cryptographic primitives | 1 independent firm |
| Public Testnet | Full protocol including ZK layer and RVM | 2 independent firms |
| Mainnet | Complete protocol, all layers | 3 independent firms + academic review |
| Major upgrades | Affected components | 2 independent firms |

Audit reports are published in full, including all findings and their resolutions.  
Critical findings block milestone advancement until resolved.  
High findings require resolution before the milestone is complete.  
Medium and low findings are tracked and resolved within defined timelines.

A permanent bug bounty program is maintained with rewards scaled to finding severity:

| Severity | Reward |
|---|---|
| Critical (consensus break, fund theft) | Up to 500,000 RPC |
| High (significant security degradation) | Up to 100,000 RPC |
| Medium (limited impact vulnerability) | Up to 10,000 RPC |
| Low (minor security issue) | Up to 1,000 RPC |

---

## 12.9 Security Summary

| Threat | Mechanism | Resistance |
|---|---|---|
| 33% consensus attack | Cryptoeconomic cost + slashing | Economically irrational at scale |
| 67% consensus attack | Slashing + stake destruction | Costly + self-destructive |
| Quantum attack on signatures | CRYSTALS-Dilithium | Computationally infeasible |
| Quantum attack on hashing | BLAKE3 (128-bit post-quantum) | Computationally infeasible |
| ZK proof forgery | zk-STARK soundness | Negligible probability |
| Long-range attack | Weak subjectivity + unstaking delay | Impractical |
| Nothing-at-stake | Slashing for equivocation | Economically irrational |
| Eclipse attack | Peer diversity + cross-validation | Significantly mitigated |
| Sybil attack | Stake requirement | Economically costly |
| DoS (transaction spam) | Minimum fee + rate limiting | Economically costly |
| Transaction malleability | ID = full tx hash | Eliminated by design |
| Smart contract exploit | RVM isolation + formal verification | Contained + reduced probability |
| Key loss | Social recovery | Recoverable without central authority |
| Custodial theft | Non-custodial by default | Eliminated for self-custody users |

No protocol can guarantee the absence of all attacks.  
RupeeCoin's security architecture ensures that successful attacks require  
either computational resources beyond current and near-future capability,  
or economic commitments that make attacks negative expected value for rational adversaries.
