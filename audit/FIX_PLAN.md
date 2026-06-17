# RupeeCoin — Bug Fix Plan

**Date:** 2026-06-17  
**Total bugs to fix:** 191 (26 Critical · 65 High · 68 Medium · 32 Low)  
**Reference:** `audit/BUGS_REPORT.md`

This plan is organized into 5 phases. Each phase must complete and pass `cargo test` before the next begins. Fix ordering within a phase is sequential — later fixes may depend on earlier ones.

---

## Phase A — Chain Integrity (Critical, 1–2 days)
*Stop-the-chain bugs. Nothing else matters until these are fixed.*

### A-1: Privacy proof verification (C-01 through C-04)

**Files:** `core/privacy/src/transaction.rs`, `core/privacy/src/proof.rs`

```
C-01  transaction.rs:233  verify_batch_stark() must call verify_spend_batch()
C-02  proof.rs:262        BalanceProof::verify — saturating_add → checked_add
C-03  transaction.rs:214  .sum() wrapping — use try_fold + checked_add + MAX_NOTE_VALUE per input
C-04  proof.rs:206        BatchedStark anchor cross-check against outer PrivacyTransaction.anchor
```

**Steps:**

1. `core/privacy/src/proof.rs` line 262:
   ```rust
   // BEFORE
   self.total_input == self.total_output.saturating_add(self.fee)
   // AFTER
   self.total_output
       .checked_add(self.fee)
       .map_or(false, |rhs| self.total_input == rhs)
   ```

2. `core/privacy/src/transaction.rs` lines 214–222:
   ```rust
   let total_in: u64 = self.shielded_inputs.iter()
       .try_fold(0u64, |acc, i| {
           if i.value == 0 || i.value > MAX_NOTE_VALUE { return None; }
           acc.checked_add(i.value)
       })
       .ok_or(PrivacyTxError::InvalidBalance)?;
   let total_out: u64 = self.shielded_outputs.iter()
       .try_fold(0u64, |acc, o| acc.checked_add(o.value))
       .ok_or(PrivacyTxError::InvalidBalance)?;
   ```

3. `core/privacy/src/transaction.rs` lines 233–253 — add at end of `verify_batch_stark`:
   ```rust
   rupeecoin_zkproof::verify_spend_batch(batch_proof)
       .map_err(|_| PrivacyTxError::InvalidSpendProof)
   ```
   And call `self.verify_batch_stark()?;` from `validate_stateless`.

4. `core/privacy/src/transaction.rs` in `validate_stateful` — after checking `self.anchor`:
   ```rust
   if let Some(bp) = &self.batch_stark_proof {
       let stark_anchor = Hash::from_anchor_felts(&bp.anchor_felts);
       if stark_anchor != self.anchor {
           return Err(PrivacyTxError::UnknownAnchor);
       }
   }
   ```

---

### A-2: Block proposer authentication (C-05)

**File:** `core/blockchain/src/lib.rs` lines 427–441

```rust
// BEFORE
if let Some(proposer) = registry.get(&proposer_addr) {
    proposer.pubkey.verify(...)?;
}
// AFTER
let proposer = registry.get(&proposer_addr)
    .ok_or(ChainError::UnknownProposer(proposer_addr))?;
proposer.pubkey.verify(block.hash().as_bytes(), sig)
    .map_err(|_| ChainError::InvalidProposerSignature)?;
if block.vrf_proof.is_none() { ... }
if !verify_block_vrf(block, ...) { ... }
```

---

### A-3: Checkpoint validation (C-07)

**File:** `core/consensus/src/checkpoints.rs` lines 57–61

Replace `is_before_checkpoint` with:
```rust
pub fn is_valid_checkpoint_block(&self, block: &Block) -> bool {
    self.trusted.iter().any(|cp| {
        cp.height == block.header.height
            && cp.block_hash == block.hash()
            && cp.state_root == block.header.state_root
    })
}
```
Update all callers to pass the full `Block` reference.

---

### A-4: Fix DoubleProposal same-slot check (C-08)

**File:** `core/blockchain/src/lib.rs` lines 501–515

After verifying both signatures, add:
```rust
let slot_a = decode_slot_from_hash(&block_hash_a);
let slot_b = decode_slot_from_hash(&block_hash_b);
if slot_a != slot_b || slot_a != evidence.slot {
    return Err(ChainError::InvalidSlashingReport("slots do not match".into()));
}
```

---

### A-5: Fix slashing detector signature verification (C-26)

**File:** `core/consensus/src/slashing.rs` lines 25–48, 53–66

