# RupeeCoin — Full Codebase Bug Report

**Date:** 2026-06-17  
**Audited by:** Internal AI audit (9 parallel agents covering all 19 crates)  
**Files audited:** 85 Rust + 30 TS/Python/Dart source files, Phases 1–15  
**Total findings:** 191 bugs

---

## Severity Summary

| Severity | Count | Description |
|----------|-------|-------------|
| 🔴 Critical | 26 | Chain-breaking, fund theft, proof bypass, remote DoS |
| 🟠 High | 65 | Significant security/correctness issues |
| 🟡 Medium | 68 | Logic errors, resource leaks, API misuse |
| 🟢 Low | 32 | Minor issues, code quality, edge cases |
| **Total** | **191** | |

---

## 🔴 CRITICAL BUGS (26)

| ID | File | Lines | Description |
|----|------|-------|-------------|
| C-01 | `core/privacy/src/transaction.rs` | 233–253 | `verify_batch_stark()` never calls the STARK verifier — only checks `num_spends` count. All Phase-11.2 shielded txs pass without ZK proof verification. |
| C-02 | `core/privacy/src/proof.rs` | 262 | `BalanceProof::verify` uses `saturating_add` — attacker sets `total_output=u64::MAX-1, fee=100` → saturates to `u64::MAX`; forges arbitrary balance. |
| C-03 | `core/privacy/src/transaction.rs` | 214–222 | Input/output `.sum::<u64>()` wraps silently. 10 inputs of ~`u64::MAX/10` each sum to a small number, satisfying balance equation with zero fee. No per-input value cap. |
| C-04 | `core/privacy/src/proof.rs` | 206–218 | `BatchedStark` anchor returns `Hash::zero()`; `validate_stateful` never cross-checks it against STARK proof's embedded anchor felts. |
| C-05 | `core/blockchain/src/lib.rs` | 427–441 | Proposer signature + VRF verification inside `if let Some(proposer) = registry.get(...)`. If `registry.get()` returns `None`, both checks are silently skipped and the unsigned block is accepted. |
| C-06 | `core/blockchain/src/lib.rs` | 547–552 | `validate_slashing_report` has no signature check on reward claims — any node can drain any validator's accumulated rewards. |
| C-07 | `core/consensus/src/checkpoints.rs` | 57–61 | `is_before_checkpoint(height)` returns `true` for any block at or below checkpoint height with **no hash check**. Fabricated blocks below checkpoint accepted. |
| C-08 | `core/blockchain/src/lib.rs` | 501–515 | `DoubleProposal` slashing never verifies both signed blocks are at the **same slot**. Any two valid signatures from a validator can trigger fraudulent slashing. |
| C-09 | `core/utxo/src/lib.rs` | 99–109 | Nonce checked only for the **first input** in a multi-input transaction — all other inputs can replay nonces freely. |
| C-10 | `core/utxo/src/lib.rs` | 179–183 | Nonce incremented only for the **first input** — nonce tracking diverges from spend reality. |
| C-11 | `core/utxo/src/lib.rs` | 160–184 | `apply_block` mutates UTXO state without calling any validation — UTXO corruption if caller skips `validate_transfer`. |
| C-12 | `core/utxo/src/lib.rs` | 144, 151 | `.expect()` on u64 arithmetic in `validate_transfer` → remote DoS (panic) via crafted inputs/outputs. |
| C-13 | `core/utxo/src/lib.rs` | 114 | `lock_height + COINBASE_MATURITY` plain u64 addition overflows at large heights — wraps to 0, bypassing 100-block coinbase maturity lock. |
| C-14 | `core/economics/src/rewards.rs` | 70–86 | `compute_block_rewards` mints unconditionally — no check against `MAX_SUPPLY`. Coinbase minting exceeds the 1B RPC cap. |
| C-15 | `core/mempool/src/lib.rs` | 56–63 | `filter_map` on UTXO lookup silently drops missing inputs, computing fee=0. Txs spending nonexistent UTXOs enter mempool bypassing fee floor. |
| C-16 | `core/staking/src/lib.rs` | 135–138 | `effective_weight()` uses `f64` multiplier — IEEE 754 is platform non-deterministic. ARM vs x86 validators compute different reward splits → consensus fork. |
| C-17 | `core/staking/src/lib.rs` | 356–362 | `total_effective_weight()` accumulates f64-derived u64s — same non-determinism as C-16 plus u64 sum overflow for large validator sets. |
| C-18 | `node/src/rpc.rs` | 379–388 | `submit_tx` queues any transaction with **zero validation** — no signature check, no fee check, no size limit. OOM DoS + arbitrary junk accepted. |
| C-19 | `faucet/src/main.rs` | 71–118 | Faucet seed stored in **plaintext JSON** — anyone who reads the file recovers the private key. |
| C-20 | `faucet/src/main.rs` | 97–99, 435 | Faucet nonce stored on disk drifts on crash/restart. After keyfile write fails, all subsequent transactions use wrong nonce. |
| C-21 | `faucet/src/main.rs` | 316–337 | Rate-limit is per-IP only — no per-recipient-address tracking. Any attacker with multiple IPs drains the faucet completely. |
| C-22 | `faucet/examples/fund_faucet.rs` | 105 | Hardcodes `nonce: 0` — second run silently fails. |
| C-23 | `sdk-js/src/address.ts` | 5–7, 76 | JS SDK uses `ADDRESS_VERSION = 0x12` and SHA-256 checksum. Chain expects `0x52` (`'R'`) and BLAKE3. Every JS-generated address is invalid on-chain. |
| C-24 | `rupeecoin-sdk/src/tx.rs` | 56–58 | `coins_to_micro(f64)` — negative input wraps to huge u64; `NaN → 0`; `0.1 RPC → 9_999_999 µRPC`. |
| C-25 | `core/blockchain/src/lib.rs` | 716–720 | State root omits privacy txs — commitment tree + nullifier set not committed. Fraudulent shielded txs undetectable via state root. |
| C-26 | `core/consensus/src/slashing.rs` | 25–48, 53–66 | `record_proposal` and `record_attestation` store signatures without verifying them. Any peer can inject false slashing evidence. |

