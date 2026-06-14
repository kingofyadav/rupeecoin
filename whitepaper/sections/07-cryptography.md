# Section 7 — Cryptographic Foundation

> Cryptography is not an implementation detail.  
> It is the foundation on which every security guarantee in this protocol rests.  
> A protocol that gets cryptography wrong cannot be patched into correctness later.  
> This section defines the cryptographic primitives of RupeeCoin and the reasoning behind every choice.

---

## 7.1 Design Philosophy

RupeeCoin's cryptographic architecture is governed by three principles:

**Quantum-safety from genesis.**  
No cryptographic primitive in the core protocol should be vulnerable to a quantum adversary.  
This is not a future upgrade path. It is a baseline requirement.

**Conservative selection.**  
RupeeCoin uses standardized, peer-reviewed primitives — not novel constructions.  
The NIST Post-Quantum Cryptography standardization process represents the most rigorous public evaluation of post-quantum candidates in history.  
RupeeCoin's selections are drawn from its outputs.

**Minimal surface area.**  
The number of distinct cryptographic primitives in the protocol is minimized.  
Each additional primitive is an additional attack surface, an additional implementation risk, and an additional dependency.

---

## 7.2 The Quantum Threat Model

To understand why each primitive was chosen, the quantum threat must first be precisely defined.

### 7.2.1 Grover's Algorithm

Grover's algorithm provides a quadratic speedup for unstructured search problems on a quantum computer.  
Applied to hash functions, it effectively halves the security parameter:  
a hash function with n-bit output has approximately n/2 bits of quantum security.

Consequence for hash functions:  
A 256-bit hash function provides approximately 128 bits of quantum security.  
128 bits of security is considered sufficient by current cryptographic standards.  
Hash functions with 256-bit output remain viable in the post-quantum era.

### 7.2.2 Shor's Algorithm

Shor's algorithm solves two problems efficiently on a quantum computer:

1. Integer factorization — breaks RSA.
2. The Discrete Logarithm Problem — breaks DSA, ECDSA, and all elliptic curve cryptography.

Consequence for signature schemes:  
Every signature scheme based on elliptic curves — including Bitcoin's secp256k1 ECDSA and Ethereum's identical construction — is broken by a sufficiently powerful quantum computer.  
There is no mitigation. The mathematical hardness assumption is invalidated.

Consequence for key exchange:  
Every Diffie-Hellman based key exchange protocol is similarly broken.

The distinction is critical:  
Grover's algorithm weakens symmetric cryptography and hash functions — they can be defended by increasing key sizes.  
Shor's algorithm breaks asymmetric cryptography based on factoring and discrete logs — there is no parameter size that restores security.

### 7.2.3 The Harvest Now, Decrypt Later Attack

Quantum-vulnerable cryptography presents a threat that precedes Q-Day — the moment a cryptographically relevant quantum computer exists.

An adversary with sufficient resources can record encrypted data and signed transactions today,  
then decrypt them retroactively when quantum hardware becomes available.

Applied to blockchain:  
Every transaction ever signed on Bitcoin or Ethereum is permanently recorded on a public ledger.  
Every signing operation exposes a public key.  
An adversary harvesting public keys today can attempt private key recovery in the future.

This means the relevant question is not:  
*"When will quantum computers break ECDSA?"*  
but rather:  
*"What is the value of assets secured by exposed ECDSA keys, and what is the expected value of a future attack?"*

For a protocol launched today, the answer to the second question must be zero.  
All signatures must be post-quantum from the first block.

---

## 7.3 Digital Signatures — CRYSTALS-Dilithium

### 7.3.1 Selection Rationale

RupeeCoin uses CRYSTALS-Dilithium as its primary digital signature scheme.

Dilithium was selected by NIST as a primary post-quantum signature standard in 2024 (FIPS 204).  
It is the most conservative and well-analyzed post-quantum signature candidate to have completed the NIST standardization process.

The security of Dilithium rests on the hardness of the Module Learning With Errors (MLWE) problem and the Module Short Integer Solution (MSIS) problem over module lattices.  
These problems are believed to be hard for both classical and quantum computers.  
No known quantum algorithm provides meaningful speedup against lattice problems.

Alternative candidates considered:

**FALCON** — Also a lattice-based scheme, standardized by NIST (FIPS 206). Produces smaller signatures than Dilithium but requires floating-point arithmetic during key generation, introducing implementation complexity and potential side-channel vulnerabilities. Rejected as primary scheme due to implementation risk. May be considered for specific constrained applications in later protocol versions.

**SPHINCS+** — A hash-based signature scheme standardized by NIST (FIPS 205). Security rests entirely on hash function security — the most conservative possible assumption. However, signature sizes are significantly larger (8–50 KB depending on parameter set) and signing is slower. Unsuitable as a primary scheme for a high-throughput payment network. Reserved as a fallback if lattice assumptions are unexpectedly broken.