```rust
// record_proposal
proposer_pubkey.verify(block_hash.as_bytes(), &signature)
    .map_err(|_| return None)?;
self.seen_proposals.insert(slot, (block_hash, proposer_pubkey, signature));

// record_attestation
let sig_data = Attestation::sig_data(att.slot, &att.block_hash);
att.validator_pubkey.verify(sig_data.as_bytes(), &att.signature)
    .map_err(|_| return None)?;
```

---

### A-6: UTXO multi-input nonce (C-09, C-10, C-11, C-12, C-13, C-15, C-18)

**File:** `core/utxo/src/lib.rs`

1. **C-09/C-10** — loop nonce check and increment over all unique sender addresses:
   ```rust
   // validate_transfer: check nonce for ALL inputs
   for input in &tx.inputs {
       let addr = input.public_key.to_address();
       let expected = self.nonces.get(&addr).copied().unwrap_or(0);
       if input.nonce != expected {
           return Err(UtxoError::InvalidNonce { addr, expected, got: input.nonce });
       }
   }
   // apply: increment for ALL unique senders
   let mut seen = HashSet::new();
   for input in &tx.inputs {
       let addr = input.public_key.to_address();
       if seen.insert(addr.clone()) {
           *self.nonces.entry(addr).or_insert(0) += 1;
       }
   }
   ```

2. **C-11** — merge validation into `apply_block`; make it mandatory:
   ```rust
   pub fn apply_block(&mut self, block: &Block) -> Result<(), UtxoError> {
       self.validate_block(block)?;  // must call; cannot skip
       self.apply_validated_block(block)
   }
   ```

3. **C-12** — replace all `.expect()` with `ok_or(UtxoError::Overflow)?`

4. **C-13** — coinbase maturity:
   ```rust
   let maturity_height = lock_height
       .checked_add(COINBASE_MATURITY)
       .ok_or(UtxoError::Overflow)?;
   ```

5. **C-15** — mempool UTXO fee:
   ```rust
   let input_sum: u64 = tx.inputs.iter()
       .map(|i| utxo_set.get(&i.outpoint)
           .ok_or(MempoolError::UnknownInput(i.outpoint.clone())))
       .collect::<Result<Vec<_>, _>>()?
       .iter().map(|o| o.amount).sum();
   ```

6. **C-18** — `node/src/rpc.rs` `submit_tx`:
   ```rust
   // Add body size limit to router
   .layer(DefaultBodyLimit::max(1_048_576))  // 1 MB
   // In handler: validate before queuing
   if tx.chain_id != expected_chain_id { return Err(...); }
   if tx.inputs.is_empty() { return Err(...); }
   // cap the queue
   if state.pending_txs.lock().unwrap().len() >= 4096 {
       return Err(StatusCode::TOO_MANY_REQUESTS);
   }
   ```

---

### A-7: Supply cap enforcement (C-14)

**File:** `core/economics/src/rewards.rs` lines 70–86

```rust
pub fn compute_block_rewards(
    height: u64,
    current_supply: u64,    // add this parameter
    ...
) -> BlockRewards {
    let gross = block_reward(height)
        .min(MAX_SUPPLY.saturating_sub(current_supply));
    ...
}
```
Update all callers to pass `chain.total_supply()`.

---

### A-8: Staking fixed-point arithmetic (C-16, C-17)

**File:** `core/staking/src/lib.rs`

Replace `f64` multiplier with integer fixed-point (×1000 scale):

```rust
// In ContributionMetrics
pub fn multiplier_scaled(&self) -> u64 {
    // returns integer in [500, 1500] representing [0.5×, 1.5×]
    let s = self.score();  // must be rewritten in integer arithmetic
    (500 + (s * 1000) as u64).clamp(500, 1500)
}

// In Validator
pub fn effective_weight(&self) -> u64 {
    self.stake.saturating_mul(self.contribution.multiplier_scaled()) / 1000
}

// total_effective_weight returns u128
pub fn total_effective_weight(&self) -> u128 {
    self.active_set.iter()
        .filter_map(|a| self.validators.get(a))
        .map(|v| v.effective_weight() as u128)
        .sum()
}
```

Rewrite `score()` and `piecewise_multiplier()` in integer arithmetic (multiply by 1000 throughout).

---

### A-9: JS SDK address version and hash (C-23)

**File:** `sdk-js/src/address.ts`

```typescript
const ADDRESS_VERSION = 0x52;  // was 0x12

// Replace sha256 checksum with BLAKE3:
// Either add blake3-wasm dependency or expose from Rust SDK via WASM
function checksum(data: Uint8Array): Uint8Array {
    // blake3(blake3(data))[0..4]
    return blake3(blake3(data)).slice(0, 4);
}
```

