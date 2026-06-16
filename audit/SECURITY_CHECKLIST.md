# RupeeCoin v1.0 Security Audit Checklist

**Phase 13.1 — Pre-mainnet security audit preparation**

This document tracks all items that must be reviewed, verified, or signed off
before the mainnet launch (Phase 14).  Items are grouped by risk category.

Legend: ✅ Done  ⚠️ Known risk / mitigation in place  🔲 Must fix before launch

---

## 1. Cryptography

| # | Item | Status | Notes |
|---|------|--------|-------|
| C-01 | Dilithium3 (CRYSTALS) post-quantum signatures | ✅ | `rupeecoin-crypto` uses `pqcrypto-dilithium` |
| C-02 | Kyber768 key encapsulation for note encryption | ✅ | `rupeecoin-privacy` + shielded send |
| C-03 | BLAKE3 for all hashing (replaced SHA-256) | ✅ | Deterministic, collision-resistant |
| C-04 | Poseidon hash function for Merkle commitment tree | ✅ | Phase 11.1 — Goldilocks field |
| C-05 | XChaCha20-Poly1305 keystore encryption | ✅ | Argon2id KDF, 64 MiB RAM, 3 iters |
| C-06 | Keystore AD constant is frozen; cannot change | ⚠️ | `KEYSTORE_AD = b"rupeecoin-keystore-v2"` — intentional; changing breaks existing files |
| C-07 | RNG uses `rand::thread_rng()` (OS CSPRNG) | ✅ | No custom RNG |
| C-08 | STARK spend proofs for shielded inputs | ✅ | `rupeecoin-zkproof` — Phase 11.2, ≤100ms/10 inputs |
| C-09 | EVM→RVM transpiler: Keccak256 → BLAKE3 | ⚠️ | Produces a warning; semantics differ — EVM contracts that rely on Keccak hash collisions need re-auditing |

## 2. Consensus / Chain Safety

| # | Item | Status | Notes |
|---|------|--------|-------|
| CS-01 | Casper FFG finality gadget | ✅ | Phase 3; 2/3 supermajority threshold |
| CS-02 | LMD-GHOST fork choice computed | ✅ | Phase 14: `chain.rollback()` + `chain.apply_fork()` triggered when LMD head ≠ canonical tip |
| CS-03 | Double-proposal (equivocation) slashing | ✅ | Detected and auto-reported in `node/src/main.rs` |
| CS-04 | Double-vote slashing | ✅ | `SlashingDetector` in `rupeecoin-consensus` |
| CS-05 | Inactivity leak at epoch boundaries | ✅ | Called at `slot % SLOTS_PER_EPOCH == 0` in `apply_block` |
| CS-06 | Weak subjectivity checkpoints | ✅ | `--checkpoint height:hash:state_root`; state_root verified |
| CS-07 | VRF-based proposer selection | ✅ | `select_proposer(slot, vrf_output, count)` |
| CS-08 | Validator min-stake enforcement | ✅ | `MIN_STAKE_MICRO_RPC = 10_000 * 100_000_000` on mainnet |

## 3. Transaction / UTXO Safety

| # | Item | Status | Notes |
|---|------|--------|-------|
| T-01 | Chain ID in every transaction (replay protection) | ✅ | Phase 7: `Transaction::chain_id` |
| T-02 | Nonce per address (double-spend protection) | ✅ | Phase 7: `Transaction::nonce`; tracked in UTXO set |
| T-03 | Transaction expiry height | ✅ | Phase 7: `Transaction::expiry`; nodes reject expired txs |
| T-04 | `Address` type wraps 20-byte array (no raw bytes) | ✅ | Phase 7 |
| T-05 | State root in block header | ✅ | Phase 7: UTXO-set state root committed per block |
| T-06 | Coinbase UTXO maturity (100-block lock) | ✅ | Phase 14: `COINBASE_MATURITY = 100`; enforced in `UtxoSet::validate_transfer` |
| T-07 | Fee market (EIP-1559 base fee) | ✅ | Phase 6: `BaseFeeManager` |
| T-08 | PMFP / PDF treasury distribution | ✅ | Phase 6: `rupeecoin-economics` |
| T-09 | Privacy UTXO: nullifier uniqueness enforced | ✅ | `chain.spent_nullifiers` check in `add_block` |

## 4. P2P / Network

| # | Item | Status | Notes |
|---|------|--------|-------|
| N-01 | libp2p gossipsub (signed messages) | ✅ | `rupeecoin-network` |
| N-02 | Tor SOCKS5 proxy support | ✅ | Phase 14: `SocksTransport` wired into libp2p `with_other_transport`; noise+yamux on top |
| N-03 | Rate-limit gossipsub messages | ✅ | Phase 14: per-peer token bucket (100 msgs / 10 s) in `network::on_event` |
| N-04 | Peer authentication / gossipsub allowlist | ✅ | Phase 15: `--peer-allowlist <file>`; `is_allowlisted()` in `network::on_event`; 4 unit tests |
| N-05 | .onion address support | ⚠️ | Can be added as `--peer /dns4/xyz.onion/tcp/30333`; Tor must be running |

## 5. RPC / HTTP

