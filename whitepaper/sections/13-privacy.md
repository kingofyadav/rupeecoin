# Section 13 — Privacy Model

> Privacy is not about hiding wrongdoing.  
> Privacy is the ability to selectively reveal yourself to the world.  
> A monetary system without privacy is a surveillance system with payment functionality.  
> This section defines RupeeCoin's privacy model:  
> what it protects, how it protects it, what it does not protect,  
> and why every design decision was made.

---

## 13.1 Why Privacy Matters in a Monetary Network

Consider the information revealed by a complete, public record of all your financial transactions:  
your employer, your salary, your rent, your medical expenses, your political donations,  
your religious contributions, your family relationships, your business dealings,  
your struggles, your vices, your values.

This is not an abstract concern.  
Bitcoin and Ethereum expose exactly this information — not to a government, not to a bank,  
but to every person on earth with an internet connection and a blockchain explorer.

The traditional banking system achieves privacy through access control:  
your transaction history is visible to your bank and, with legal process, to the state.  
It is not visible to the general public.  
Bitcoin eliminated the trusted intermediary but eliminated the privacy that came with it.

RupeeCoin's position: **financial privacy is a fundamental right.**  
A monetary network should provide stronger privacy guarantees than the traditional banking system,  
not weaker ones.

---

## 13.2 Privacy Threat Model

We consider four categories of privacy adversary:

**Passive observer.**  
Reads the public blockchain. Applies chain analysis techniques to de-anonymize transactions.  
Does not control any network infrastructure.  
This is the threat model Bitcoin fails against — chain analysis firms can trace the majority of Bitcoin transactions.

**Active network adversary.**  
Controls network infrastructure and can observe transaction propagation.  
Can correlate a transaction's origin IP address with its first broadcast.  
Can analyze timing patterns to infer network topology.

**Targeted adversary.**  
Specifically investigates a known address or a known individual.  
May have access to off-chain information — KYC records from exchanges, public address associations.  
Combines on-chain and off-chain data to build a complete picture.

**State-level adversary.**  
Has access to exchange KYC data through legal process.  
Can compel disclosures from regulated entities.  
May operate network-level surveillance infrastructure.

RupeeCoin's privacy model is designed to protect against the passive observer and significantly raise the cost for all other adversary classes.  
It does not claim to provide perfect anonymity against a state-level adversary with unlimited resources and off-chain data.  
No decentralized protocol can make this claim honestly.

---

## 13.3 What Is Protected

In a standard private transaction (Type 1 — see Section 10):

**Protected:**
- Sender address and public key
- Receiver address and public key
- Transaction amount
- Link between input and output (transaction graph)
- Whether the sender and receiver have transacted before
- The sender's total balance (not inferable from the transaction)
- The receiver's total balance received in private transactions

**Not protected:**
- The fact that a transaction occurred (a nullifier appears on-chain)
- The transaction fee (fees are public — validators need to verify them)
- The timing of the transaction (block timestamp is public)
- Network-level metadata (IP address of broadcast origin — mitigated but not eliminated)

For transparent transactions (Type 0):

**Protected:** Nothing. All transaction details are public.  
**Not protected:** Everything.

Transparent transactions exist because:
1. Some use cases legitimately require transparency (public treasuries, verifiable payroll, on-chain governance).
2. Private transaction proof generation has a cost. Transparent transactions are cheaper and faster.
3. Users should have the choice.

---

## 13.4 The ZK-STARK Privacy Construction

### 13.4.1 Commitment Scheme

RupeeCoin uses a Pedersen-variant commitment scheme adapted for post-quantum security.

A commitment to a value v with randomness r is:  
`C(v, r) = BLAKE3(v || r)`

The commitment hides v (a computationally hiding commitment: given C, v cannot be recovered without r).  
The commitment binds v (a computationally binding commitment: it is infeasible to find v' ≠ v such that C(v, r) = C(v', r') for any r').