---

### A-10: coins_to_micro precision fix (C-24)

**File:** `rupeecoin-sdk/src/tx.rs`, `wallet/src/main.rs`, `desktop-wallet/src/wallet_commands.rs`, `faucet/`

```rust
pub fn coins_to_micro(amount: f64) -> Result<u64, &'static str> {
    if !amount.is_finite() || amount < 0.0 {
        return Err("amount must be a finite non-negative number");
    }
    let micro = (amount * MICRO_RPC_PER_COIN as f64).round();
    if micro > u64::MAX as f64 { return Err("amount too large"); }
    Ok(micro as u64)
}
```

---

### A-11: State root must include privacy state (C-25)

**File:** `core/blockchain/src/lib.rs` lines 716–720

```rust
let state_root = {
    let mut snap = chain.utxo_set.clone();
    snap.apply_transactions(&transactions);
    let utxo_root = snap.state_root();
    let privacy_root = chain.privacy_state.commitment_tree.root();
    let nullifier_root = chain.privacy_state.nullifier_root();
    Hash::merge_three(&utxo_root, &privacy_root, &nullifier_root)
};
```

---

## Phase B — Consensus & Chain Safety (High/Critical, 2–3 days)

### B-1: Attestation signature verification in blocks (H-11)

**File:** `core/blockchain/src/lib.rs` in `validate()`

```rust
for att in &block.attestations {
    let sig_data = Attestation::sig_data(att.slot, &att.block_hash);
    att.validator_pubkey
        .verify(sig_data.as_bytes(), &att.signature)
        .map_err(|_| ChainError::InvalidAttestationSignature)?;
}
```

Also fix `on_block_accepted`: change `let _ = self.attestation_pool.add(...)` to propagate/log the error.

---

### B-2: LMD-GHOST cycle protection (H-12)

**File:** `core/consensus/src/lib.rs` lines 342–351

```rust
fn is_in_subtree(&self, mut hash: Hash, root: Hash) -> bool {
    let mut visited = HashSet::new();
    loop {
        if hash == root { return true; }
        if !visited.insert(hash) { return false; } // cycle
        match self.known_block_parents.get(&hash) {
            Some(&parent) => hash = parent,
            None => return false,
        }
    }
}
```

---

### B-3: Privacy nullifier rollback on reorg (H-08)

**File:** `core/blockchain/src/lib.rs`, `core/blockchain/src/delta.rs`

Add to `BlockDelta`:
```rust
pub struct BlockDelta {
    // existing fields...
    pub inserted_nullifiers: Vec<Hash>,
    pub inserted_commitments: Vec<NoteCommitment>,
}
```

In `rollback()`:
```rust
for nullifier in &delta.inserted_nullifiers {
    self.privacy_state.spent_nullifiers.remove(nullifier);
}
// Remove inserted commitments from tree (requires tree to support truncation)
```

As an interim measure: refuse reorgs that include blocks with `privacy_txs.len() > 0` beyond `REORG_DEPTH`.

---

### B-4: Equivocation escalation (H-13)

**File:** `core/consensus/src/lib.rs` lines 130–134

When duplicate attestation is detected for a *different* block hash:
```rust
if existing_hash != att.block_hash {
    // Escalate to slashing detector instead of dropping
    if let Some(evidence) = self.slashing_detector.record_attestation(att.clone()) {
        self.pending_slashing.push(evidence);
    }
    return Err(ConsensusError::DuplicateAttestation(att.slot));
}
```

---

### B-5: Casper FFG supermajority link (H-14)

**File:** `core/consensus/src/finality.rs` lines 43–48

Implement a proper source→target link check:
```rust
// justify() now requires a source checkpoint
pub fn justify(&mut self, epoch: u64, block_hash: Hash, source_epoch: u64, ...) {
    // Only justify if source_epoch == last_justified_epoch - 1 or 0
    if source_epoch + 1 != epoch && source_epoch != 0 { return; }
    ...
}
```

---

### B-6: Inactivity leak fix (H-15, H-16)

**File:** `core/economics/src/inactivity.rs`

```rust
// Per-epoch penalty only (function called once per epoch boundary)
let penalty = stake / INACTIVITY_PENALTY_DIVISOR;  // one epoch's penalty
registry.slash(&addr, penalty);

// Fix overflow
let exit_block = current_epoch.saturating_mul(32);
let _ = registry.request_exit(&addr, exit_block);
```