### 7.3.2 Parameter Selection

RupeeCoin uses Dilithium3 parameters (NIST security level 3):

| Parameter | Value |
|---|---|
| Security Level | NIST Level 3 (equivalent to AES-192) |
| Public Key Size | 1,952 bytes |
| Signature Size | 3,293 bytes |
| Private Key Size | 4,000 bytes |
| Security Assumption | MLWE + MSIS |

NIST Level 3 provides security against an adversary with quantum resources equivalent to breaking AES-192.  
This is considered a conservative margin for a protocol expected to operate for decades.

Dilithium5 (NIST Level 5, AES-256 equivalent) provides higher security at larger key and signature sizes.  
The protocol reserves the right to migrate to Dilithium5 parameters via governance if the threat model warrants it.

### 7.3.3 Size Implications and Mitigation

The primary cost of post-quantum signatures is size.  
ECDSA signatures are 64 bytes. Dilithium3 signatures are 3,293 bytes — approximately 51 times larger.

This has direct implications for block size, storage requirements, and bandwidth.

RupeeCoin addresses this through:

**Signature aggregation.**  
Where multiple inputs in a transaction require separate signatures, batch verification techniques reduce the per-signature verification cost.

**Layer 2 signature compression.**  
The majority of high-frequency transactions occur on Layer 2, where signatures are managed within payment channels and do not require individual Layer 1 settlement.

**Optimized block structure.**  
The Layer 1 block format is designed with Dilithium signature sizes as a baseline requirement, not as an afterthought.

**Storage pruning.**  
Spent transaction outputs can be pruned from full node storage. Signature data for pruned outputs does not need to be retained indefinitely.

### 7.3.4 Key Derivation

RupeeCoin uses a hierarchical deterministic (HD) key derivation scheme adapted for Dilithium.  
A single seed (256 bits of entropy) derives a tree of key pairs.  
This allows a single backup phrase to recover all keys in a wallet.

The derivation path follows a structure analogous to BIP-32/44 but adapted for lattice-based key material:  
`m / purpose / coin_type / account / change / address_index`

Seed entropy is generated using a CSPRNG seeded from hardware entropy sources.  
Minimum seed entropy: 256 bits.

---

## 7.4 Hash Functions — BLAKE3

### 7.4.1 Selection Rationale

RupeeCoin uses BLAKE3 as its primary hash function, replacing SHA-256.

BLAKE3 provides:

**Quantum resistance.**  
With 256-bit output, BLAKE3 provides approximately 128 bits of security against Grover's algorithm.  
This meets the quantum security requirement for hash functions.

**Performance.**  
BLAKE3 is significantly faster than SHA-256 on modern hardware, particularly on processors with SIMD instructions.  
On a modern CPU, BLAKE3 achieves multiple GB/s throughput — 10–20x faster than SHA-256.  
This matters for a high-throughput protocol where hash operations occur millions of times per second across the network.

**Security.**  
BLAKE3 is derived from BLAKE2, which was a finalist in the NIST SHA-3 competition.  
Its security has been extensively analyzed. No significant weaknesses have been found.

**Simplicity.**  
BLAKE3 is a single function that serves as a hash function, PRF, MAC, KDF, and XOF.  
This reduces the number of distinct cryptographic constructions in the protocol.

SHA-3 (Keccak) was also considered.  
SHA-3 is a NIST standard with strong security properties.  
BLAKE3 was preferred due to its significantly superior performance characteristics.

### 7.4.2 Usage in Protocol

BLAKE3 is used for:

- Block header hashing
- Transaction identifier (TXID) computation
- Merkle tree construction
- Key derivation (as KDF)
- Address derivation from public keys
- Proof-of-Work nonce hashing (if applicable to specific protocol components)
- Commitment schemes in the ZK layer

### 7.4.3 Merkle Tree Construction

RupeeCoin uses a binary Merkle tree with BLAKE3 as the hash function.  
The construction follows the standard approach: leaf nodes are hashes of transaction data, internal nodes are hashes of concatenated child hashes.

To prevent second-preimage attacks on the Merkle tree, leaf nodes and internal nodes use domain-separated hash functions:  
`leaf_hash = BLAKE3("leaf" || data)`  
`internal_hash = BLAKE3("node" || left || right)`

The Merkle root is included in the block header.  
Transaction inclusion proofs (Merkle proofs) allow light clients to verify transaction inclusion without storing the full block.

---

## 7.5 Zero-Knowledge Proofs — zk-STARKs

### 7.5.1 The Role of ZK-Proofs in RupeeCoin

Zero-knowledge proofs allow one party to prove to another that a statement is true without revealing any information beyond the truth of the statement.

In RupeeCoin, ZK-proofs serve two purposes:

**Transaction privacy.**  
A sender proves that a transaction is valid — that inputs exist, are unspent, and the sum of inputs equals the sum of outputs plus fees — without revealing the sender address, receiver address, or transaction amount.

**Computational integrity.**  
ZK-proofs allow Layer 2 execution results to be verified on Layer 1 without re-executing the computation.  
This is the foundation of the rollup architecture.

### 7.5.2 zk-STARKs vs zk-SNARKs

Two major families of practical ZK-proof systems exist: zk-SNARKs and zk-STARKs.

| Property | zk-SNARKs | zk-STARKs |
|---|---|---|
| Trusted setup required | Yes (most constructions) | No |
| Proof size | Small (hundreds of bytes) | Larger (tens to hundreds of KB) |
| Verification time | Fast | Moderate |
| Quantum resistance | No (pairing-based) | Yes (hash-based) |
| Security assumption | Elliptic curve pairings | Collision-resistant hash functions |

RupeeCoin uses zk-STARKs.

The primary reason is quantum resistance.  
zk-SNARKs based on elliptic curve pairings inherit the same quantum vulnerability as ECDSA.  
A quantum adversary that can break ECDSA can also break pairing-based SNARKs.

The secondary reason is the elimination of trusted setup.  
zk-SNARKs typically require a trusted setup ceremony — a multi-party computation that generates public parameters.  
If the ceremony is compromised, an adversary can forge proofs undetectably.  
A monetary network should not have a single catastrophic failure mode that depends on the integrity of a one-time ceremony.

zk-STARKs require only a collision-resistant hash function — a cryptographic primitive that is both quantum-resistant and well-understood.  
The security assumptions are minimal and conservative.

The cost is proof size.  
STARK proofs are larger than SNARK proofs.  
This is an acceptable tradeoff for a protocol where security assumptions must survive decades of cryptographic advances.

### 7.5.3 Transaction Privacy Model

A private RupeeCoin transaction proves the following statements in zero knowledge:

1. The inputs reference valid, existing, unspent output commitments in the UTXO set.
2. The sum of input values equals the sum of output values plus the transaction fee.
3. The sender possesses the private key corresponding to the input commitments.
4. The output commitments are correctly formed and spendable only by the intended recipient.

None of the following are revealed in a standard private transaction:
- Sender address or public key
- Receiver address or public key
- Transaction amount
- Link between input and output

### 7.5.4 Selective Disclosure

RupeeCoin supports selective disclosure — the ability for a user to voluntarily reveal transaction details to a chosen third party.

This serves legitimate use cases:  
tax reporting, regulatory compliance in regulated environments, audit trails for enterprise users.

Selective disclosure is implemented via viewing keys:  
a cryptographic key that allows the holder to decrypt transaction details for a specific address without the ability to spend funds.

Selective disclosure is user-initiated and voluntary.  
No protocol-level mechanism forces disclosure.  
The protocol does not implement backdoors for regulatory access — this would defeat the privacy guarantee for all users, including the majority with entirely legitimate purposes.

---

## 7.6 Address Format

A RupeeCoin address is derived from a Dilithium public key as follows:

```
address = Base58Check( version_byte || BLAKE3( public_key )[0:20] )
```

The public key hash provides an additional layer of security:  
the public key is not revealed until the address is first used to sign a transaction.  
An unspent, never-signed address has only the hash of the public key on-chain.  
Even if Dilithium were somehow broken, coins in never-spent addresses would retain protection from the hash layer until a transaction is signed.

Best practice: use each address only once.  
The wallet implementation generates a new address for each receive operation by default.

---

## 7.7 Cryptographic Agility

Cryptography advances. Assumptions that are conservative today may become inadequate in the future.

RupeeCoin's protocol design supports cryptographic agility — the ability to introduce new cryptographic primitives via the governance process without requiring a contentious hard fork.

The mechanism:

- Algorithm identifiers are encoded in transaction and key formats.
- New signature schemes or hash functions can be added as supported algorithms via governance vote.
- Old algorithms are deprecated over a defined migration window.
- Users migrate to new algorithms before the deprecation deadline.

This is how Bitcoin should have been designed.  
The inability to migrate Bitcoin's cryptography without a hard fork is a direct consequence of the absence of cryptographic agility in the original design.

---

## 7.8 Summary

| Component | Primitive | Standard | Quantum-Safe |
|---|---|---|---|
| Digital Signatures | CRYSTALS-Dilithium3 | NIST FIPS 204 | ✅ |
| Hash Function | BLAKE3 | — | ✅ |
| ZK Proofs | zk-STARKs | — | ✅ |
| Key Derivation | BLAKE3-based HD scheme | — | ✅ |
| Address Derivation | BLAKE3 truncated hash | — | ✅ |

Every cryptographic primitive in the core protocol is quantum-resistant.  
This is not a future upgrade. It is the baseline.
