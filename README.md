# RupeeCoin (RPC)

> **Post-quantum. Privacy-native. Built in India. Owned by no one.**

RupeeCoin is a from-scratch Layer-1 blockchain written entirely in Rust. It combines
quantum-resistant cryptography (CRYSTALS-Dilithium3 signatures, Kyber768 note
encryption), a shielded UTXO privacy layer, Casper FFG finality, VRF-chained
proposer selection, HD key derivation, and EIP-1559-style fee markets — all over a
libp2p gossipsub peer-to-peer network. The design targets UPI-scale throughput while
remaining auditable, decentralised, and resistant to future quantum adversaries.

---

## Table of Contents

- [Why RupeeCoin](#why-rupeecoin)
- [Architecture](#architecture)
- [Cryptographic Primitives](#cryptographic-primitives)
- [Workspace Layout](#workspace-layout)
- [Phase Roadmap](#phase-roadmap)
- [Quick Start](#quick-start)
  - [Prerequisites](#prerequisites)
  - [Build](#build)
  - [Run a Node](#run-a-node)
  - [Bootstrap from a Checkpoint](#bootstrap-from-a-checkpoint)
  - [Two-Node Local Testnet](#two-node-local-testnet)
  - [Wallet CLI](#wallet-cli)
- [Node CLI Reference](#node-cli-reference)
- [Wallet CLI Reference](#wallet-cli-reference)
- [HTTP JSON-RPC API](#http-json-rpc-api)
- [Privacy Model](#privacy-model)
- [Consensus — Casper FFG + VRF](#consensus--casper-ffg--vrf)
- [Economics — EIP-1559 + PMFP + Inactivity Leak](#economics--eip-1559--pmfp--inactivity-leak)
- [HD Key Derivation](#hd-key-derivation)
- [Weak Subjectivity Checkpoints](#weak-subjectivity-checkpoints)
- [Security Model](#security-model)
- [Future Roadmap](#future-roadmap)
- [Testnet Faucet](#testnet-faucet)
- [Benchmarks](#benchmarks)
- [Desktop Wallet](#desktop-wallet)
- [Mobile Wallet](#mobile-wallet)
- [Contributing](#contributing)

---

## Why RupeeCoin

| Goal | Design choice |
|---|---|
| Quantum resistance | Dilithium3 signing; Kyber768 note encryption |
| Privacy by default | Shielded UTXO pool; Kyber-encrypted notes; BLAKE3 nullifiers |
| Decentralised | Proof-of-Stake (Casper FFG); no trusted setup; open validator set |
| Deterministic wallets | BLAKE3 HD derivation (m/0/i receive, m/1/0 validator) |
| Sustainable fee market | EIP-1559 base fee; PMFP public-goods fund; inactivity leak |
| Born in India | Genesis message; UPI-scale ambition; permissionless access |
| Auditable | All cryptography is open-source; whitepaper public |

**Priority order**: Security ≻ Decentralisation ≻ Privacy ≻ Scalability.

---

## Architecture

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                           Application Layer                                  │
│   rupeecoin-wallet CLI ─────────────────────── External dApps / tooling      │
│          │                                              │                    │
│          └──────────────── HTTP JSON-RPC ───────────────┘                   │
└──────────────────────────────────┬───────────────────────────────────────────┘
                                   │  port 9944 (axum)
┌──────────────────────────────────▼───────────────────────────────────────────┐
│                           Execution Layer                                    │
│  rupeecoin-node (tokio)                                                      │
│  ┌───────────┐ ┌────────────┐ ┌────────────┐ ┌───────────┐ ┌─────────────┐  │
│  │  Mempool  │ │  Casper    │ │ Validator  │ │  Privacy  │ │  Checkpoint │  │
│  │ (4096 tx) │ │  FFG       │ │ Registry   │ │  RPC      │ │  Manager    │  │
│  └─────┬─────┘ └─────┬──────┘ └─────┬──────┘ └─────┬─────┘ └─────┬───────┘  │
│        └─────────────┴──────────────┴───────────────┴─────────────┘         │
└──────────────────────────────────┬───────────────────────────────────────────┘
                                   │
┌──────────────────────────────────▼───────────────────────────────────────────┐
│                          Settlement Layer                                    │
│  rupeecoin-blockchain ──── Chain { UTXO set, CommitmentTree, PmfpAccumulator}│
│         │                                                                    │
│  rupeecoin-storage ──────── RocksDB (9 column families)                      │
│         │                                                                    │
│  rupeecoin-network ──────── libp2p (gossipsub + request-response)            │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Block anatomy

```
Block
├── BlockHeader
│   ├── version, prev_hash, merkle_root, timestamp, height
│   ├── slot, proposer_index           ← Casper FFG consensus (Phase 3)
│   ├── privacy_root                   ← shielded pool commitment (Phase 4)
│   ├── base_fee, gas_used             ← EIP-1559 fee market (Phase 6)
│   ├── state_root                     ← Merkle over sorted UTXO set (Phase 7)
│   └── vrf_output                     ← VRF-chained proposer entropy (Phase 8)
├── transactions[]                     ← transparent UTXO (coinbase first)
├── attestations[]                     ← Casper FFG votes
├── proposer_signature                 ← Dilithium3 over block hash
├── vrf_proof                          ← Dilithium3 VRF proof
└── privacy_txs[]                      ← shielded transactions (Phase 4)
    └── PrivacyTransaction
        ├── anchor                     ← commitment-tree root (must be recent)
        ├── shielded_inputs[]          ← nullifier + spend proof
        ├── shielded_outputs[]         ← note commitment + Kyber-encrypted note
        ├── fee                        ← public, in micro-RPC
        └── balance_proof              ← Σin = Σout + fee
```

---

## Cryptographic Primitives

| Purpose | Algorithm | Notes |
|---|---|---|
| Block / tx hashing | BLAKE3 | domain-separated leaf/node hashing |
| Address derivation | Base58Check(0x52 ∥ BLAKE3(pk)[0:20]) | 20-byte hash, version byte 0x52 |
| Merkle trees (transparent + shielded) | BLAKE3 binary tree | domain-separated |
| Signing (validators, transfers) | CRYSTALS-Dilithium3 | PK=1952 B, SK=4032 B, Sig=3309 B |
| VRF proof | Dilithium3 sig over (domain ∥ prev_vrf_output ∥ slot) | chained proposer entropy |
| HD key derivation | BLAKE3 `derive_key` XOF (64 B) | m / account / index path |
| Note encryption (KEM) | CRYSTALS-Kyber768 | post-quantum KEM |
| Note symmetric encryption | XChaCha20-Poly1305 | AEAD |
| Note commitment | BLAKE3(value ∥ recipient ∥ rcm) | hiding + binding |
| Nullifier | BLAKE3(nullifier_key ∥ cm) | spend linkage prevention |
| Payment address | BLAKE3(nullifier_key ∥ kyber_pk) | receiver identity |
| Keystore KDF | Argon2id | passphrase → 32-byte wrapping key |
| Keystore encryption | XChaCha20-Poly1305 | v3 keystore format |
| State root | BLAKE3 Merkle over sorted (OutPoint → TxOutput) | per-block UTXO commitment |
| Spend proof (Phase 4) | BLAKE3 Merkle path | reveals note commitment (gap closed in Phase 5B) |
| Spend proof (Phase 5B) | zk-STARK (Winterfell) | planned — hides commitment |

---

## Workspace Layout

```
rupeecoin/
├── Cargo.toml                       # workspace root — 19 members
├── core/
│   ├── crypto/                      # Hash, Signature, KeyPair, HdNode, VrfProof
│   │   ├── build.rs                 # cc build for Dilithium3 seeded-keypair C shim
│   │   └── c/dilithium3_seeded.c    # PQClean internal-symbol shim
│   ├── types/                       # Block, Transaction, UTXO, Attestation structs
│   ├── utxo/                        # in-memory UTXO set + state_root
│   ├── blockchain/                  # Chain, genesis, build_block, validation
│   ├── storage/                     # RocksDB store (8 column families)
│   ├── mempool/                     # in-memory tx pool (4096 cap, EIP-1559 eviction)
│   ├── network/                     # libp2p gossipsub + request-response
│   ├── staking/                     # ValidatorRegistry, stake queue, metrics
│   ├── consensus/                   # Casper FFG, select_proposer (VRF), CheckpointManager
│   ├── privacy/                     # Note, CommitmentTree, SpendProof, PrivacyTransaction
│   ├── zkproof/                     # Winterfell STARK prover/verifier (wired to chain)
│   ├── economics/                   # supply schedule, EIP-1559, PMFP, inactivity leak
│   └── rvm/                         # 64-bit stack VM, gas metering, RPC-20/721/1155, evm2rvm
├── node/
│   └── src/
│       ├── main.rs                  # tokio event loop, P2P, block production
│       └── rpc.rs                   # axum HTTP JSON-RPC server
├── wallet/
│   └── src/
│       ├── main.rs                  # clap CLI (generate/address/receive/scan/send/claim)
│       ├── keystore.rs              # v3 keystore: Argon2id + XChaCha20-Poly1305
│       ├── scanner.rs               # trial-decryption wallet scanner
│       └── node_rpc.rs              # ureq HTTP client for node RPC
├── explorer/                        # block explorer: axum REST API + dark-mode SPA
├── dashboard/                       # validator dashboard: ratatui TUI
├── faucet/                          # testnet faucet: axum web service + dark-mode SPA
│   └── examples/fund_faucet.rs      # one-shot tool: fund faucet from validator seed
├── sdk-js/                          # TypeScript SDK (types, codec, rpc, tx, signer)
├── sdk-py/                          # Python SDK (PyO3 + maturin wheel)
├── rupeecoin-sdk/                   # Rust SDK (Wallet, RpcClient, build_transfer)
├── rupeecoin-tests/                 # integration tests + Criterion benchmark suite
├── desktop-wallet/                  # Tauri 2 desktop wallet (Rust backend + HTML SPA)
│   ├── src/                         # keystore.rs, wallet_commands.rs, main.rs
│   ├── frontend/index.html          # dark-mode SPA (unlock, portfolio, send, receive)
│   └── icons/                       # RGBA app icons
├── mobile-wallet/                   # Flutter + flutter_rust_bridge mobile wallet
│   ├── rust/                        # Rust crypto bridge (cdylib: generate/unlock/advance)
│   │   └── src/api.rs               # 6 #[frb(sync)] functions, FRB-annotated
│   ├── lib/                         # Flutter Dart UI (welcome, portfolio, receive, settings)
│   └── android/                     # Android project (minSdk 24, arm64-v8a + x86_64)
└── whitepaper/                      # 16-section whitepaper
```

### RocksDB column families

| Column family | Key | Value |
|---|---|---|
| `blocks` | block hash (32 B) | `bincode(Block)` |
| `heights` | height BE u64 (8 B) | block hash (32 B) |
| `utxos` | `bincode(OutPoint)` | `bincode(TxOutput)` |
| `meta` | `"tip_hash"` / `"height"` / `"note_count"` | raw bytes |
| `nullifiers` | nullifier hash (32 B) | block height BE u64 |
| `note_cmts` | leaf index BE u64 | `bincode(NoteCommitment)` |
| `shielded_outs` | leaf index BE u64 | `bincode(ShieldedOutput)` |
| `checkpoints` | height BE u64 | `bincode(WeakSubjectivityCheckpoint)` |
| `contract_storage` | `addr_u64_le(8B)++slot_u64_le(8B)` | `u64_le(8B)` |

---

## Phase Roadmap

| Phase | Name | Status |
|---|---|---|
| 0 | Whitepaper | **Complete** — 16 sections |
| 1 | Research Prototype | **Complete** — BLAKE3, Dilithium3, chain, genesis |
| 2 | Alpha Network | **Complete** — RocksDB storage, mempool, libp2p P2P, two-node testnet |
| 3 | Validator Framework + PoS | **Complete** — Casper FFG, Dilithium3 proposer signing, slashing |
| 4 | Privacy Layer | **Complete** — Kyber768 KEM, note commitments, nullifiers, shielded pool |
| 5A | Wallet CLI + Node RPC | **Complete** — `rupeecoin-wallet`, axum RPC, scanner, send |
| 6 | Economics | **Complete** — EIP-1559 fee market, PMFP fund, inactivity leak, supply schedule |
| 7 | Transaction Hardening | **Complete** — chain_id, nonce, expiry, Address type, state root, keystore v3 |
| 8 | Protocol Hardening | **Complete** — Winterfell STARKs, reward claims, slashing reports, LMD-GHOST |
| 9.1 | Block Explorer | **Complete** — axum REST + dark-mode SPA |
| 9.2 | Validator Dashboard | **Complete** — ratatui TUI |
| 9.3 | JavaScript / TypeScript SDK | **Complete** — types, codec, rpc, tx, signer, browser demo |
| 9.4 | Python SDK | **Complete** — PyO3 + maturin wheel |
| 9.5 | Testnet Faucet | **Complete** — rate-limited web service, dark-mode SPA, keyfile persistence |
| 9.6 | Benchmarking Suite | **Complete** — 4 Criterion groups; ~800 blocks/s, ~290K UTXOs/s state_root |
| 10.1 | Desktop Wallet GUI | **Complete** — Tauri 2, webkit2gtk-4.1, dark-mode SPA |
| 10.2 | Mobile Wallet Alpha | **Complete** — Flutter + FRB bridge, 5/5 Rust tests |
| 10 | Public Testnet (Zeta) | **In Progress** — external validators, security audit |
| 11.1 | Poseidon Commitment Tree | **Complete** — Goldilocks field, incremental updates |
| 11.2 | Batched STARK Verification | **Complete** — up to 10 shielded inputs per aggregated proof |
| 11.3 | Incremental Merkle Root | **Complete** — eliminates O(N log N) state_root bottleneck |
| 11.4 | Mobile Prover | **Complete** — arm64 native + WASM-compatible |
| 11.5 | Output Well-Formedness | **Complete** — STARK statement 4 |
| 12 | RupeeCoin VM | **Complete** — RVM interpreter, RPC-20/721/1155, EVM transpiler |
| 13 | Mainnet Preparation | **Complete** — genesis config, Tor/SOCKS5, hardware wallet, SDK v1.0 |
| 14 | Mainnet Launch Hardening | **Complete** — all 7 security checklist items; 37 suites green |
| 15 | Ecosystem Growth | **Next** — engineering carry-overs + validator onboarding + grants |

---

## Quick Start

### Prerequisites

- Rust **1.73+** stable (`rustup update stable`)
- A C compiler — required for the Dilithium3 seeded-keypair shim (`gcc` or `clang`)

### Build

```bash
git clone https://github.com/rupeecoin/rupeecoin
cd rupeecoin
cargo build --release
```

Release binaries land in `target/release/`:
- `rupeecoin-node`
- `rupeecoin-wallet`

```bash
# Run all tests
cargo test --workspace
```

### Run a Node

```bash
# Observer node — syncs chain, serves RPC, does not produce blocks
./rupeecoin-node --data-dir /var/lib/rupeecoin

# Validator node — 10,000 RPC stake, joins proposer rotation
./rupeecoin-node \
  --data-dir /var/lib/rupeecoin \
  --stake    10000

# Full options
./rupeecoin-node \
  --data-dir   ./chain-data               \
  --listen     /ip4/0.0.0.0/tcp/30333     \
  --peer       /ip4/192.168.1.10/tcp/30333 \
  --stake      10000                       \
  --rpc-port   9944
```

### Bootstrap from a Checkpoint

For new nodes joining a live network, skip full history validation using a
published weak-subjectivity checkpoint:

```bash
./rupeecoin-node \
  --data-dir   ./chain-data                           \
  --peer       /ip4/<bootnode-ip>/tcp/30333            \
  --stake      10000                                   \
  --checkpoint 604800:<block_hash_hex>:<state_root_hex>
```

The node trusts all blocks up to `height` (verifying the block hash and state
root at the boundary), then validates every subsequent block normally. The chain
publishes a new checkpoint every 604,800 slots (~28 days).

### Two-Node Local Testnet

```bash
# Terminal 1 — node A
cargo run -p rupeecoin-node -- \
  --data-dir ./node-a \
  --listen   /ip4/127.0.0.1/tcp/30333 \
  --stake    10000 \
  --rpc-port 9944

# Terminal 2 — node B (dials A)
cargo run -p rupeecoin-node -- \
  --data-dir ./node-b \
  --listen   /ip4/127.0.0.1/tcp/30334 \
  --peer     /ip4/127.0.0.1/tcp/30333 \
  --stake    10000 \
  --rpc-port 9945
```

Both nodes discover each other, exchange blocks, and start producing the chain
in lock-step via VRF-chained proposer rotation.

### Wallet CLI

```bash
# 1. Generate a new encrypted wallet (HD master seed + privacy key bundle).
#    You will be prompted for a passphrase.
cargo run -p rupeecoin-wallet -- generate
# → Wallet created: wallet.json
# → Shielded payment address: <base58check>

# 2. Show your current receive address (HD path m/0/index)
cargo run -p rupeecoin-wallet -- address

# 3. Advance to the next HD receive address (generates a fresh address)
cargo run -p rupeecoin-wallet -- receive

# 4. Scan the shielded pool for notes belonging to this wallet
cargo run -p rupeecoin-wallet -- scan --rpc http://127.0.0.1:9944

# 5. Send shielded RPC to any address or rupeecoin: URI
cargo run -p rupeecoin-wallet -- send \
  --rpc    http://127.0.0.1:9944 \
  --to     rupeecoin:<Base58CheckAddress>?amount=1.5 \
  --amount 1.5 \
  --fee    0.001
```

---

## Node CLI Reference

```
rupeecoin-node [FLAGS]

FLAGS:
  --data-dir       <path>                 Chain data directory
                                          [default: ./rupeecoin-data]
  --listen         <multiaddr>            P2P listen address
                                          [default: /ip4/0.0.0.0/tcp/30333]
  --peer           <multiaddr>            Dial this peer on startup (repeatable)
  --stake          <RPC>                  Full-coin stake to join the validator set
                                          (min: 10,000 RPC)
  --rpc-port       <port>                 HTTP JSON-RPC listen port [default: 9944]
  --no-rpc                                Disable the HTTP RPC server
  --block-interval <secs>                 Slot duration override [default: 4]
  --validator-seed <64-char-hex>          Deterministic 32-byte seed for the
                                          validator Dilithium3 keypair.
                                          Use on testnets to obtain a stable,
                                          externally-known address so you can
                                          fund it or transfer coinbase rewards.
                                          Omit on mainnet (random key per run).
                                          Example: --validator-seed cc...cc
  --checkpoint     <height:hash:state>    Weak-subjectivity bootstrap checkpoint.
                                          Format: height:block_hash_hex:state_root_hex
                                          Skips full validation before <height>,
                                          verifies block hash and state root at
                                          the boundary, then validates normally.
                                          Example:
                                          --checkpoint 604800:<hash_hex>:<state_hex>

ENVIRONMENT:
  RUST_LOG=rupeecoin_node=info,rupeecoin_network=debug,rupeecoin_consensus=debug
```

**Staking**: `--stake 10000` meets the 10,000 RPC minimum. The node registers
itself, broadcasts a signed `ValidatorAnnouncement` over gossipsub, and begins
participating in block production and attestation. The node's Dilithium3 signing
key is derived from the HD path `m/1/0` (validator account, index 0).

---

## Wallet CLI Reference

```
rupeecoin-wallet [--keys <path>] <COMMAND>

GLOBAL FLAGS:
  --keys <path>    Keystore file path    [default: wallet.json]

COMMANDS:
  generate   Generate a new key bundle (HD master seed + privacy keys).
             Prompts for a passphrase; encrypts with Argon2id + XChaCha20-Poly1305.

  address    Print the current (next unused) transparent receive address.
             Derives from HD path m/0/<index> without advancing the index.

  receive    Advance the HD receive index and print the new Base58Check address.
             Each call generates a fresh address at m/0/<index+1>.

  scan       Scan the shielded pool for notes owned by this wallet.
               --rpc <url>    Node RPC URL   [default: http://127.0.0.1:9944]

  send       Build and submit a shielded transaction.
               --rpc    <url>     Node RPC URL     [default: http://127.0.0.1:9944]
               --to     <target>  Recipient payment address (Base58Check)
                                  or rupeecoin: URI
               --amount <f64>     Amount in RPC (e.g. 1.5)
               --fee    <f64>     Fee in RPC           [default: 0.001]
```

### Payment URI format

```
rupeecoin:<Base58CheckAddress>?amount=1.5&label=Coffee
```

The wallet's `send` command accepts either a raw address string or a full
`rupeecoin:` URI. An embedded `amount` parameter is used as the default if
`--amount` is not supplied on the command line.

### Coin denomination

| Unit | Value |
|---|---|
| 1 RPC | 100,000,000 micro-RPC |
| 1 micro-RPC | smallest indivisible unit |

---

## HTTP JSON-RPC API

The node exposes a read-mostly HTTP API on `127.0.0.1:9944` (configurable via
`--rpc-port`). All endpoints are `GET` except `POST /rpc/privacy_tx`.

### `GET /rpc/height`

```json
{ "height": 42 }
```

### `GET /rpc/recent_roots`

```json
{
  "roots": ["aabbcc…", "ddeeff…"],
  "commitment_count": 17
}
```

`roots` are the last ≤100 Merkle roots of the shielded commitment tree, most
recent last. Wallets must use one of these as the `anchor` in a spend proof;
roots older than `ANCHOR_DEPTH = 100` blocks are rejected.

### `GET /rpc/shielded_outputs?start=0&count=100`

```json
[
  {
    "index": 0,
    "commitment_hex": "aabbcc…",
    "encrypted_note_bytes": "ddeeff…"
  }
]
```

Paginated list of on-chain shielded outputs. `encrypted_note_bytes` is
hex-encoded `bincode(EncryptedNote)`. The wallet attempts Kyber decryption with
its view secret key; success means the note belongs to this wallet.

### `GET /rpc/merkle_path?index=<N>`

```json
{
  "path": [["sibling_hex", false], ["sibling_hex", true]],
  "anchor": "aabbcc…"
}
```

Authentication path for leaf `N` in the commitment tree. `true` means the node
is a right child at that level. Used to build `SpendProof`.

### `GET /rpc/nullifier_status?hex=<nullifier_hex>`

```json
{ "spent": false }
```

Whether a nullifier is already recorded on-chain. The wallet scanner calls this
for every owned note to exclude already-spent outputs from the spendable balance.

### `POST /rpc/privacy_tx`

Body: hex-encoded `bincode(PrivacyTransaction)`.

```json
{ "status": "accepted" }
```

The node runs `validate_stateless` and `validate_stateful` immediately. At the
next slot boundary the node re-validates against current chain state and includes
still-valid transactions in the next block.

### `GET /rpc/status`

Full node snapshot — useful for dashboards and monitoring.

```json
{
  "height": 42,
  "tip_hash": "aabb…",
  "validator_count": 3,
  "mempool_size": 7,
  "base_fee": 1000,
  "my_address": "aNV16Nqm…"
}
```

### `GET /rpc/utxos?addr=<40-char-hex>`

All unspent transparent UTXOs for a given address (hex-encoded 20-byte address).

```json
[
  { "txid": "aabb…", "vout": 0, "amount_micro": 5000000000 }
]
```

### `POST /rpc/tx`

Submit a signed transparent transaction.

Body: hex-encoded `bincode(Transaction)`.

```json
{ "status": "accepted" }
```

### `GET /rpc/nonce?addr=<hex-or-b58>`

Next expected nonce for a transparent address (number of txs already sent from it).
A fresh address returns `0`. Accepts 40-char hex or Base58Check.

```json
{ "nonce": 3 }
```

---

## Privacy Model

### Two UTXO pools

| Pool | Visibility | Linkability |
|---|---|---|
| Transparent | Public (address, amount) | Fully traceable |
| Shielded | Hidden (amount, recipient) | Phase 4: partial; Phase 5B: full |

### Note lifecycle

```
Sender                                On-chain                  Recipient
  │                                       │                         │
  │── Note::new(value, recipient) ──→     │                         │
  │── note.commitment()           ──→  CommitmentTree               │
  │── EncryptedNote::encrypt()    ──→  ShieldedOutput.enc_note ────→│
  │── note.nullifier(nk)          ──→  (hidden until spend)         │
  │                                       │                         │
  │                                       │  ← Kyber.decrypt(vsk) ──│
  │                                       │  ← nullifier(nk) ────────│
  │                                       │                         │
  │── spend: reveal nullifier     ──→  spent_nullifiers             │
  │── spend proof (Merkle path)   ──→  validated by all nodes       │
```

### What is private (Phase 4 / 5A)

- Note value — hidden inside BLAKE3 commitment + Kyber-encrypted payload
- Recipient identity — hidden inside note commitment + Kyber ciphertext
- Randomness `rcm` — never transmitted in plaintext

### What is public (Phase 4 / 5A)

- Nullifiers (required for double-spend prevention)
- Note commitments (stored in the on-chain tree)
- Public fee
- Input/output count per transaction
- **Note commitment in `SpendProof`** — links spend to deposit; closed in Phase 5B

### Phase 5B — closing the linkability gap

A zk-STARK spend proof (Winterfell AIR) will prove:
1. A note commitment exists in the tree (without revealing the leaf position)
2. The nullifier is correctly derived from that commitment and nullifier key
3. The value is in range `[0, 2⁶⁴)`

…without revealing the commitment, position, value, or recipient.

---

## Consensus — Casper FFG + VRF

| Parameter | Value |
|---|---|
| Slot duration | 4 s |
| Slots per epoch | 32 |
| Min stake | 10,000 RPC |
| Max active validators | 10,000 |
| Unstaking delay | 604,800 slots (~28 days) |
| Justification threshold | ≥ ⅔ attesting stake |
| Finality | Two consecutively justified epochs |
| Proposer selection | VRF-chained: `BLAKE3("vrf-output-v1" ∥ sig ∥ prev_vrf_output ∥ slot)` |

### VRF-chained proposer selection

Each slot's proposer is determined by the previous slot's VRF output:

```
proposer_index = BLAKE3(prev_vrf_output ∥ slot) mod active_count
```

The proposer includes a Dilithium3 VRF proof in the block. Verifiers re-derive
the output and confirm the proposer index matches. This chains randomness
forward without any additional beacon, making future proposers unpredictable
until the previous block is finalised.

Bootstrap blocks (before any validator registers) use a deterministic
`BLAKE3(prev_vrf_output ∥ slot)` fallback — no signed proof is required or
accepted.

### Validator lifecycle

```
Queued ──(activate)──→ Active ──(request_exit)──→ Exiting ──(delay elapsed)──→ Removed
                           └──(slash)──→ Slashed
```

Slashing is triggered by equivocation evidence: a proposer signing two different
blocks in the same slot, or a validator attesting to two different blocks in the
same slot.

### Contribution score multiplier

Effective weight = `stake × multiplier(score)`:

| Score | Multiplier |
|---|---|
| 1.0 | 1.5× |
| 0.8 | 1.1× |
| 0.5 | 1.0× |
| 0.2 | 0.5× |
| 0.0 | 0.5× |

Score = weighted average of uptime (30%), attestation timeliness (40%),
proposal timeliness (20%), propagation quality (10%).

---

## Economics — EIP-1559 + PMFP + Inactivity Leak

### Fee market

The protocol targets `GAS_TARGET = 500` transactions per block
(`GAS_LIMIT = 1000` hard cap). The base fee adjusts ±12.5% per block:

```
next_base_fee = base_fee × (1 + 0.125 × (gas_used − target) / target)
```

Base fees are burned; priority fees go to validators.

### PMFP — Public Monetary Fund Protocol

30% of all base fees flow into the on-chain PMFP accumulator. At each epoch
boundary the accumulated balance is distributed pro-rata across active
validators weighted by their contribution score. This funds public-goods work
without inflation.

### Block reward schedule

Block rewards follow a decreasing emission schedule. The treasury receives a
fixed split of every coinbase, funding protocol development without a pre-mine.

### Inactivity leak

Validators that miss attestations are penalised exponentially. Validators
offline for an extended period drain toward zero, ensuring the active set stays
responsive even when a large fraction goes offline.

---

## HD Key Derivation

RupeeCoin uses a BLAKE3-based BIP-32 analogue. The derivation tree is:

```
master seed (32 random bytes)
  │
  └── HdNode::master(seed)
        │
        ├── m / 0 / i   — receive addresses (transparent, Base58Check)
        │
        └── m / 1 / 0   — validator signing key
```

### Derivation function

```
derive(context, data) = BLAKE3::new_derive_key(context).update(data).finalize_xof()[0..64]

master:   (key, chain_code) = split(derive("rupeecoin-hd-master-v1", seed))
child(i): (key, chain_code) = split(derive("rupeecoin-hd-child-v1", key ∥ chain_code ∥ i_le32))
keypair:  KeyPair::from_seed(key)   — seeded Dilithium3
```

The wallet persists only the 32-byte master seed (encrypted in the v3 keystore).
All child keys are re-derived on load. `HdNode` is intentionally **not
serialisable** — private key material never leaves the process.

### Seeded Dilithium3

The standard `pqcrypto-dilithium` crate does not expose a seeded keypair API.
RupeeCoin ships a thin C shim (`core/crypto/c/dilithium3_seeded.c`) compiled
via the `cc` build dependency. It calls PQClean's internal symbols directly,
enabling fully deterministic key derivation from a 32-byte seed.

---

## Weak Subjectivity Checkpoints

To defend against long-range attacks, the network publishes a
**weak-subjectivity checkpoint** every **604,800 slots** (~28 days at 4 s/slot):

```
WeakSubjectivityCheckpoint {
    height:     u64,
    block_hash: Hash,   // expected block hash at this height
    state_root: Hash,   // expected UTXO Merkle root at this height
}
```

Checkpoints are stored in the `checkpoints` RocksDB column family and published
out-of-band (GitHub releases, community channels). A new node bootstrapping
with `--checkpoint`:

1. Trusts the provided checkpoint and fast-forwards through history, applying
   UTXO and privacy state changes without re-running consensus.
2. Verifies the block hash and state root at the exact boundary height.
3. Validates every subsequent block normally using full consensus rules.

---

## Security Model

### Guarantees

1. **Quantum resistance** — All signing uses Dilithium3; note encryption uses
   Kyber768. Classical ECDSA/RSA are not used anywhere in the protocol.
2. **No trusted setup** — BLAKE3 requires no ceremony. Phase 5B targets
   Winterfell STARKs, which are also transparent (no trusted setup).
3. **Double-spend prevention** — Nullifiers are checked at RPC submission time
   and at block validation time. Spent nullifiers are stored in both RocksDB
   and the in-memory `HashSet<Nullifier>`.
4. **Equivocation detection** — `SlashingDetector` records all proposals and
   attestations per slot; double-signing produces immediate `SlashingEvidence`
   and triggers a stake slash.
5. **Balance soundness** — `BalanceProof` ensures `Σin = Σout + fee` for every
   shielded transaction; all validators verify this before including a tx.
6. **Proposer unpredictability** — VRF-chained selection prevents an adversary
   from predicting more than one slot ahead without controlling the current
   proposer's secret key.
7. **State integrity** — Every block commits a Merkle root over the post-block
   UTXO set (`state_root`). Nodes reject any block whose `state_root` does not
   match their local computation.
8. **Keystore protection** — Wallet keys are stored encrypted at rest using
   Argon2id key derivation and XChaCha20-Poly1305 authenticated encryption
   (v3 keystore format).

### Known limitations

- **Trezor hardware wallet is a Phase 15 stub** — `TrezorHardwareWallet::open()` always returns `NotConnected`. Ledger (Nano S/X/S+/Stax) is fully supported via Linux hidraw ioctl.
- **Contract bytecode not persisted across restarts** — `CF_CONTRACT_CODE` column family is a Phase 15 carry-over. Only the slot→value storage map is persisted; deployed bytecode is lost on restart.
- **No light-client proof protocol** — `Chain.blocks` retains the last 512 blocks in RAM; older blocks are served from RocksDB. A full node is required for all current use cases.
- **Privacy tree not rolled back on reorg** — the Poseidon commitment tree and nullifier set are append-only by design. A reorg deeper than `REORG_DEPTH = 32` requires a full privacy state resync. This is an accepted Phase 14 trade-off.

---

## Testnet Faucet

The `faucet/` binary crate provides a self-hosted testnet faucet.

```bash
# 1. Start the node with a deterministic keypair so coinbase rewards are recoverable
cargo run -p rupeecoin-node -- \
  --data-dir /tmp/testnet \
  --stake 10000 \
  --validator-seed cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc

# 2. Fund the faucet address (printed by node on startup)
cargo run --example fund_faucet -p rupeecoin-faucet -- \
  --seed cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc \
  --to   <faucet_address_b58> \
  --amount 5000

# 3. Start the faucet
cargo run -p rupeecoin-faucet -- \
  --node-rpc http://127.0.0.1:9944 \
  --keyfile  faucet-key.json
# → Browse to http://127.0.0.1:8081
```

**Faucet endpoints:**

| Endpoint | Description |
|---|---|
| `GET /` | Dark-mode SPA — paste address, request 100 RPC |
| `POST /drip` | JSON `{"address":"<b58>"}` → drips 100 RPC, rate-limited 1/IP/24h |
| `GET /api/stats` | `{"faucet_address","balance_micro","total_drips","drip_amount_micro"}` |

---

## Benchmarks

Run the Criterion benchmark suite (release build recommended):

```bash
cargo bench -p rupeecoin-tests                     # all groups
cargo bench -p rupeecoin-tests --bench chain_sync  # single group
```

| Benchmark | Result | Notes |
|---|---|---|
| `build_block/single` | ~830 µs → ~1 200 blocks/s | construction only |
| `add_blocks/200` | ~249 ms → ~803 blocks/s | produce + validate |
| `chain_sync/50` | 11.5 ms → 4 355 blocks/s | pure sync |
| `chain_sync/1000` | 2.28 s → 439 blocks/s | degrades with UTXO growth |
| `state_root/100K` | 346 ms → ~290K UTXOs/s | O(N log N) |
| STARK verify | ~50 ms | within ≤50 ms whitepaper target |
| STARK prove | ~3–5 s | Phase 11 mobile optimization target |

---

## Future Roadmap

### Phases 11–14 — Complete

All zk-STARK hardening (Phase 11), the RupeeCoin VM (Phase 12), mainnet preparation (Phase 13), and mainnet launch hardening (Phase 14) are **complete as of 2026-06-16**. The security checklist (`audit/SECURITY_CHECKLIST.md`) is 7/7 items resolved.

### Phase 15 — Ecosystem Growth — Next

**Engineering carry-overs (3 items from Phase 14 security checklist):**
- **N-04** — Validator allow-list for gossipsub (`core/network/src/lib.rs`)
- **W-05b** — Trezor protobuf HID implementation (`wallet/src/hw_trezor.rs`)
- **VM-07b** — `CF_CONTRACT_CODE` RocksDB column family for bytecode persistence (`core/storage/src/lib.rs`)

**Public validator onboarding:**
- `docs/testnet-validator.md` — step-by-step guide to join Zeta testnet
- Bootnode multiaddrs + latest checkpoint hash published
- External Security Audit #1 (≥2 independent firms; scope: consensus + core + staking)

**Ecosystem:**
- Ecosystem grants: first cohort of 10 funded projects
- Cross-chain bridges: Bitcoin (HTLC), Ethereum (ZK bridge)
- Mobile wallet v2.0: consumer-grade iOS + Android, send enabled
- Block explorer v2.0: RVM contract UI, ABI viewer
- Developer hackathon series: 3 events in Year 1

**Milestone:** 100+ external validators, Casper FFG finalising across ≥5 geographic regions for 30 consecutive days.

---

## Desktop Wallet

A Tauri 2 desktop GUI wallet wrapping the RupeeCoin transparent UTXO stack.

```bash
# System deps (Ubuntu 24.04)
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev

cd desktop-wallet
cargo build --release
./target/release/rupeecoin-desktop-wallet
```

**Features:** create / unlock wallet, portfolio view, send (coin selection + Dilithium3 sign), receive address, UTXO list, Settings (RPC URL + chain ID), auto-refresh every 15 s.

**Keystore:** Argon2id (64 MiB, 3 iter) + XChaCha20-Poly1305, AD = `b"rupeecoin-keystore-v2"`. File stored in app data dir.

---

## Mobile Wallet

A Flutter + flutter_rust_bridge 2 Android wallet. Rust crypto bridge lives at `mobile-wallet/rust/`.

```bash
# Verify Rust side
cd mobile-wallet/rust && cargo test

# One-time setup
rustup target add aarch64-linux-android
cargo install flutter_rust_bridge_codegen
# Install Flutter SDK: https://flutter.dev/docs/get-started/install/linux
# Install Android Studio + NDK 27.0.12077973

# Generate Dart/Rust bridge glue
cd mobile-wallet
flutter_rust_bridge_codegen generate

# Build APK
flutter pub get
flutter build apk --release
# → build/app/outputs/flutter-apk/app-release.apk
```

**Features:** create / unlock wallet, portfolio (balance + UTXO count), receive (QR code + payment URI `rupeecoin:<addr>`), settings (RPC URL, chain ID, node status), lock wallet. Send is deferred to Phase 11 (STARK prove too slow on mobile until circuit optimisation).

**Android:** minSdk 24 (Android 7.0), arm64-v8a + x86_64, package `network.rupeecoin.wallet`. Handles `rupeecoin:` intent scheme.

---

## Contributing

```bash
# Build
cargo build --workspace

# Test
cargo test --workspace

# Lint
cargo clippy --workspace -- -D warnings

# Format
cargo fmt --all
```

RupeeCoin is open-source and community-governed. No company controls the chain.

---

> *From India, For the World, Owned by No One.*