---

### B-7: Reward claim signature check (H-10, C-06)

**File:** `core/blockchain/src/lib.rs` lines 468–488

```rust
// validate_reward_claim must verify validator signed the claim
let sig_data = reward_claim_sig_data(addr, claimed, height);
validator_pubkey.verify(&sig_data, &tx_signature)
    .map_err(|_| ChainError::InvalidRewardClaimSignature)?;
```

---

### B-8: Slashing prune + bloom (M-62, M-63)

**File:** `core/consensus/src/slashing.rs`, `core/consensus/src/lib.rs`

In `on_block_accepted`:
```rust
// After pruning attestation pool
let prune_slot = block.header.slot.saturating_sub(SLOTS_PER_EPOCH * 2);
self.slashing_detector.prune(prune_slot);
```

In `SlashingDetector::prune`:
```rust
pub fn prune(&mut self, before_slot: u64) {
    self.seen_proposals.retain(|&slot, _| slot >= before_slot);
    self.seen_attestations.retain(|&slot, _| slot >= before_slot);
}
```

---

### B-9: Gas metering with actual gas (H-42)

**File:** `core/blockchain/src/lib.rs` lines 343–349

Replace 1-gas-per-tx with per-tx RVM gas execution:
```rust
let gas_used: u64 = block.transactions.iter().skip(1)  // skip coinbase
    .map(|tx| tx.gas_limit.min(MAX_GAS_PER_TX))
    .sum();
```
For contract txs, use the `gas_used` returned from `run()`.

---

### B-10: Genesis timestamp parameter fix (H-64)

**File:** `core/blockchain/src/genesis.rs` line 63

```rust
let header = BlockHeader {
    // ...
    timestamp,  // use the parameter, not the hardcoded value
    // ...
};
```

---

## Phase C — Token VM & EVM Transpiler (High/Medium, 1–2 days)

### C-1: RPC-20 transferFrom bugs (H-18, H-19, H-53)

**File:** `core/rvm/src/token/rpc20.rs`

1. Fix allowance slot multiplier to match `token/mod.rs::allowance_slot` (use `0x9e37_79b9_7f4a_7c15` for spender).

2. Fix allowance check off-by-one (revert if `amount > allowance`, not `amount >= allowance`):
   ```rust
   // Replace Lt+IsZero with Gt:
   a.op(Op::Dup3).op(Op::Gt);   // allowance > amount?
   a.op(Op::IsZero).push_label("transferFrom_revert").op(Op::JumpI);
   ```

3. Move balance check **before** the allowance `SStore` — check all preconditions before any state write.

4. Fix balance check off-by-one (revert if `from_balance < amount`):
   ```rust
   a.op(Op::Dup3).op(Op::Lt);   // amount < from_balance?
   a.op(Op::IsZero).push_label("transferFrom_revert").op(Op::JumpI);
   ```

5. Fix recipient balance overflow — use `Op::AddOv` and revert on overflow flag.

---

### C-2: RPC-721 mint and RPC-1155 auth (H-24, H-25)

**File:** `core/rvm/src/token/rpc721.rs`, `core/rvm/src/token/rpc1155.rs`

At the start of `mint` in RPC-721, insert caller == owner check:
```rust
a.op(Op::SelfAddr).op(Op::SLoad)  // load owner from slot 0
 .op(Op::Caller).op(Op::Eq)
 .op(Op::IsZero).push_label("mint_revert").op(Op::JumpI);
```

In RPC-1155 `burn` and `transfer`, add `Op::Caller == from` check before any state write.

---

### C-3: VM opcode bugs (H-20, H-21, H-22, H-23, M-09)

**File:** `core/rvm/src/vm.rs`, `core/rvm/src/opcode.rs`

1. `AddOv/SubOv/MulOv` — pre-check room for net +1 before executing:
   ```rust
   // After popping 2 values, before pushing 2:
   if self.stack.len() + 1 >= STACK_LIMIT {
       return Err(VmError::StackOverflow);
   }
   ```

2. `Exp` exponent truncation:
   ```rust
   let result = if exp > u32::MAX as u64 { 0u64 }
                else { base.wrapping_pow(exp as u32) };
   ```

3. `SAR` shift threshold:
   ```rust
   let result = if shift >= 64 { val >> 63 } else { val >> shift };
   ```

4. `MulOv` gas:
   ```rust
   Mul | Div | Sdiv | Mod | Smod | MulOv => 5,  // was in 3-gas group
   ```

