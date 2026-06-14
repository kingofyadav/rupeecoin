# Section 3 — Problems With Existing Systems

> The purpose of this section is not criticism for its own sake.  
> It is to define, with precision, the exact gaps that RupeeCoin is designed to fill.  
> Every problem stated here corresponds directly to a design decision made later in this paper.

---

## 3.1 The Failure of Traditional Finance

The global financial system was not designed for the internet age.  
It was designed for a world of physical branches, paper ledgers, and national borders.  
It has been patched, extended, and digitized — but never rebuilt.

The consequences are structural, not accidental.

### 3.1.1 Exclusion by Design

Approximately 1.4 billion adults worldwide remain unbanked as of 2024.  
The majority are concentrated in South Asia, Sub-Saharan Africa, and Southeast Asia.  
In India alone, hundreds of millions of citizens have historically operated outside the formal financial system.

The cause is not lack of technology. It is the cost structure of trust-based intermediaries.  
Banks require physical infrastructure, identity verification, minimum balances, and ongoing compliance overhead.  
For low-income populations, the cost of being served exceeds the revenue a customer generates.  
Exclusion is therefore a rational business decision within the existing model — not a failure, but a feature.

### 3.1.2 The Cost of Intermediation

Every transaction in the traditional financial system passes through one or more intermediaries:  
acquiring banks, issuing banks, payment networks, clearinghouses, correspondent banks.

Each layer extracts a fee.  
Cross-border remittances — critical for families across the developing world — carry average fees of 6–8%.  
A migrant worker sending earnings home loses a significant fraction to infrastructure that adds no value to the transfer itself.

Beyond fees, intermediaries introduce latency.  
Domestic transfers that appear instant often settle on a T+1 or T+2 basis at the interbank level.  
International transfers can take 3–5 business days.  
In a world where information moves in milliseconds, money moves at the speed of 1970s infrastructure.

### 3.1.3 Reversibility and Fraud Surface

The traditional system is built on reversibility.  
Chargebacks, disputes, and fraud protection are features from the consumer perspective.  
From the system perspective, they are liabilities that must be priced into every transaction.

Merchants pay for fraud they did not commit.  
Consumers pay through higher prices.  
The system as a whole allocates significant resources to mediating disputes rather than processing value.

### 3.1.4 Surveillance and Control

Access to the financial system is contingent on compliance with state and institutional requirements.  
Accounts can be frozen. Transactions can be blocked. Access can be revoked.

In stable democracies with strong rule of law, this power is exercised with restraint.  
In unstable regimes, it becomes a tool of political control.  
Financial access should not be a privilege that governments can revoke.  
It should be a right that mathematics guarantees.

---

## 3.2 The Limitations of Bitcoin

Bitcoin solved the most important problem in digital payments:  
how to transfer value between two parties without requiring a trusted intermediary.

The solution — a decentralized chain of cryptographic proof — was a genuine intellectual breakthrough.  
Fifteen years of operation have proven the core concept sound.

However, Bitcoin was designed in 2008.  
The threat landscape, the computational environment, and the scale requirements of 2024 are fundamentally different.  
Bitcoin's limitations are not bugs — they are the consequences of design decisions made with the information available at the time.

### 3.2.1 Quantum Vulnerability

Bitcoin's security rests on two cryptographic assumptions:

1. SHA-256 is a one-way function — outputs cannot be used to reconstruct inputs.
2. The Elliptic Curve Discrete Logarithm Problem (ECDLP) is computationally hard — a public key cannot be used to derive its private key.

The first assumption remains robust against quantum attack.  
Grover's algorithm provides a quadratic speedup against hash functions, effectively reducing SHA-256 to 128-bit security on a quantum computer — still computationally infeasible to break.

The second assumption does not survive quantum computing.  
Shor's algorithm solves the discrete logarithm problem in polynomial time on a sufficiently powerful quantum computer.  
This means that a quantum computer with enough logical qubits can derive a private key from any exposed public key.

In Bitcoin, a public key is exposed every time a transaction is signed.  
Every address that has ever sent a transaction has an exposed public key recorded permanently on the blockchain.  
Those coins are vulnerable to a future quantum attacker who can harvest the public key today and decrypt it when quantum hardware becomes capable.

