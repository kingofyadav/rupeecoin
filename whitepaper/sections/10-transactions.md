# Section 10 — Transaction Model

> A transaction is the atomic unit of the protocol.  
> Everything else — blocks, chains, consensus, validators —  
> exists to serve one purpose: ensuring that a valid transaction,  
> once submitted, is recorded correctly and permanently.  
> This section defines what a transaction is, how it is constructed,  
> how it moves through the network, and how it achieves finality.

---

## 10.1 Transaction Types

RupeeCoin defines four transaction types at the protocol level.  
Each type has a distinct structure, validation rules, and fee schedule.

### 10.1.1 Type 0 — Transfer Transaction
A direct transfer of RPC between two addresses on Layer 1.  
The simplest transaction type. No smart contract interaction.

### 10.1.2 Type 1 — Private Transfer Transaction
A transfer of RPC with ZK-proof privacy.  
Sender, receiver, and amount are hidden.  
The transaction proves validity in zero knowledge without revealing transaction details.

### 10.1.3 Type 2 — Contract Interaction Transaction
An interaction with a smart contract on Layer 2 (RVM).  
Includes contract address, function selector, input data, and gas parameters.

### 10.1.4 Type 3 — Protocol Transaction
A special transaction type for protocol-level operations:  
validator registration, staking, unstaking, governance voting, channel open/close, rollup batch submission.  
Protocol transactions have reduced fees and are prioritized in block inclusion.

---

## 10.2 Transaction Structure

### 10.2.1 Type 0 — Transfer Transaction

```
TransferTransaction {
    // Header
    version:        u8,           // Protocol version
    type:           u8,           // = 0
    chain_id:       u32,          // Network identifier (prevents replay across forks)
    nonce:          u64,          // Sender's transaction count — prevents replay
    
    // Inputs
    inputs: Vec<TransactionInput> {
        utxo_id:    Blake3Hash,   // 32 bytes — identifies the UTXO being spent
        sequence:   u32,          // Timelock sequence number
    },
    
    // Outputs
    outputs: Vec<TransactionOutput> {
        value:      u64,          // Amount in satoshi-equivalent units (10^-8 RPC)
        recipient:  Address,      // 25 bytes — BLAKE3 hash of recipient public key
        script:     Script,       // Spending condition
    },
    
    // Fees
    base_fee:       u64,          // Protocol-determined minimum fee
    priority_fee:   u64,          // Optional tip to validator
    
    // Signatures
    signatures: Vec<DilithiumSignature>,  // One per input (~3293 bytes each)
    
    // Metadata
    expiry:         u64,          // Block height after which tx is invalid (0 = no expiry)
}
```

### 10.2.2 Type 1 — Private Transfer Transaction

```
PrivateTransferTransaction {
    // Header
    version:        u8,
    type:           u8,           // = 1
    chain_id:       u32,
    
    // ZK-proof components
    nullifiers:     Vec<Blake3Hash>,    // Input commitment nullifiers
    output_commitments: Vec<Commitment>, // Encrypted output commitments
    binding_sig:    DilithiumSignature, // Proves ownership of inputs
    
    // ZK proof
    proof:          STARKProof,   // zk-STARK proof of transaction validity
    
    // Public inputs to the proof
    merkle_root:    Blake3Hash,   // UTXO tree root at time of transaction
    fee:            u64,          // Fee is public (required for validator compensation)
    
    // Memo (optional, encrypted)
    encrypted_memo: Option<Vec<u8>>,
}
```

The nullifier prevents double-spending of private inputs:  
each private UTXO has a unique nullifier that is revealed when spent.  
The nullifier reveals that some UTXO was spent, but not which one.  
Once a nullifier appears on-chain, the corresponding UTXO cannot be spent again.

### 10.2.3 Fee Structure

Every transaction pays a fee composed of:

```
total_fee = base_fee + priority_fee

base_fee:
    30% → Perpetual Micro-Fee Pool
    70% → burned (removed from supply)

priority_fee:
    100% → block proposer validator
```

The base fee is set by the protocol's dynamic fee adjustment mechanism.  
It adjusts each block based on the previous block's utilization (see Section 11).

The priority fee is optional. Transactions with zero priority fee are valid but may experience  
longer confirmation times during periods of high block utilization.

---

## 10.3 Transaction Lifecycle

A transaction passes through five stages from creation to finality.

```
[CREATED] → [SIGNED] → [BROADCAST] → [MEMPOOL] → [INCLUDED] → [FINALIZED]
```

### 10.3.1 Stage 1 — Creation

The sender constructs the transaction:

1. Identify UTXOs to spend (inputs).
2. Specify outputs (recipient addresses and amounts).
3. Calculate the required base fee from the current fee oracle.
4. Optionally set a priority fee for faster inclusion.
5. Set the nonce (for replay protection).
6. Optionally set an expiry block height.

For private transactions, the sender additionally:
1. Generates output commitments for each recipient.
2. Computes nullifiers for each input.
3. Generates the zk-STARK proof of transaction validity.

Proof generation is the most computationally intensive step for private transactions.  
On a modern mobile device, proof generation takes approximately 1–3 seconds.  
On a desktop device, approximately 0.1–0.5 seconds.  
Hardware acceleration for proof generation will be supported in the Layer 3 SDK.

### 10.3.2 Stage 2 — Signing

The sender signs the transaction with their Dilithium private key.

For Type 0 transactions with multiple inputs, one signature is required per input.  
Each signature covers: the transaction hash + the specific input being signed.

The transaction hash is:
```
tx_hash = BLAKE3(version || type || chain_id || nonce || inputs || outputs || fees || expiry)
```

The chain_id in the hash prevents a signed transaction on the RupeeCoin mainnet  
from being replayed on a testnet or a forked chain.

### 10.3.3 Stage 3 — Broadcast

The signed transaction is broadcast to the peer-to-peer network.  
The sender submits it to one or more known nodes.  
Each receiving node validates the transaction and, if valid, forwards it to its peers.

**Initial validation on receipt:**
- Transaction version is supported.
- Transaction is not already in the mempool or confirmed chain.
- All input UTXOs exist in the current UTXO set.
- All input UTXOs are unspent.
- Signature(s) are valid.
- Input values ≥ output values + fees.
- Base fee ≥ current protocol minimum.
- Transaction size is within protocol limits.
- Script conditions are well-formed.

Transactions that fail initial validation are dropped and not forwarded.  
This prevents the network from propagating invalid transactions.

### 10.3.4 Stage 4 — Mempool

Valid transactions wait in the mempool — a local, unordered pool of pending transactions maintained by each node.

**Mempool management:**
- Maximum mempool size: configurable per node (default: 300 MB)
- When the mempool is full, transactions with the lowest priority fee per byte are evicted.
- Transactions that have been in the mempool for more than 72 hours without confirmation are evicted.
- Expired transactions (block height > expiry field) are evicted immediately.

**Transaction ordering:**
When a validator builds a block, it selects transactions from the mempool ordered by priority fee per byte, descending.  
This creates the fee market: users who need faster confirmation pay higher priority fees.

### 10.3.5 Stage 5 — Inclusion and Finality

A validator includes the transaction in a proposed block.  
The block is attested by the attestation committee.  
When the block achieves finality (approximately 4 minutes after inclusion), the transaction is final.

**Finality is absolute.**  
A transaction in a finalized block cannot be reversed, modified, or excluded retroactively.  
This is a cryptoeconomic guarantee: reverting a finalized block requires destroying more than 1/3 of the total staked supply.

---

## 10.4 UTXO Set Management

The UTXO set is the complete record of all unspent transaction outputs.  
It is the authoritative state of the protocol.