5. `LOG0..LOG4` static check:
   ```rust
   Op::Log0 | Op::Log1 | Op::Log2 | Op::Log3 | Op::Log4 => {
       if s.is_static { return Err(VmError::WriteProtection); }
       ...
   }
   ```

---

### C-4: EVM transpiler missing opcodes (M-10, M-11)

**File:** `evm2rvm/src/main.rs`

```rust
const EVM_SAR: u8 = 0x1D;
const EVM_CALLDATALOAD: u8 = 0x35;
const EVM_CALLDATASIZE: u8 = 0x36;
const EVM_CALLDATACOPY: u8 = 0x37;

// In match:
EVM_SAR => { rvm.push(Op::Sar as u8); }
EVM_CALLDATALOAD | EVM_CALLDATASIZE | EVM_CALLDATACOPY => {
    // Map to memory reads; emit warning:
    eprintln!("WARNING: CALLDATALOAD/SIZE/COPY: using memory-at-0 approximation");
    rvm.push(Op::MLoad as u8);
}
```

---

## Phase D — Node, Wallet & Infrastructure (High/Medium, 2–3 days)

### D-1: RPC server hardening (H-31, H-32, H-49, M-30, M-31, M-32, M-33)

**File:** `node/src/rpc.rs`

```rust
// Router setup — add body limit and routing guards
let app = Router::new()
    ...
    .layer(DefaultBodyLimit::max(512 * 1024))  // 512 KB

// submit_tx — add validation
async fn submit_tx(State(state): ..., body: Bytes) -> impl IntoResponse {
    if body.len() > 1_048_576 { return Err(StatusCode::PAYLOAD_TOO_LARGE); }
    let tx: Transaction = bincode::deserialize(&raw)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    if tx.inputs.is_empty() { return Err(StatusCode::UNPROCESSABLE_ENTITY); }
    if tx.chain_id != state.chain_id { return Err(StatusCode::BAD_REQUEST); }
    {
        let mut q = state.pending_txs.lock().unwrap();
        if q.len() >= 4096 { return Err(StatusCode::TOO_MANY_REQUESTS); }
        q.push(tx);
    }
    Ok(Json(json!({"status": "accepted"})))
}

// serve() — return Result
pub async fn serve(state: Arc<RpcState>, port: u16) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await
        .with_context(|| format!("binding port {port}"))?;
    axum::serve(listener, app).await.context("axum serve")?;
    Ok(())
}

// get_utxos — fix unwrap
let bytes = hex::decode(&params.addr)
    .map_err(|_| RpcError("invalid hex address".into()))?;

// get_shielded_outputs — cap count
let count = params.count.min(1000);

// get_merkle_path — bounds check
if params.index >= snap.commitment_tree.len() as u64 {
    return Err(RpcError(format!("index {} out of range", params.index)));
}
let mp = snap.commitment_tree.auth_path(params.index as usize)...

// bincode note encoding — propagate error
let enc_bytes = bincode::serialize(&out.encrypted_note)
    .map_err(|e| RpcError(format!("serialize note: {e}")))?;
```

---

### D-2: Wallet nonce fix (H-37)

**File:** `wallet/src/main.rs` lines 562–564

```rust
// Replace:
let nonce = ks.hd_receive_index as u64;
// With:
let nonce = rpc.nonce(&my_addr_hex)?;
```

Add `fn nonce(&self, addr: &str) -> Result<u64>` to `NodeRpc` in `wallet/src/node_rpc.rs`.

---

### D-3: Key material zeroization (H-33, H-34, H-35)

**File:** `wallet/src/keystore.rs`

Add to `Cargo.toml`: `zeroize = { version = "1", features = ["derive"] }`

```rust
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

#[derive(ZeroizeOnDrop)]
pub struct LoadedKeystore {
    pub bundle: PrivacyKeyBundle,
    pub hd_master_seed: Option<[u8; 32]>,
    pub hd_receive_index: u32,
}

fn derive_key(password: &[u8], salt: &[u8]) -> Zeroizing<[u8; ARGON2_OUT_LEN]> {
    let mut key = Zeroizing::new([0u8; ARGON2_OUT_LEN]);
    argon2.hash_password_into(password, salt, key.as_mut()).expect("argon2");
    key
}

// In save/load:
let password = Zeroizing::new(rpassword::prompt_password("Passphrase: ")?);
```

---

### D-4: Storage key endianness fix (H-30, M-26)

**File:** `core/storage/src/lib.rs`