The timeline is uncertain but no longer distant.  
In April 2026, a 15-bit elliptic curve key was broken using publicly accessible quantum hardware — a 512-fold improvement over the previous benchmark from September 2025.  
Google has internally committed to post-quantum cryptography migration across its infrastructure by 2029.  
Research from the University of Sussex estimates that 317 logical qubits would be sufficient to break a Bitcoin transaction within one hour.

Bitcoin has no post-quantum migration path that does not require a contentious hard fork,  
the cooperation of every wallet holder,  
and the resolution of the problem of Satoshi's coins — which cannot be migrated by any living person.

A protocol designed today has no excuse for this vulnerability.

### 3.2.2 Throughput

Bitcoin processes approximately 7 transactions per second at the base layer.  
Visa processes approximately 24,000 transactions per second.  
The global financial system processes orders of magnitude more.

The 1MB block size limit, combined with a 10-minute block time, creates a hard ceiling on base-layer throughput.  
During periods of high demand, this produces fee spikes that price out small transactions entirely.

The Lightning Network addresses this for simple payments but introduces its own limitations:  
channel liquidity requirements, routing failures, and the need for online presence to receive payments.  
It does not solve the base-layer throughput problem — it defers it.

A monetary network intended for global adoption cannot process fewer transactions per second than a city.

### 3.2.3 Privacy

Bitcoin transactions are pseudonymous, not private.  
Every transaction — sender address, receiver address, amount, timestamp — is permanently recorded on a public ledger.

Pseudonymity provides weak protection.  
Address clustering, chain analysis, and exchange KYC requirements allow sophisticated observers to de-anonymize transaction graphs with high accuracy.  
Commercial blockchain analytics firms routinely trace transaction histories for law enforcement and other clients.

Financial privacy is not primarily about hiding criminal activity.  
It is about the same principle that makes sealed ballots, private medical records, and confidential legal communication fundamental to a free society.  
A world in which every financial transaction is permanently and publicly visible is a world with structural capacity for financial surveillance at a scale no government has previously possessed.

### 3.2.4 The Incentive Collapse Problem

Bitcoin's security model depends on miners expending computational resources to extend the chain.  
Miners are compensated through two mechanisms: block subsidies and transaction fees.

The block subsidy halves approximately every four years.  
By approximately 2140, the subsidy reaches zero.  
At that point, transaction fees must provide sufficient incentive for miners to continue securing the network.

Whether fee revenue will be sufficient is an open economic question with no definitive answer.  
If it is not sufficient, hash rate declines.  
If hash rate declines sufficiently, the cost of a 51% attack decreases.  
Bitcoin's long-term security model contains an unresolved assumption.

### 3.2.5 No Programmability

Bitcoin's scripting language is intentionally limited.  
It supports multisignature transactions, timelocks, and basic conditional logic — but not general computation.

This was a deliberate security decision: a limited attack surface.  
The consequence is that Bitcoin cannot support smart contracts, decentralized finance, token issuance, or programmable monetary policy.  
Bitcoin is a store of value and a payment rail. It is not a programmable platform.

The global financial system requires programmability:  
payroll automation, escrow, insurance, lending, derivatives.  
A monetary network that cannot express these operations natively cannot replace the infrastructure it aims to supersede.

---

## 3.3 The Limitations of Ethereum

Ethereum extended Bitcoin's vision with a critical addition:  
a Turing-complete virtual machine that allows arbitrary computation on a decentralized network.

This was a genuine conceptual advance.  
Ethereum demonstrated that decentralized programmable money was possible and created an ecosystem of applications — DeFi, NFTs, DAOs, stablecoins — that proved the concept at scale.

However, Ethereum was not designed as a currency protocol.  
It was designed as a general computation platform.  
This distinction produces structural limitations that affect its fitness as a monetary network.

### 3.3.1 Quantum Vulnerability

Ethereum uses ECDSA over the secp256k1 curve — the same signature scheme as Bitcoin.  
It inherits the identical quantum vulnerability.

Ethereum has announced post-quantum research as a strategic priority and has a dedicated Post-Quantum team.  
Vitalik Buterin has identified validator signatures, wallet signatures, and certain zero-knowledge proofs as areas of concern.

However, no production post-quantum upgrade has been deployed.  
Every Ethereum address that has ever signed a transaction exposes a public key to future quantum attack.  
Retrofitting post-quantum cryptography into a live network with hundreds of billions of dollars in assets is an extraordinarily complex engineering and coordination problem.

### 3.3.2 Fee Volatility