### 10.4.1 UTXO Commitment

The UTXO set is committed in each block header via a Merkle tree root.  
The Merkle root allows any node to prove that a specific UTXO exists (or does not exist)  
in the current set with a logarithmic-size proof.

This enables light nodes to verify UTXO existence without downloading the full UTXO set.

### 10.4.2 UTXO Pruning

Spent UTXOs are removed from the active UTXO set.  
Full nodes are not required to retain spent UTXO data beyond the minimum required for  
the slashing evidence window (28 days) and the rollup withdrawal challenge period.

Archive nodes retain the complete UTXO history for all blocks.

### 10.4.3 Private UTXO Set

Private transactions use a separate commitment tree — a sparse Merkle tree of output commitments.  
Spent commitments are tracked through the nullifier set.

The nullifier set contains the nullifier of every spent private output.  
Before including a private transaction, validators check that none of its nullifiers  
appear in the nullifier set. If any nullifier is already present, the transaction is rejected as a double-spend.

---

## 10.5 Transaction Limits and Constraints

| Parameter | Value | Rationale |
|---|---|---|
| Maximum transaction size | 100 KB | Prevents block stuffing with oversized transactions |
| Maximum inputs per transaction | 4,096 | Computational bound on validation time |
| Maximum outputs per transaction | 4,096 | Same |
| Maximum script size | 10,000 bytes | Limits script complexity |
| Minimum fee | Dynamic (base fee oracle) | Prevents spam at zero cost |
| Maximum expiry window | 1,000 blocks (~66 hours) | Limits mempool bloat from long-lived pending transactions |
| Nonce | Sequential per address | Prevents replay and enforces transaction ordering per sender |

---

## 10.6 Fee Estimation

Users and applications need to estimate the appropriate fee before broadcasting a transaction.

RupeeCoin provides a fee oracle interface on all nodes:

```
GET /v1/fee_estimate

Response {
    base_fee_current:     u64,   // Current block's base fee
    base_fee_next:        u64,   // Estimated next block's base fee
    priority_fee_low:     u64,   // Estimated fee for ~10 block confirmation
    priority_fee_medium:  u64,   // Estimated fee for ~3 block confirmation
    priority_fee_high:    u64,   // Estimated fee for next block inclusion
    mempool_depth:        u32,   // Current mempool transaction count
}
```

The SDK uses this interface to provide automatic fee estimation for all transaction types.  
Users can override the estimated fee if they have specific confirmation time requirements.

---

## 10.7 Replay Protection

RupeeCoin implements two layers of replay protection:

**Chain ID.**  
Every transaction includes the network's chain ID.  
A transaction signed for mainnet is invalid on testnet and vice versa.  
A transaction signed before a hard fork is invalid on the post-fork chain if the chain ID changes.

**Nonce.**  
Every address has an associated nonce — the count of transactions sent from that address.  
Each transaction must include the sender's current nonce.  
A transaction with a nonce lower than the current on-chain nonce is rejected as already processed.  
A transaction with a nonce higher than current + 1 is held in the mempool pending the missing nonces.

---

## 10.8 Transaction Summary

| Property | Type 0 (Transfer) | Type 1 (Private) | Type 2 (Contract) | Type 3 (Protocol) |
|---|---|---|---|---|
| Sender visible | Yes | No | Yes | Yes |
| Receiver visible | Yes | No | Yes | Yes |
| Amount visible | Yes | No | Yes | Yes |
| ZK proof required | No | Yes | No | No |
| Layer 1 settlement | Yes | Yes | Via rollup | Yes |
| Typical size | ~4 KB | ~15 KB | ~5 KB | ~4 KB |
| Finality | ~4 minutes | ~4 minutes | ~4 minutes | ~4 minutes |

The transaction model is designed to be simple enough to reason about formally,  
flexible enough to support all required use cases,  
and efficient enough to achieve the throughput targets of a global monetary network.