```rust
fn contract_storage_key(contract_addr: u64, slot: u64) -> [u8; 16] {
    let mut key = [0u8; 16];
    key[..8].copy_from_slice(&contract_addr.to_be_bytes());  // was to_le_bytes
    key[8..].copy_from_slice(&slot.to_be_bytes());           // was to_le_bytes
    key
}

// Also fix save/load_contract_code:
let key = contract_addr.to_be_bytes();  // was to_le_bytes
```

---

### D-5: Network message size limit (H-28)

**File:** `core/network/src/lib.rs` and `core/network/src/dandelion.rs`

```rust
const MAX_SYNC_MESSAGE_SIZE: usize = 4 * 1024 * 1024;  // 4 MiB

// In SyncCodec::read_request, read_response, StemCodec::read_request:
let length = u32::from_be_bytes(len) as usize;
if length > MAX_SYNC_MESSAGE_SIZE {
    return Err(io::Error::new(io::ErrorKind::InvalidData,
        format!("message too large: {length} bytes (max {MAX_SYNC_MESSAGE_SIZE})")));
}
let mut buf = vec![0u8; length];
```

---

### D-6: mDNS allowlist check (H-29)

**File:** `core/network/src/lib.rs` lines 582–591

```rust
for (peer_id, addr) in &peers {
    if is_allowlisted(allowlist.as_ref(), peer_id) {
        info!("mDNS: adding allowlisted peer {peer_id} at {addr}");
        swarm.behaviour_mut().gossipsub.add_explicit_peer(peer_id);
    } else {
        trace!("mDNS: ignoring non-allowlisted peer {peer_id}");
    }
}
```

---

### D-7: Anonymous gossip message rejection (H-26)

**File:** `core/network/src/lib.rs` lines 404–413

```rust
let src = match message.source {
    Some(s) => s,
    None => {
        warn!("gossipsub: dropping anonymous message (no source PeerId)");
        return;
    }
};
if !is_allowlisted(allowlist.as_ref(), &src) { return; }
if is_rate_limited(peer_rate, &src) { return; }
```

---

### D-8: Faucet security hardening (C-19, C-20, C-21, H-65, M-42, M-43)

**File:** `faucet/src/main.rs`

1. Encrypt seed using keystore v3 (Argon2id + XChaCha20):
   ```rust
   // Use same pattern as wallet/src/keystore.rs
   // Read passphrase from FAUCET_KEY_PASSPHRASE env var at startup
   ```

2. Fetch nonce at startup:
   ```rust
   let nonce = rpc.nonce(&faucet_addr_hex)?;
   ```

3. Add per-address rate limit:
   ```rust
   struct FaucetMutable {
       nonce: u64,
       ip_limits: HashMap<IpAddr, u64>,
       addr_limits: HashMap<[u8; 20], u64>,
   }
   ```

4. Release mutex before network I/O:
   ```rust
   let (nonce, allowed) = { let m = f.mutable.lock().await; (m.nonce, check_limits(&m)) };
   // do I/O without lock
   // re-acquire to commit:
   { let mut m = f.mutable.lock().await; m.nonce += 1; record_limits(&mut m); }
   ```

5. Fix innerHTML XSS:
   ```javascript
   const txidEl = document.createElement('span');
   txidEl.textContent = 'txid: ' + d.txid;  // textContent, not innerHTML
   ```

---

### D-9: Desktop wallet hardening (H-40, H-41, H-52, M-44, M-45)

**File:** `desktop-wallet/src/wallet_commands.rs`, `desktop-wallet/src/main.rs`

1. Fix coin selection to deduct fee:
   ```rust
   let fee_micro: u64 = 10_000;  // or fetch from node
   let needed = amount_micro.checked_add(fee_micro)
       .ok_or_else(|| "amount + fee overflow".to_string())?;
   // coin select until total_in >= needed
   let change = total_in - amount_micro - fee_micro;
   ```

2. Store RPC URL in Tauri state — not per-command:
   ```rust
   // In main.rs:
   .manage(NodeConfig { rpc_url: args.rpc_url })
   // Remove rpc_url from all command signatures
   ```

3. Fix `receive_index` overflow:
   ```rust
   inner.receive_index = inner.receive_index
       .checked_add(1).ok_or_else(|| "index overflow".to_string())?;
   ```

4. Atomic keystore write:
   ```rust
   let tmp = format!("{}.tmp", path);
   std::fs::write(&tmp, &json).map_err(|e| e.to_string())?;
   std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
   ```

---

### D-10: Explorer/Dashboard fixes (M-39, M-40, M-41)

**Files:** `explorer/src/main.rs`, `dashboard/src/main.rs`