---

## 🟠 HIGH BUGS (65)

| ID | File | Lines | Description |
|----|------|-------|-------------|
| H-01 | `core/privacy/src/note.rs` | 46–52 | BLAKE3 nullifier (on-chain) and Poseidon nullifier (STARK) are never cross-checked — double-spend via inconsistent nullifier derivation. |
| H-02 | `core/privacy/src/transaction.rs` | 296–318 | `PrivacyBlockApply::compute` applies txs without calling any validation — forged txs silently commit to tree + nullifier set. |
| H-03 | `core/privacy/src/commitment_tree.rs` | 95–116 | No capacity guard — after 2²⁰ leaves the frontier corrupts silently (break never fires). |
| H-04 | `core/privacy/src/proof.rs` | 23–42 | `Blake3Path` reveals raw `NoteCommitment` on-chain (zero privacy) and is the default in `ShieldedTxBuilder::build()`. |
| H-05 | `core/privacy/src/keys.rs` | 195–206 | `generate()` creates `ViewKey` with an independent random call — not derivable from seed. Backing up spend key loses decryption ability. |
| H-06 | `core/zkproof/src/types.rs` | 37–39 | Single subtraction doesn't fully reduce values ≥ P in Goldilocks field — Poseidon collision resistance reduced from 128 to ~96 bits. |
| H-07 | `core/zkproof/src/circuit.rs` | 122–134 | STARK circuit uses product-form OR constraint for cm-placement — attacker sets `left=right=cm_carry` to break Merkle membership soundness. |
| H-08 | `core/blockchain/src/lib.rs` | 79–81 | Privacy nullifiers NOT rolled back on reorg — shielded notes spent in reorged block can never be spent again (permanent fund loss). |
| H-09 | `core/blockchain/src/lib.rs` | 181–188 | `blocks` and `block_deltas` trimmed in separate operations — panic between them de-syncs structures, causing wrong delta on rollback. |
| H-10 | `core/blockchain/src/lib.rs` | 468–488 | `validate_reward_claim` does not verify claimant's signature — anyone can drain any validator's rewards. |
| H-11 | `core/consensus/src/lib.rs` | 217–224 | Attestation signatures inside blocks never verified — `attestation_pool.add` error silently discarded. Entire finality can be forged. |
| H-12 | `core/consensus/src/lib.rs` | 342–351 | `is_in_subtree` loops forever on a cycle in `known_block_parents` — peer hangs node permanently with `A→B, B→A`. |
| H-13 | `core/consensus/src/lib.rs` | 130–134 | Equivocating second vote silently dropped instead of escalating to slashing detector — double-voter avoids punishment. |
| H-14 | `core/consensus/src/finality.rs` | 43–48 | Casper FFG finalizes epoch N-1 whenever N and N-1 are both justified — no supermajority link check. Two disjoint 2/3 subsets can finalize. |
| H-15 | `core/economics/src/inactivity.rs` | 37–41 | Inactivity penalty = `stake × excess_epochs / 65536` — validator missing 65,537 epochs loses **entire stake in one call**. |
| H-16 | `core/economics/src/inactivity.rs` | 47 | `current_epoch * 32` unchecked multiply — panics in debug at large epoch numbers; wraps in release, releasing stake at past block. |
| H-17 | `core/economics/src/rewards.rs` | 103–104 | Plain `u64` addition in dust correction — overflows in debug (panic); wraps in release, inflating proposer reward. |
| H-18 | `core/rvm/src/token/rpc20.rs` | 177–178 | `transferFrom` allowance check off-by-one — rejects `amount == allowance`. All exact-amount approvals fail. |
| H-19 | `core/rvm/src/token/rpc20.rs` | 179–193 | Allowance consumed **before** balance check — if sender has insufficient balance, allowance permanently lost with no token movement. |
| H-20 | `core/rvm/src/vm.rs` | 292–309 | `AddOv/SubOv/MulOv` push 2 values; second push can fail at `STACK_LIMIT`, leaving corrupted stack with result interpreted as overflow flag. |
| H-21 | `core/rvm/src/vm.rs` | 285–290 | `Exp` silently casts `u64 exponent → u32` — exponents > `u32::MAX` truncate; `u32::MAX+1` → exp=0, result=1. |
| H-22 | `core/rvm/src/vm.rs` | 324–328 | `SAR` with `shift >= 64` executes `val >> 64` — panics in debug builds. |
| H-23 | `core/rvm/src/opcode.rs` | 159–161 | `MulOv` priced at 3 gas (same as `Pop`) instead of 5 (same as `Mul`) — attackers use it as cheap multiplication. |
| H-24 | `core/rvm/src/token/rpc721.rs` | 39–58 | RPC-721 `mint` has no access control — anyone can mint NFTs to any address. |
| H-25 | `core/rvm/src/token/rpc1155.rs` | 94–187 | RPC-1155 `burn` and `transfer` have no ownership check — anyone can burn or move any user's tokens. |
| H-26 | `core/network/src/lib.rs` | 404–413, 418 | Gossipsub messages with `source = None` bypass both allowlist and rate limit. |
| H-27 | `core/network/src/lib.rs` | 362–376 | On dandelion timeout, if gossipsub publish fails, tx is permanently dropped with no retry. |
| H-28 | `core/network/src/lib.rs` | 95–99, 125–129 | `SyncCodec` reads 4-byte length prefix with no size cap — peer sends `0xFFFFFFFF` → 4 GiB allocation → OOM/panic. |
| H-29 | `core/network/src/lib.rs` | 582–591 | mDNS-discovered peers added as explicit gossipsub peers without allowlist check. |
| H-30 | `core/storage/src/lib.rs` | 351–380 | Contract storage keys use `to_le_bytes()` — all other keys use big-endian. LE keys break RocksDB prefix scans. |
| H-31 | `node/src/rpc.rs` | 223–229 | `submit_privacy_tx` body is unlimited — peer sends 2 GB hex → 1 GB allocation. |
| H-32 | `node/src/rpc.rs` | 379–388 | `pending_txs` and `pending_privacy_txs` are unbounded Vecs — DoS via flooding. |
| H-33 | `wallet/src/keystore.rs` | 94–100 | `LoadedKeystore` does not zeroize on drop — spend key lingers in memory/swap. |
| H-34 | `wallet/src/keystore.rs` | 104–111 | Derived encryption key `[u8; 32]` not zeroed after use in `save`/`load`. |
| H-35 | `wallet/src/keystore.rs` | 131–133 | Passphrase `String` not zeroed after `derive_key`. |
| H-36 | `wallet/src/keystore.rs` | 254–266 | V1 unencrypted keystore loaded silently with no warning. |
| H-37 | `wallet/src/main.rs` | 562–564 | Nonce derived from `hd_receive_index` instead of on-chain tx count — two sends without `wallet receive` use same nonce. |
| H-38 | `wallet/src/main.rs` | 237–271 | Hardware wallet opened and printed then immediately dropped — all signing still uses in-memory key. |
| H-39 | `wallet/src/main.rs` | 273–274 | f64 conversion of RPC amounts — `1.1 RPC → 109_999_999 µRPC`; `NaN → 0`; negative wraps to huge u64. |
| H-40 | `desktop-wallet/src/wallet_commands.rs` | 226–238 | Coin selection ignores fee — change = `total_in - amount` with no fee deduction. Every desktop wallet tx has zero fee. |
| H-41 | `desktop-wallet/src/wallet_commands.rs` | 133–298 | `rpc_url` accepted from JavaScript frontend on every Tauri command — SSRF via `file://`, cloud metadata endpoints, port-scan. |
| H-42 | `core/blockchain/src/lib.rs` | 343–349 | `gas_used` validated as `tx_count - 1` (1 gas per tx) — EIP-1559 fee market is broken; all txs cost identical gas. |
| H-43 | `core/consensus/src/lib.rs` | 156–162 | `is_justified` returns `false` when `total_stake == 0` — chain can never achieve finality during bootstrap. |
| H-44 | `core/network/src/socks.rs` | 42–86 | All addresses (including raw IPs) encoded as `ATYP=0x03` domain strings — Tor may resolve locally, breaking anonymity. |
| H-45 | `sdk-js/src/rpc.ts` | 29–54 | u64 fields parsed via JS `number` (f64) — all values above `2^53` silently truncate before `BigInt()`. |
| H-46 | `rupeecoin-sdk/src/keys.rs` | 72 | `advance_receive_index()` plain u32 addition — panics in debug at `u32::MAX`; wraps to 0 in release. |
| H-47 | `rupeecoin-sdk/src/rpc.rs` | — | `get_nonce` endpoint missing from Rust SDK and JS SDK. All builders default to `nonce=0`. |
| H-48 | `core/economics/src/rewards.rs` | 85–86 | No max-supply cap in `compute_block_rewards` — minting continues past the 1B RPC cap. |
| H-49 | `node/src/rpc.rs` | 379–388 | `submit_tx` accepts transactions with no signature verification. |
| H-50 | `core/blockchain/src/lib.rs` | 596–605 | `build_reward_claim` sets `nonce: 0, expiry: 0` — old signed claims replay after stake replenishment. |
| H-51 | `node/src/main.rs` | 396–398 | Contract address = first 8 bytes of txid as u64 — birthday collision at 2^32 deploys; `unwrap_or(0)` assigns address 0. |
| H-52 | `desktop-wallet/src/wallet_commands.rs` | 114 | `receive_index += 1` on u32 — overflow panic in debug, silent wrap in release. |
| H-53 | `core/rvm/src/token/rpc20.rs` | 131–137, 172–174 | Allowance slot uses same multiplier for owner and spender — diverges from `token/mod.rs::allowance_slot`. |
| H-54 | `core/consensus/src/lib.rs` | 254–258 | LMD-GHOST `latest_messages` updated with caller-supplied hash, not `att.block_hash`. Stake credited to wrong fork. |
| H-55 | `wallet/src/keystore.rs` | 24–27 | Argon2id: `memory=64 MiB, parallelism=1` — below recommended 256 MiB; single-lane removes GPU resistance. |
| H-56 | `core/privacy/src/proof.rs` | 63–131 | `RangeProof` is not ZK — `verify` takes `value: u64` in cleartext; no actual range bound enforced. |
| H-57 | `core/blockchain/src/lib.rs` | 199–211 | Nonce delta recorded inside per-input loop with `tx.inputs.first()` — fragile; incorrect for multi-sender support. |
| H-58 | `core/crypto/src/signature.rs` | 94–110 | 4032-byte `sk_bytes` stack array not zeroized after signing — Dilithium3 secret key lingers in memory. |
| H-59 | `core/crypto/src/signature.rs` | 103 | `assert_eq!` on C FFI return value → unrecoverable panic in wallet restore path. |
| H-60 | `core/utxo/src/lib.rs` | 190–206 | `apply_transactions` is `pub` with no validation — second UTXO corruption entry point. |
| H-61 | `core/types/src/block.rs` | 51–54 | `Block::hash()` uses `.expect()` on serialization — crash in consensus hot path. |
| H-62 | `core/types/src/transaction.rs` | 78, 83 | `sig_hash()` and `txid()` use `.expect()` — crash in validation hot path. |
| H-63 | `core/types/src/attestation.rs` | 44–60 | `SlashingEvidence` accepted without any pre-use validation (self-matching hashes, fake pubkeys). |
| H-64 | `core/blockchain/src/genesis.rs` | 62–63 | `genesis_block_with_timestamp(timestamp)` hardcodes `timestamp: 1_700_000_000` — ignores the parameter. |
| H-65 | `faucet/src/main.rs` | 316–431 | Faucet holds global `Mutex` across all network I/O — one slow connection freezes all faucet users. |