Ethereum's gas fee market — even after EIP-1559 — produces extreme fee volatility during periods of high demand.  
Base fees during network congestion have regularly exceeded $50–$200 per transaction for simple operations.

A monetary network with unpredictable transaction costs cannot serve as a medium of exchange.  
A merchant cannot price goods in a currency whose transaction fee may exceed the value of a small purchase.  
Fee predictability is a basic requirement of a functioning monetary system.

### 3.3.3 Complexity as Attack Surface

Ethereum's generality is a security liability at the base layer.  
The EVM supports arbitrary computation. Arbitrary computation has arbitrary attack surface.

Smart contract exploits have cost the Ethereum ecosystem billions of dollars.  
The DAO hack, the Parity multisig freeze, countless DeFi protocol exploits — these are not anomalies.  
They are the expected consequence of deploying complex, general-purpose code to manage financial assets without the ability to patch or roll back.

A monetary base layer should be minimal by design.  
Complexity belongs in the layers above it, where errors are recoverable.

### 3.3.4 Not Designed as Currency

Ethereum's monetary policy has changed multiple times.  
The transition from proof-of-work to proof-of-stake altered the issuance model.  
EIP-1559 introduced fee burning, creating deflationary pressure under certain conditions.

These changes reflect the reality that Ethereum's monetary policy is governance-driven and subject to revision.  
For a store of value, predictability of monetary policy is as important as security of the cryptography.  
A monetary network whose supply schedule is determined by committee decisions introduces a class of risk that a fixed-policy protocol does not.

---

## 3.4 The Problem With Central Bank Digital Currencies

Central Bank Digital Currencies (CBDCs) represent the state's response to the growth of decentralized finance.  
Several major economies — China, the European Union, India — have active CBDC programs at various stages of development.

CBDCs solve some real problems: payment system modernization, financial inclusion, cross-border settlement efficiency.  
They do not solve — and in fact deliberately worsen — the fundamental problem with the existing financial system.

### 3.4.1 Surveillance by Architecture

A CBDC is a liability of the central bank, recorded on infrastructure controlled by the state.  
Every transaction is observable, attributable, and storable by the issuing authority.

This is not a side effect. It is the point.  
The ability to monitor financial flows is a primary motivation for CBDC development from the perspective of state actors.  
A CBDC is the existing surveillance capacity of the financial system, extended, modernized, and made total.

### 3.4.2 Programmable Control

CBDCs introduce the technical possibility of programmable money controlled by the issuer:  
expiry dates on currency, geographic restrictions on spending, automatic tax collection, conditional access based on social or political criteria.

Whether these capabilities are used benevolently or maliciously depends entirely on the character of the issuing government.  
The architecture makes them possible regardless.  
Sound monetary infrastructure should not make authoritarian financial control technically trivial.

### 3.4.3 Single Point of Failure

A CBDC network is operated by a central authority.  
It inherits all the failure modes of centralized infrastructure:  
system outages, cyberattacks, political interference, institutional capture.

Decentralization is not an ideological preference.  
It is an engineering property that eliminates single points of failure.  
A monetary network that can be shut down by attacking one institution has not solved the problem that Bitcoin was designed to address.

---

## 3.5 Summary of Gaps

The problems documented in this section define the requirements for RupeeCoin:

| Problem | Source | RupeeCoin Response |
|---|---|---|
| Quantum vulnerability of ECDSA | Bitcoin, Ethereum | CRYSTALS-Dilithium from genesis |
| 7 TPS throughput ceiling | Bitcoin | 3-layer architecture, 100,000+ TPS |
| Pseudonymity, not privacy | Bitcoin, Ethereum | zk-STARKs, private by default |
| Incentive collapse at zero subsidy | Bitcoin | Perpetual micro-fee pool |
| No programmability | Bitcoin | RupeeCoin VM on Layer 2 |
| Gas fee volatility | Ethereum | Predictable fee market design |
| Complexity as attack surface | Ethereum | Minimal Layer 1, complexity on L2 |
| Mutable monetary policy | Ethereum | Fixed supply, governance-protected |
| Financial surveillance | CBDCs | No central authority, ZK privacy |
| Programmable state control | CBDCs | Protocol-level censorship resistance |
| Single point of failure | CBDCs | Full decentralization |
| Financial exclusion | Traditional finance | Permissionless by design |

Each row in this table is a design constraint.  
The remainder of this whitepaper describes how each constraint is satisfied.