1. Refresh tx index after `try_catch_up_with_primary()`:
   ```rust
   let old_tip = state.last_indexed_height.load(Ordering::Relaxed);
   state.store.try_catch_up_with_primary().ok();
   let new_tip = state.store.height().unwrap_or(0);
   if new_tip > old_tip {
       refresh_tx_index(&state, old_tip + 1, new_tip).await;
       state.last_indexed_height.store(new_tip, Ordering::Relaxed);
   }
   ```

2. Normalize txid to lowercase before lookup:
   ```rust
   let txid_hex = txid_hex.to_lowercase();
   ```

3. URL-encode + validate address in dashboard:
   ```rust
   if !addr.chars().all(|c| c.is_ascii_hexdigit()) || addr.len() != 40 {
       return Err("invalid address format");
   }
   let url = format!("{}/rpc/validator_info?addr={}", self.rpc, addr);
   ```

---

### D-11: SDK fixes (C-23, C-24, H-45, H-46, H-47, M-47, M-48, M-50, M-51, M-52)

**Files:** `sdk-js/src/`, `rupeecoin-sdk/src/`, `sdk-py/src/`

1. **Rust SDK** — add `nonce()` to `RpcClient`; add connect/read timeout; validate `chain_id` in `build_transfer`.

2. **JS SDK** — fix address version; switch to `json-bigint` for u64 fields; add `AbortController` timeout; fix txid/recipient length validation; `PreSignedSigner` should assert message matches.

3. **Python SDK** — export `TESTNET_CHAIN_ID`; add `py.allow_threads(||...)` around HTTP calls; add `key_index` parameter to `build_tx`.

4. **`advance_receive_index`** — use `checked_add(1).ok_or(...)?`.

---

## Phase E — Medium/Low Cleanups (1–2 days)

### E-1: Cryptographic cleanups (L-01, L-02, L-03, L-04, M-59)

- `hash_concat` — add length-prefixed domain separation.
- HD derivation — add hardened child support; return `Result` from `child()` on depth overflow.
- Attestation/announcement `sig_data` — prefix with type tag.
- VRF — switch to a deterministic construction (document as future work if full redesign needed).

### E-2: Types cleanups (M-54, M-55, M-56, M-64, L-05)

- `contract.rs` `encode()` — return `Result` instead of `.expect()`.
- `genesis.rs` `validate()` — reject `halving_interval == 0` and `treasury == Address::zero()`.
- `contract_addr` — change from `u64` to `Address`.
- `transaction.rs` — add length to domain prefix to avoid collision with coinbase height bytes.

### E-3: UTXO/state cleanups (M-57, M-58, M-68, L-06)

- Zero-input non-coinbase tx — reject in `validate_transfer`.
- `state_root()` — return `Result`; allocate once per call.
- `decrement_nonce` — log a warning on underflow attempt instead of silent clamp.

### E-4: Staking / economics cleanups (L-07, L-08, L-09, M-12, M-13, M-14, M-15, M-16)

- Contribution counters — use `saturating_add`.
- `total_active_stake` — accumulate in `u128`.
- Supply comment — correct "Era 4+" to "Era 3+ (3 halvings)".
- Fee split — compute `pmfp = total * 30/100`; `burned = total - pmfp`.
- PMFP dust correction — credit unspent dust to highest-weight validator.
- Mempool eviction — use fee/size ratio; check intra-batch UTXO conflicts in `drain_for_block`.
- Staking `score()` — define distinct uptime metric or collapse weights accurately.

### E-5: Consensus/storage cleanups (M-18, M-19, M-22, M-23, M-24, M-25, M-27, M-28, M-29, M-61, M-62, M-63)

- Epoch boundary off-by-one — fix `slot % SLOTS_PER_EPOCH == 0` check.
- Proposer modulo bias — use rejection sampling for large validator sets.
- Rate limit off-by-one — reset counter to 0 on window reset.
- `pending_sync` cap — max 64 entries; drop oldest on overflow.
- `enter_stem` terminal hop — don't call for immediately-fluffed txs.
- SOCKS5 unknown ATYP — return error.
- `note_count` atomicity — use RocksDB merge operator or mutex.
- `flush()` on shutdown — add `Drop` impl to `ChainStore`.
- Address index — add `CF_ADDRESS_INDEX` for O(1) UTXO-by-address lookup.
- `justified_checkpoints` pruning — remove entries older than `last_finalized_epoch - 1`.
- `SlashingDetector::prune` — call from `on_block_accepted`.

### E-6: RVM/assembler cleanups (L-14, L-15, M-08, M-09)