---

## 🟡 MEDIUM BUGS (68)

| ID | File | Lines | Description |
|----|------|-------|-------------|
| M-01 | `core/zkproof/src/hash.rs` | 32–44 | Poseidon MDS branch number = 2 (minimum). Spec requires t+1 = 9 for t=8. |
| M-02 | `core/privacy/src/commitment_tree.rs` | 193–204 | `MerklePath` length unchecked — 0-length path returns leaf as root; passes anchor check. |
| M-03 | `core/privacy/src/proof.rs` | 251–264 | Stored `balance_proof` never validated against actual field sums — equivocation possible. |
| M-04 | `core/privacy/src/proof.rs` | 231–241 | Deprecated `StarkProofPlaceholder::verify()` always returns `true` — not gated by `#[cfg(test)]`. |
| M-05 | `core/privacy/src/keys.rs` | 15–22, 149–156 | Raw `extern "C"` Kyber keygen with local size constants; `assert_eq!` panics in production. |
| M-06 | `core/privacy/src/note.rs` | 128–145 | `note_enc_key` uses raw BLAKE3; `note_nonce` uses `new_derive_key` mode — mixed KDF APIs, violates domain separation. |
| M-07 | `core/privacy/src/keys.rs` | 211–223 | `from_seed` derives both spend and view keys from the same `seed` with two different BLAKE3 API modes. |
| M-08 | `core/rvm/src/token/rpc20.rs` | 106 | Recipient balance addition uses `Op::Add` (wrapping) — `to_balance + amount` can overflow, destroying tokens. |
| M-09 | `core/rvm/src/vm.rs` | 466–480 | `LOG0..LOG4` opcodes not guarded by `is_static` — logs emitted in static calls. |
| M-10 | `evm2rvm/src/main.rs` | — | EVM `SAR` (0x1D) has no match arm — silently emits REVERT stub for all signed arithmetic. |
| M-11 | `evm2rvm/src/main.rs` | — | EVM `CALLDATALOAD/SIZE/COPY` (0x35–0x37) unmapped — nearly every real EVM contract silently broken. |
| M-12 | `core/economics/src/rewards.rs` | 125–132 | Fee split asymmetric rounding — PMFP always gets slightly more than 30%, violating stated 70/30 split. |
| M-13 | `core/economics/src/pmfp.rs` | 57–71 | No dust correction after per-validator distribution — up to N-1 µRPC per epoch locked permanently in PMFP. |
| M-14 | `core/mempool/src/lib.rs` | 66–76 | Eviction compares absolute fee not fee-per-byte — large low-rate txs can pin mempool. |
| M-15 | `core/mempool/src/lib.rs` | 89–100 | `drain_for_block` doesn't check intra-batch UTXO conflicts — double-spend in proposed block possible. |
| M-16 | `core/staking/src/lib.rs` | 62–84 | `uptime = att_rate` and `attest = att_rate` are identical — W_UPTIME + W_ATTESTATION = 0.7 both on same metric. |
| M-17 | `core/blockchain/src/lib.rs` | 596–605 | Reward claim `nonce=0, expiry=0` — replay attack after rewards replenished. |
| M-18 | `core/blockchain/src/lib.rs` | 305–309 | Epoch boundary fires at height 32 with only 31 slots of attestation data — off-by-one. |
| M-19 | `core/consensus/src/lib.rs` | 57 | `select_proposer` uses `idx % active_count` — modulo bias; first validators chosen ~0.003% more often. |
| M-20 | `core/consensus/src/checkpoints.rs` | 38–47 | Conflicting checkpoint at same height silently dropped — misconfiguration goes undetected. |
| M-21 | `core/consensus/src/lib.rs` | 254–258 | *(see H-54)* — LMD-GHOST also affected by stale caller-supplied hash. |
| M-22 | `core/network/src/lib.rs` | 48–60 | Rate limit off-by-one — 101 messages pass per window instead of 100 (window-reset resets to 1, not 0). |
| M-23 | `core/network/src/lib.rs` | 329, 554–562 | `pending_sync` map grows unboundedly — malicious peer exhausts memory with repeated unanswered sync requests. |
| M-24 | `core/network/src/lib.rs` | 496–536 | `enter_stem` called even for immediately-fluffed terminal hops — ghost tracking entry. |
| M-25 | `core/network/src/socks.rs` | 98–108 | Unknown SOCKS5 ATYP byte silently ignored — stream left desynchronized. |
| M-26 | `core/storage/src/lib.rs` | 404, 411 | `save_contract_code`/`load_contract_code` keys use `to_le_bytes()` — inconsistent with rest of codebase. |
| M-27 | `core/storage/src/lib.rs` | 254–309 | `note_count` read-modify-write outside `WriteBatch` — concurrent `apply_block` causes leaf index collisions. |
| M-28 | `core/storage/src/lib.rs` | 37–63 | No `flush()` on shutdown — secondary readers see stale state after unclean shutdown. |
| M-29 | `core/storage/src/lib.rs` | 331–346 | `scan_utxos_for_address` is O(N) full table scan — DoS if exposed via RPC on mainnet. |
| M-30 | `node/src/rpc.rs` | 449 | `hex::decode(&addr).unwrap()` in UTXO handler — panics the axum task. |
| M-31 | `node/src/rpc.rs` | 498–499 | `TcpListener::bind(...).unwrap()` — RPC server silently dies on port conflict. |
| M-32 | `node/src/rpc.rs` | 135–154 | `count` pagination parameter unbounded — `count=u64::MAX` tries to return billions of records. |
| M-33 | `node/src/rpc.rs` | 184 | `params.index as usize` — silent truncation on 32-bit; unchecked `auth_path(idx)` may panic. |
| M-34 | `node/src/main.rs` | 813–816 | Fork block cache never evicts stale entries — once at 128, new fork blocks are silently discarded. |
| M-35 | `node/src/main.rs` | 517–519 | Reorg ancestor height computed from `fork_path[0].height - 1` — wrong if fork blocks are non-contiguous. |
| M-36 | `wallet/src/scanner.rs` | 45–49 | Nullifier-check errors silently include the note in spendable set — expensive failed STARK generation. |
| M-37 | `node/src/rpc.rs` | 160 | `bincode::serialize(...).unwrap_or_default()` — serialization failure silently drops note from response. |
| M-38 | `node/src/main.rs` | 396–398 | *(see H-51)* — contract address collision-prone. |
| M-39 | `explorer/src/main.rs` | 391–451 | Tx index built once at startup, never refreshed — all txs after startup invisible to search. |
| M-40 | `explorer/src/main.rs` | 223, 347 | Tx lookup case-sensitive — uppercase hex txids fail even though tx exists. |
| M-41 | `dashboard/src/main.rs` | 114–120 | `--addr` CLI flag used directly in URL without encoding — log injection via crafted address. |
| M-42 | `faucet/src/main.rs` (HTML) | 615–618 | `txid` inserted via `innerHTML` — XSS potential. |
| M-43 | `faucet/src/main.rs` (HTML) | 619–633 | `d.error` inserted via `innerHTML` — XSS potential. |
| M-44 | `desktop-wallet/src/keystore.rs` | 86 | Non-atomic `fs::write` — power loss between truncate and write leaves keystore zero-bytes. |
| M-45 | `desktop-wallet/src/wallet_commands.rs` | 249–250 | `hex::decode(...).expect(...)` on node txid — panics Tauri backend on malformed response. |
| M-46 | `faucet/src/main.rs` | 197–212 | `saturating_add` masks UTXO sum overflow; change subtraction unguarded — wraps silently. |
| M-47 | `sdk-js/src/tx.ts` | 31–44 | No length validation on txid (32 B) or recipient (20 B) — wrong lengths produce malformed bincode. |
| M-48 | `sdk-js/src/rpc.ts` | 14–70 | No `AbortController` / timeout on `fetch` — unresponsive node hangs caller indefinitely. |
| M-49 | `sdk-py/src/lib.rs` | 337 | `TESTNET_CHAIN_ID` not exported to Python module — callers hard-code magic number `2`. |
| M-50 | `rupeecoin-sdk/src/rpc.rs` | 104–195 | All ureq HTTP calls use default agent with no connect/read timeout. |
| M-51 | `sdk-js/src/signer.ts` | 36–38 | `PreSignedSigner.sign()` ignores `message` argument — always returns stale stored signature. |
| M-52 | `rupeecoin-sdk/src/tx.rs` | 21–51 | `build_transfer` accepts any `chain_id` — no validation against known chain IDs. |
| M-53 | `sdk-py/src/lib.rs` | 239 | `build_tx` always signs with `receive_keypair()` — no way to specify which HD index controls the UTXOs. |
| M-54 | `core/types/src/contract.rs` | 42–43, 58–59 | `encode()` uses `.expect()` — panic in tx construction path. |
| M-55 | `core/types/src/genesis.rs` | 149–158 | `halving_interval = 0` not rejected — division by zero in `block_reward(height)`. |
| M-56 | `core/types/src/genesis.rs` | 84 | Zero treasury address accepted by `validate()` — funds sent to null address unrecoverable. |
| M-57 | `core/utxo/src/lib.rs` | 86–157 | Zero-input non-coinbase transaction passes `validate_transfer` — zero-cost spam. |
| M-58 | `core/utxo/src/lib.rs` | 218, 223–224 | `state_root()` uses `.expect()` + allocates two Vecs per UTXO per call — expensive in hot path. |
| M-59 | `core/crypto/src/vrf.rs` | 22–35 | Dilithium3 signing is non-deterministic — VRF output malleable; proposer can bias lottery. |
| M-60 | `core/blockchain/src/lib.rs` | 214 | `vout as u32` silent truncation if tx has > 4B outputs. |
| M-61 | `core/consensus/src/finality.rs` | 32–49 | `justified_checkpoints` Vec grows without bound — one entry per epoch, never pruned. |
| M-62 | `core/consensus/src/slashing.rs` | 70–75 | `SlashingDetector::prune()` never called — `seen_proposals` and `seen_attestations` grow forever. |
| M-63 | `core/economics/src/pmfp.rs` | 35–40 | Epoch emission window comment wrong — effective window always slightly longer than 365 days due to truncation. |
| M-64 | `core/types/src/transaction.rs` | 9–10 | 3-byte domain prefixes can collide with 8-byte coinbase height encoding at specific block numbers. |
| M-65 | `faucet/src/main.rs` | 316–431 | *(also H-65)* — mutex performance listed separately as blocking all requests. |
| M-66 | `desktop-wallet/src/wallet_commands.rs` | 194 | f64 precision loss on µRPC in desktop wallet (same as H-39). |
| M-67 | `faucet/examples/fund_faucet.rs` | 55 | f64 precision on µRPC in funding example (same as C-24). |
| M-68 | `core/utxo/src/lib.rs` | 70–76 | `decrement_nonce` clamps at 0 silently — hides reorg bugs. |

