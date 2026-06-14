# Section 1 — Abstract

The global financial system fails the majority of the people it claims to serve.
It is slow, expensive, exclusionary, and structurally dependent on the trust of institutions
that have repeatedly demonstrated they cannot be trusted unconditionally.
Bitcoin proposed a solution in 2008: a peer-to-peer electronic cash system
secured by cryptographic proof rather than institutional trust.
The proposal was correct. The implementation, designed for the computational environment of 2008,
is now showing the limits of its era.

We propose RupeeCoin: a decentralized monetary network designed to address
the full set of problems that Bitcoin and Ethereum leave unsolved.

RupeeCoin makes four primary technical contributions:

**First**, a post-quantum cryptographic foundation.
RupeeCoin replaces the Elliptic Curve Digital Signature Algorithm —
vulnerable to Shor's algorithm on a sufficiently powerful quantum computer —
with CRYSTALS-Dilithium, a lattice-based signature scheme standardized by NIST in 2024.
All hashing uses BLAKE3. Zero-knowledge proofs use zk-STARKs.
Every cryptographic primitive in the core protocol is quantum-resistant from the first block.
No future migration is required.

**Second**, a privacy model that treats financial privacy as a fundamental right rather than a premium feature.
Standard transactions in RupeeCoin hide the sender, receiver, and amount
through zero-knowledge proofs built on zk-STARKs.
Privacy requires no trusted setup and is secure against quantum adversaries.
Selective disclosure allows users to reveal transaction details to chosen parties
for compliance, audit, or personal purposes — voluntarily, without protocol-level backdoors.

**Third**, a three-layer architecture that achieves global-scale throughput
without sacrificing the security or decentralization of the settlement layer.
Layer 1 provides quantum-resistant, PoS-secured final settlement at 1,000–5,000 TPS.
Layer 2 provides programmability through the RupeeCoin Virtual Machine and payment channels,
targeting 100,000+ TPS.
Layer 3 provides consumer and developer access through mobile-first SDKs and merchant tooling.

**Fourth**, a perpetual validator incentive model that solves Bitcoin's long-term security problem.
A Perpetual Micro-Fee Pool, funded by a fraction of every transaction fee,
provides ongoing validator compensation independent of block subsidy schedules.
Combined with a fee burn mechanism, the protocol achieves long-term supply equilibrium
that responds to actual network usage.

RupeeCoin is governed entirely on-chain through a four-category proposal system
with anti-capture mechanisms, time-locks, and hardcoded constitutional constraints.
No founder holds special governance privileges.
The endgame is a protocol owned by no individual, corporation, or government —
governed by its community, secured by mathematics, open to the world.

RupeeCoin originates in India —
home to 1.4 billion people, the world's largest developer population,
and a payment culture that already operates at the scale this protocol targets.
It is built for India first and the world immediately after.

The remainder of this paper specifies the protocol in full technical detail.