- `build()` — return `Result` instead of panicking on undefined label.
- `Revert` return — use `Option<HashMap>` to prevent callers from accidentally wiping storage.

### E-7: Wallet/node minor fixes (L-17, L-18, L-19, L-20, L-21, M-30, M-34, M-35, M-36, M-37)

- Tor reachability — check `buf[1] == 0x00`.
- `unwrap_or_default` on note serialization — propagate error.
- Note selection — sort ascending by value before selecting.
- Accumulator overflows — use `saturating_add` in `cmd_scan`; `checked_add` in `select_notes`.
- Fork block cache — LRU eviction of entries below `chain.height() - REORG_DEPTH`.
- Reorg ancestor — look up from chain directly.
- Nullifier-check errors — skip note with warning instead of including.

### E-8: Infrastructure minor fixes (L-22, L-23, L-24, L-25, L-26, M-44, M-46)

- Explorer + faucet bind — add `--bind <ip>` flags; default `127.0.0.1`.
- Test tempdir leak — return `(TempDir, String)` from `tmp_path()`.
- Dashboard display — use integer arithmetic for RPC display.
- Keystore version check — return informative error on v1/v2.
- Atomic keystore writes — write to `.tmp` then rename (mobile wallet too).
- Faucet `saturating_add` — use `checked_add` throughout coin selection.

### E-9: Privacy / zkproof cleanups (H-04, H-05, H-06, M-01, M-02, M-03, M-04, M-05, M-06, M-07, L-13)

- `Blake3Path` — deprecate with `#[deprecated]`; gate behind `#[cfg(test)]`.
- `generate()` — call `from_seed(&random_seed)` to keep all keys co-derived.
- Goldilocks reduction — document and enforce `[0, P)` domain for inputs.
- Poseidon MDS — replace with a proper branch-number-9 matrix (reference from Poseidon paper).
- `MerklePath` length — enforce `path.len() == TREE_DEPTH` in `verify()`.
- Stored `balance_proof` — validate against re-derived values or remove field.
- `StarkProofPlaceholder::verify` — gate behind `#[cfg(test)]` or panic.
- Kyber keygen — add link-time size assertions; change `assert_eq!` to `Result`.
- `note_enc_key`/`note_nonce` — use same BLAKE3 KDF API for both.
- `from_seed` — expand to 64-byte intermediate before splitting.
- `auth_path` O(N) — plan to implement ommers-based incremental tree (Phase 16 work).

---

## Testing Checklist

After each phase, run:

```bash
# Full test suite
cargo test --workspace

# Specific crate tests
cargo test -p rupeecoin-utxo
cargo test -p rupeecoin-consensus
cargo test -p rupeecoin-privacy
cargo test -p rupeecoin-zkproof
cargo test -p rupeecoin-rvm
cargo test -p rupeecoin-staking
cargo test -p rupeecoin-economics
cargo test -p rupeecoin-network
cargo test -p rupeecoin-storage
cargo test -p rupeecoin-sdk

# Integration tests
cargo test -p rupeecoin-tests

# Benchmarks (verify no regressions)
cargo bench --workspace

# JS SDK tests (after Phase D)
cd sdk-js && npm test
```

New tests to write for each phase:
- **Phase A:** Test zero-input tx rejection; test max-supply cap; test multi-input nonce uniqueness; test shielded balance overflow; test STARK verification called.
- **Phase B:** Test attestation sig verification in blocks; test LMD-GHOST cycle resistance; test inactivity drain ≤ 1 epoch per call; test checkpoint hash mismatch rejection.
- **Phase C:** Test `transferFrom` with exact allowance (should succeed); test mint auth rejection; test `SAR` with shift=64; test transpiler SAR mapping.
- **Phase D:** Test `submit_tx` with zero inputs rejected; test RPC body limit; test nonce from chain; test atomic keystore write.
- **Phase E:** Parameterized `coins_to_micro` tests (negative, NaN, 0.1, infinity); test tx index refreshes on new blocks; test `advance_receive_index` at `u32::MAX`.

---

## Estimated Timeline

| Phase | Focus | Effort |
|-------|-------|--------|
| A | Chain integrity (Critical) | 2 days |
| B | Consensus & chain safety (High) | 3 days |
| C | Token VM & transpiler (High/Medium) | 2 days |
| D | Node, wallet & infra (High/Medium) | 3 days |
| E | Medium/Low cleanups | 2 days |
| **Total** | | **~12 days** |

**Recommended order for solo developer:** A → B → C → D → E, one PR per phase, all tests green before merging.