When a private output is created, the output commitment is recorded on-chain.  
The value v and randomness r are transmitted to the recipient through an encrypted channel  
(encrypted to the recipient's public key, included as an encrypted memo in the transaction).

### 13.4.2 Nullifier Scheme

When a private output is spent, a nullifier is derived from the output's secret data:  
`nullifier = BLAKE3(spending_key || output_commitment)`

The nullifier is revealed on-chain when the output is spent.  
It proves that some output was spent without identifying which output.

The mapping from nullifier to commitment is one-way: given a nullifier, it is computationally  
infeasible to determine which output it corresponds to without knowledge of the spending key.

The nullifier set — all nullifiers ever revealed — is maintained on-chain.  
Before including a private transaction, validators verify that none of its nullifiers are already in the set.

### 13.4.3 The zk-STARK Proof

The privacy transaction's zk-STARK proof proves the following statements:

**Statement 1 — Input validity:**  
"I know a spending key sk and randomness r such that:  
BLAKE3(sk.public_key || r) = commitment C  
and C exists in the commitment tree with Merkle root R."

This proves the input exists and the spender owns it without revealing which commitment is being spent.

**Statement 2 — Double-spend prevention:**  
"The nullifier N was derived as BLAKE3(sk || C)  
and I know the sk that produces this nullifier."

This links the nullifier to the commitment in a way that can be verified  
without revealing the commitment or the key.

**Statement 3 — Value conservation:**  
"The sum of input values equals the sum of output values plus the fee."

This proves no value was created from nothing without revealing any individual value.

**Statement 4 — Output well-formedness:**  
"Each output commitment was correctly constructed as BLAKE3(v || r)  
and the encrypted memo is correctly encrypted to the recipient's public key."

This proves the recipient can decrypt and spend the output.

All four statements are proven simultaneously in a single zk-STARK proof.  
The proof is verified by validators as part of transaction validation.  
Verification is fast — a few milliseconds per transaction.  
Proof generation is the expensive operation (1–3 seconds on mobile, <0.5 seconds on desktop).

---

## 13.5 Selective Disclosure

RupeeCoin supports three levels of selective disclosure:

### 13.5.1 Transaction Disclosure

A user can reveal the details of a specific transaction to a chosen party  
by sharing the transaction's viewing key:  
`tx_viewing_key = BLAKE3(spending_key || tx_hash)`

The tx_viewing_key allows the recipient to:
- Decrypt the transaction's amounts and addresses
- Verify the transaction occurred on-chain

The tx_viewing_key does not allow:
- Spending the outputs
- Viewing other transactions

Use case: proving a specific payment to a counterparty or auditor.

### 13.5.2 Address Viewing Key

A user can generate an address-level viewing key that decrypts  
all incoming transactions to a specific address:  
`address_viewing_key = derive(spending_key, address_index)`

The address_viewing_key allows the recipient to:
- View all incoming transactions to the address
- See amounts and sender commitments (but not sender identities)
- Compute the address balance

The address_viewing_key does not allow:
- Spending any outputs
- Viewing outgoing transactions

Use case: tax reporting, exchange compliance, enterprise accounting.

### 13.5.3 Full Account Disclosure

A user can provide full account disclosure by sharing both  
the incoming and outgoing viewing keys for their account.  
This reveals the complete transaction history of the account  
in a form that can be verified against the on-chain record.

Use case: full financial audit, regulatory compliance in jurisdictions where this is required.

**Critical design principle:**  
Selective disclosure is always user-initiated.  
No protocol mechanism can force disclosure.  
No backdoor exists for regulatory access to private transactions without user cooperation.  
The protocol treats all users equally — privacy is a right, not a privilege.

---

## 13.6 Network-Level Privacy

On-chain privacy protects against analysis of the blockchain record.  
Network-level privacy protects against analysis of transaction propagation.

### 13.6.1 Dandelion++ Protocol

RupeeCoin uses the Dandelion++ protocol for transaction propagation.

Standard broadcast: a transaction is immediately sent to all peers.  
An adversary observing network traffic can identify the originating node as the first broadcaster.

Dandelion++:
1. **Stem phase.** When a transaction is first broadcast, it is forwarded to a single randomly selected peer,  
   not to all peers. This single-peer forwarding continues for a random number of hops (the "stem").
2. **Fluff phase.** After the stem phase, the transaction enters the standard broadcast (the "fluff"),  
   propagating to all peers.

An observer trying to identify the originating node sees only the node that transitioned  
from stem to fluff — not the node that originated the transaction.  
The random stem length makes it statistically difficult to trace the transaction back to its origin.

### 13.6.2 Tor and I2P Compatibility

The RupeeCoin node implementation supports routing peer connections through Tor and I2P.  
Users who require stronger network-level privacy can configure their nodes to operate as hidden services.  
This hides the node operator's IP address from their peers.

Network-level privacy via Tor is optional and slightly reduces performance (higher latency).  
It is not required for standard use.

---

## 13.7 Privacy Comparison

| Property | Bitcoin | Monero | Zcash | RupeeCoin |
|---|---|---|---|---|
| Sender hidden | ❌ | ✅ | ✅ (shielded) | ✅ |
| Receiver hidden | ❌ | ✅ | ✅ (shielded) | ✅ |
| Amount hidden | ❌ | ✅ | ✅ (shielded) | ✅ |
| Transaction graph hidden | ❌ | ✅ | ✅ (shielded) | ✅ |
| Quantum resistant | ❌ | ❌ | ❌ | ✅ |
| Trusted setup required | N/A | N/A | Yes (Sapling) | ❌ |
| Selective disclosure | ❌ | ✅ (limited) | ✅ | ✅ |
| Privacy by default | ❌ | ✅ | ❌ (opt-in) | ✅ (Type 1 default) |
| Scalable proofs | N/A | Moderate | Yes (SNARK) | Yes (STARK) |

**vs Monero:**  
Monero uses ring signatures and stealth addresses to provide strong privacy.  
However, Monero's ring signature scheme is not quantum-resistant.  
RupeeCoin's zk-STARK construction provides equivalent or stronger privacy  
with quantum resistance and better scalability.

**vs Zcash:**  
Zcash's shielded transactions use zk-SNARKs, which are not quantum-resistant  
and required a trusted setup ceremony.  
RupeeCoin uses zk-STARKs: no trusted setup, quantum-resistant.  
Zcash's privacy is opt-in — the majority of Zcash transactions are transparent.  
RupeeCoin's Type 1 private transaction is the recommended default.

---

## 13.8 Privacy and Compliance

A common objection to protocol-level privacy is that it enables illicit activity.  
This objection deserves a direct response.

**The empirical evidence:**  
The overwhelming majority of illicit financial activity occurs through the traditional financial system,  
not through privacy-preserving cryptocurrencies.  
Cash — which provides stronger privacy than any cryptocurrency — has not been eliminated  
because it enables illicit activity, because the privacy it provides to billions of legitimate users  
outweighs the marginal benefit to bad actors who have many other options.

**The design reality:**  
RupeeCoin's privacy model does not prevent law enforcement from obtaining information  
through legal process directed at exchanges, service providers, and counterparties.  
It prevents passive surveillance — the ability to monitor anyone's financial activity  
without specific cause, without legal process, and without the subject's knowledge.  
This is the privacy protection that matters for legitimate users.

**Selective disclosure as a compliance tool:**  
Regulated entities — exchanges, payment processors, enterprises — can use  
RupeeCoin's selective disclosure mechanisms to meet their compliance obligations  
while preserving their users' privacy from unrelated parties.

**The protocol is neutral.**  
RupeeCoin provides privacy as infrastructure.  
What is built on that infrastructure — and whether it complies with applicable laws —  
is the responsibility of the builders and users, not of the protocol.

---

## 13.9 Privacy Limitations

Honest documentation of what RupeeCoin's privacy model does not protect:

**Transaction existence.**  
The fact that a private transaction occurred is visible on-chain (a nullifier is revealed, an output commitment is added).  
The total number of private transactions is public.

**Timing.**  
Transaction timestamps are public. Timing analysis can sometimes reveal patterns.

**Fee amounts.**  
Transaction fees are public. An unusually high fee on a private transaction is visible.

**Network-level metadata (residual).**  
Dandelion++ significantly reduces but does not eliminate network-level traceability.  
A well-resourced adversary monitoring a significant fraction of network nodes  
may be able to trace transaction origins despite Dandelion++.

**Off-chain information.**  
If a user publicly associates their address with their identity,  
their address's transaction history becomes known.  
RupeeCoin cannot protect against voluntary self-deanonymization.

**Exchange and service provider data.**  
Users who acquire RPC through a KYC exchange have a known entry point.  
A targeted adversary with access to that exchange's records can determine  
when a user acquired RPC and for how much.  
On-chain privacy begins at the point of self-custody — it does not retroactively  
hide the existence of the exchange account.

---

## 13.10 Privacy Summary

RupeeCoin's privacy model provides:

- **Strong on-chain privacy** for private transactions via zk-STARKs — sender, receiver, and amount are hidden.
- **Quantum-resistant privacy** — the ZK construction is secure against quantum adversaries.
- **No trusted setup** — privacy does not depend on the integrity of a one-time ceremony.
- **Selective disclosure** — users can prove transaction details to chosen parties without revealing everything.
- **Network-level privacy** — Dandelion++ reduces transaction origin traceability.
- **Default privacy** — private transactions are the recommended default, not an opt-in feature.

Privacy is not an add-on to RupeeCoin.  
It is a core design property — present from the first block, available to every user,  
requiring no special action to activate.  
Because in a monetary network, the default is what matters.  
And the default should be private.