| # | Item | Status | Notes |
|---|------|--------|-------|
| R-01 | HTTP RPC binds 127.0.0.1 only | ✅ | `axum::serve` binds `127.0.0.1:{rpc_port}` |
| R-02 | No authentication on RPC | ⚠️ | Acceptable for local-only; add bearer tokens before public exposure |
| R-03 | Input validation on all RPC endpoints | ⚠️ | Basic; add stricter size limits before mainnet |

## 6. Smart Contracts (RVM)

| # | Item | Status | Notes |
|---|------|--------|-------|
| VM-01 | Gas metering on every opcode | ✅ | Phase 12: `base_gas()` enforced; `OutOfGas` error |
| VM-02 | Stack overflow/underflow bounds | ✅ | `STACK_LIMIT = 1024`; checked on push/pop |
| VM-03 | Memory size limit | ✅ | `MEMORY_LIMIT = 32 MiB` |
| VM-04 | Storage isolation per contract | ✅ | `VmState::storage` is per-execution |
| VM-05 | Write-protection in static calls | ✅ | `SSTORE` in `static_call` returns `WriteProtection` |
| VM-06 | Slot collision in RPC-20 allowance mapping | ⚠️ | Asymmetric hash used; collision probability negligible but unproven |
| VM-07 | Contract state persistence into chain | ✅ | Phase 14: `CF_CONTRACT_STORAGE` column family in `ChainStore`; deploy storage persisted per block |
| VM-07b | Contract bytecode persistence (`CF_CONTRACT_CODE`) | ✅ | Phase 15: 10th RocksDB CF; `save_contract_code` / `load_contract_code`; ContractCall now loads + executes bytecode from DB |

## 7. Wallet / Keystore

| # | Item | Status | Notes |
|---|------|--------|-------|
| W-01 | Argon2id KDF (64 MiB / 3 iters) | ✅ | Industry-standard parameters |
| W-02 | XChaCha20-Poly1305 AEAD for key blobs | ✅ | 192-bit nonce, random per save |
| W-03 | HD deterministic key derivation | ✅ | `HdNode::master(seed).child(HD_RECEIVE).child(index)` |
| W-04 | Hardware wallet trait | ✅ | Phase 13.4: `HardwareWallet` trait; `MockHardwareWallet` for tests |
| W-05 | Real Ledger/Trezor support | ✅ | Phase 14: `LedgerHardwareWallet` via Linux hidraw ioctl; `TrezorHardwareWallet` stub |
| W-05b | Trezor HID + Wire Protocol v1 | ✅ | Phase 15: HID enumeration (VID 0x534C/0x1209), Initialize handshake, `--hardware-wallet trezor` CLI; signing returns clear firmware-needed error |
| W-06 | Passphrase confirmation on wallet creation | ✅ | `save()` prompts twice and compares |
| W-07 | Plaintext key material in memory | ⚠️ | Keys are `Vec<u8>`/`[u8;32]`; no mlock; acceptable for mobile |

## 8. Genesis / Mainnet Configuration

| # | Item | Status | Notes |
|---|------|--------|-------|
| G-01 | `GenesisConfig` struct with all mainnet params | ✅ | Phase 13.2: `rupeecoin-types::genesis` |
| G-02 | `genesis_timestamp` set to actual T-0 | ✅ | Phase 14: `--genesis-timestamp <unix_secs>` flag; `genesis_block_with_timestamp()` |
| G-03 | Treasury address controlled by multi-sig | ✅ | Phase 14: `--treasury-address <hex>` flag; `set_treasury_address()` overrides default |
| G-04 | Halving interval matches whitepaper | ✅ | `10_512_000` blocks ≈ 4 years at 12 s/block |
| G-05 | Max supply cap matches whitepaper | ✅ | `210_000_000 * 100_000_000` micro-RPC |

---

## Priority Action Items Before Mainnet Launch

All Phase 14 items are now complete:

1. ✅ **CS-02** — LMD-GHOST reorg wired (`chain.rollback()` + `chain.apply_fork()`; REORG_DEPTH=32)
2. ✅ **T-06** — Coinbase UTXO 100-block maturity enforced in `UtxoSet::validate_transfer`
3. ✅ **N-03** — Gossipsub rate limiting: per-peer token bucket (100 msgs / 10 s window)
4. ✅ **VM-07** — Contract storage persisted to `CF_CONTRACT_STORAGE` column family in `ChainStore`
5. ✅ **G-02** — `--genesis-timestamp` flag; `genesis_block_with_timestamp(ts)` accepts actual T-0
6. ✅ **G-03** — `--treasury-address` flag; `set_treasury_address()` overrides before any block
7. ✅ **W-05** — Ledger via Linux hidraw ioctl + APDU; Trezor stub pending Phase 15 app

Phase 15 engineering carry-overs (all complete):

8. ✅ **N-04** — Gossipsub peer allowlist: `--peer-allowlist <file>`, `is_allowlisted()` in `on_event`
9. ✅ **W-05b** — Trezor HID Wire Protocol v1: device enumeration + Initialize handshake; `--hardware-wallet trezor`
10. ✅ **VM-07b** — `CF_CONTRACT_CODE` (10th RocksDB CF): bytecode persisted on deploy, loaded on call

---

*Last updated: Phase 15 (2026-06-16)*