---

## 🟢 LOW BUGS (32)

| ID | File | Lines | Description |
|----|------|-------|-------------|
| L-01 | `core/crypto/src/hash.rs` | 42–47 | `hash_concat` no domain separation — second-preimage in Merkle tree. |
| L-02 | `core/crypto/src/hd.rs` | 45–57 | No hardened derivation support — child key compromise leaks parent. |
| L-03 | `core/crypto/src/hd.rs` | 52 | `depth.saturating_add(1)` clamps silently at 255 — wrong path reconstruction. |
| L-04 | `core/types/src/attestation.rs` | 13, 35–40 | `Attestation::sig_data` and `ValidatorAnnouncement::sig_data` no domain separator. |
| L-05 | `core/types/src/contract.rs` | 29–30 | `contract_addr: u64` instead of `Address` type — API inconsistency. |
| L-06 | `core/utxo/src/lib.rs` | 70–76 | `decrement_nonce` clamps at 0 silently — hides bugs during reorg processing. |
| L-07 | `core/staking/src/lib.rs` | 297–312 | Plain `u64 +=` on contribution counters — should use `saturating_add`. |
| L-08 | `core/staking/src/lib.rs` | 329–335 | `total_active_stake()` sums u64 without u128 accumulator — overflow for large validator sets. |
| L-09 | `core/economics/src/supply.rs` | 19–21 | Comment says "Era 4+ tail" but only 3 halvings before floor; comment is wrong. |
| L-10 | `core/blockchain/src/lib.rs` | 141–143 | `contains_hash` recomputes all block hashes O(N·hash) — wasteful in gossip hot path. |
| L-11 | `core/blockchain/src/genesis.rs` | 18–21 | Genesis coinbase sends `BLOCK_REWARD` to `Address::zero()` — permanently burned; needs named constant. |
| L-12 | `core/consensus/src/lib.rs` | 71–80 | `verify_block_vrf` returns `true` on absent proof — dangerous if reused outside current guard. |
| L-13 | `core/privacy/src/commitment_tree.rs` | 141–174 | `auth_path()` always rebuilds from `self.leaves` — O(N) allocation; O(N²) for scanning notes. |
| L-14 | `core/rvm/src/assemble.rs` | 71–78 | `build()` panics on undefined label instead of returning `Result`. |
| L-15 | `core/rvm/src/vm.rs` | 491–497 | `Revert` returns `HashMap::new()` — callers who don't check `ExecResult` silently wipe storage. |
| L-16 | `evm2rvm/src/main.rs` | 153 | `EVM_PC` offset wrong post-transpilation — no warning emitted. |
| L-17 | `node/src/tor.rs` | 165–172 | Proxy reachability check verifies `buf[0]` but not `buf[1]` (method byte). |
| L-18 | `node/src/rpc.rs` | 160 | `unwrap_or_default()` on serialization failure silently drops note. |
| L-19 | `wallet/src/scanner.rs` | 68–88 | Note selection not sorted by value — sub-optimal coin selection. |
| L-20 | `wallet/src/main.rs` | 213–214 | `total_micro_rpc += note.value` — no overflow guard. |
| L-21 | `wallet/src/scanner.rs` | 80 | `total += note.note.value` in `select_notes` — no overflow guard. |
| L-22 | `explorer/src/main.rs` | 455 | Explorer hard-binds to `127.0.0.1` — inaccessible from external clients. |
| L-23 | `faucet/src/main.rs` | 704 | Faucet hard-binds to `127.0.0.1` — inaccessible externally. |
| L-24 | `mobile-wallet/rust/src/api.rs` | 246–249 | `tmp_path()` test helper leaks tempdir via `into_path()`. |
| L-25 | `dashboard/src/main.rs` | 142–184 | Stake display via `u64 as f64 / 1e8` — precision loss for values > 90 trillion µRPC. |
| L-26 | `desktop-wallet/src/keystore.rs` | 6, 84 | `version: 3` written but never checked on load — v1/v2 keystore gives misleading "wrong passphrase" error. |
| L-27 | `rupeecoin-sdk/src/tx.rs` | 45–49 | All inputs signed with same signature — single-key constraint invisible at type level. |
| L-28 | `sdk-py/src/lib.rs` | 95–143 | Python SDK holds GIL during blocking HTTP — stalls multi-threaded Python programs. |
| L-29 | `rupeecoin-tests/tests/chain_integration.rs` | 243 | `sdk_coins_to_micro_roundtrip` only tests `1.0` and `0.0` — zero coverage for negative, NaN, 0.1, u32 overflow. |
| L-30 | `core/consensus/src/finality.rs` | 65–71 | `justified_checkpoints` Vec unbounded growth (resource issue; correctness is fine). |
| L-31 | `core/types/src/attestation.rs` | 13 | Attestation `sig_data` no domain separator (same root as L-04). |
| L-32 | `core/economics/src/pmfp.rs` | 35–40 | PMFP emission window comment wrong — dust accumulation not documented. |
