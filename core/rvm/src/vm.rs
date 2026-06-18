use std::collections::{HashMap, HashSet};

use rupeecoin_crypto::hash_data;
use thiserror::Error;

use crate::opcode::Op;

pub const STACK_LIMIT: usize = 1024;
pub const MEMORY_LIMIT: usize = 32 * 1024 * 1024; // 32 MiB

/// A log entry emitted by LOG0–LOG4.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogEntry {
    pub address: u64,
    pub topics: Vec<u64>,
    pub data: Vec<u8>,
}

/// Execution context supplied by the calling layer (node / test harness).
#[derive(Clone, Debug, Default)]
pub struct ExecContext {
    pub caller: u64,
    pub call_value: u64,
    pub block_height: u64,
    pub timestamp: u64,
    pub chain_id: u64,
    pub self_addr: u64,
    pub gas_limit: u64,
    /// Call data placed in memory starting at offset 0 before execution.
    pub calldata: Vec<u8>,
}

/// Outcome of a successful or reverted execution.
#[derive(Debug, PartialEq, Eq)]
pub enum ExecResult {
    /// Contract returned normally; `data` is the return payload.
    Return { data: Vec<u8>, gas_used: u64 },
    /// Contract reverted; `data` is the revert payload.
    Revert { data: Vec<u8>, gas_used: u64 },
}

/// Fatal execution errors (gas exhaustion, bad jumps, …).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum VmError {
    #[error("out of gas")]
    OutOfGas,
    #[error("stack underflow at opcode 0x{op:02X}")]
    StackUnderflow { op: u8 },
    #[error("stack overflow")]
    StackOverflow,
    #[error("invalid jump destination 0x{dest:X}")]
    InvalidJump { dest: u64 },
    #[error("write to static context (SSTORE in STATICCALL)")]
    WriteProtection,
    #[error("memory limit exceeded")]
    MemoryLimit,
    #[error("unknown opcode 0x{0:02X}")]
    UnknownOpcode(u8),
    #[error("division by zero")]
    DivisionByZero,
}

/// The full mutable state of a single contract execution frame.
pub struct VmState {
    pub stack: Vec<u64>,
    pub memory: Vec<u8>,
    pub storage: HashMap<u64, u64>,
    pub pc: usize,
    pub gas: u64,
    pub logs: Vec<LogEntry>,
    ctx: ExecContext,
    // Keys read in this execution (for warm/cold accounting — simplified flat model here)
    warm_slots: HashSet<u64>,
    is_static: bool,
    // Pre-computed set of valid JUMPDEST positions
    jumpdests: HashSet<usize>,
}

impl VmState {
    pub fn new(
        bytecode: &[u8],
        storage: HashMap<u64, u64>,
        ctx: ExecContext,
        is_static: bool,
    ) -> Self {
        let jumpdests = find_jumpdests(bytecode);
        let gas = ctx.gas_limit;
        let mut memory = ctx.calldata.clone();
        // Pad calldata in memory to 8-byte boundary for clean MLOAD alignment
        while memory.len() % 8 != 0 {
            memory.push(0);
        }
        VmState {
            stack: Vec::with_capacity(64),
            memory,
            storage,
            pc: 0,
            gas,
            logs: vec![],
            ctx,
            warm_slots: HashSet::new(),
            is_static,
            jumpdests,
        }
    }

    // ── stack helpers ────────────────────────────────────────────────────────

    fn push(&mut self, v: u64, op: u8) -> Result<(), VmError> {
        if self.stack.len() >= STACK_LIMIT {
            return Err(VmError::StackOverflow);
        }
        self.stack.push(v);
        let _ = op;
        Ok(())
    }

    fn pop(&mut self, op: u8) -> Result<u64, VmError> {
        self.stack.pop().ok_or(VmError::StackUnderflow { op })
    }

    fn peek(&self, depth: usize, op: u8) -> Result<u64, VmError> {
        let len = self.stack.len();
        if depth >= len {
            return Err(VmError::StackUnderflow { op });
        }
        Ok(self.stack[len - 1 - depth])
    }

    // ── gas helpers ──────────────────────────────────────────────────────────

    fn charge(&mut self, cost: u64) -> Result<(), VmError> {
        if cost > self.gas {
            return Err(VmError::OutOfGas);
        }
        self.gas -= cost;
        Ok(())
    }

    // ── memory helpers ───────────────────────────────────────────────────────

    fn mem_grow(&mut self, offset: usize, len: usize) -> Result<(), VmError> {
        let end = offset.saturating_add(len);
        if end > MEMORY_LIMIT {
            return Err(VmError::MemoryLimit);
        }
        if end > self.memory.len() {
            // charge 1 gas per new byte (simplified; EVM uses quadratic formula)
            let new_bytes = (end - self.memory.len()) as u64;
            self.charge(new_bytes)?;
            self.memory.resize(end, 0);
        }
        Ok(())
    }

    fn mload(&mut self, offset: usize) -> Result<u64, VmError> {
        self.mem_grow(offset, 8)?;
        let b: [u8; 8] = self.memory[offset..offset + 8].try_into().unwrap();
        Ok(u64::from_be_bytes(b))
    }

    fn mstore(&mut self, offset: usize, value: u64) -> Result<(), VmError> {
        self.mem_grow(offset, 8)?;
        self.memory[offset..offset + 8].copy_from_slice(&value.to_be_bytes());
        Ok(())
    }

    fn mstore8(&mut self, offset: usize, value: u8) -> Result<(), VmError> {
        self.mem_grow(offset, 1)?;
        self.memory[offset] = value;
        Ok(())
    }

    fn mem_slice(&mut self, offset: usize, len: usize) -> Result<Vec<u8>, VmError> {
        if len == 0 {
            return Ok(vec![]);
        }
        self.mem_grow(offset, len)?;
        Ok(self.memory[offset..offset + len].to_vec())
    }
}

fn find_jumpdests(code: &[u8]) -> HashSet<usize> {
    let mut set = HashSet::new();
    let mut i = 0;
    while i < code.len() {
        let b = code[i];
        if b == Op::JumpDest as u8 {
            set.insert(i);
        }
        // Skip immediate bytes for PUSH1..PUSH8
        if (0x60..=0x67).contains(&b) {
            i += 1 + (b - 0x5F) as usize;
        } else {
            i += 1;
        }
    }
    set
}

/// Run `bytecode` to completion, returning the final result.
///
/// The second element of the Ok tuple is `Some(storage)` on normal return/stop,
/// or `None` on `Revert` — callers must check for `None` before updating storage.
pub fn run(
    bytecode: &[u8],
    storage: HashMap<u64, u64>,
    ctx: ExecContext,
) -> Result<(ExecResult, Option<HashMap<u64, u64>>), VmError> {
    run_inner(bytecode, storage, ctx, false)
}

pub fn static_call(
    bytecode: &[u8],
    storage: HashMap<u64, u64>,
    ctx: ExecContext,
) -> Result<(ExecResult, Option<HashMap<u64, u64>>), VmError> {
    run_inner(bytecode, storage, ctx, true)
}

fn run_inner(
    bytecode: &[u8],
    storage: HashMap<u64, u64>,
    ctx: ExecContext,
    is_static: bool,
) -> Result<(ExecResult, Option<HashMap<u64, u64>>), VmError> {
    let gas_limit = ctx.gas_limit;
    let mut s = VmState::new(bytecode, storage, ctx, is_static);

    loop {
        if s.pc >= bytecode.len() {
            // implicit STOP
            let gas_used = gas_limit - s.gas;
            let storage = s.storage;
            return Ok((ExecResult::Return { data: vec![], gas_used }, Some(storage)));
        }

        let raw = bytecode[s.pc];
        let op = Op::try_from(raw).map_err(|_| VmError::UnknownOpcode(raw))?;
        s.charge(op.base_gas())?;
        s.pc += 1;

        match op {
            // ── stop ─────────────────────────────────────────────────────
            Op::Stop => {
                let gas_used = gas_limit - s.gas;
                let storage = s.storage;
                return Ok((ExecResult::Return { data: vec![], gas_used }, Some(storage)));
            }

            // ── arithmetic ────────────────────────────────────────────────
            Op::Add => {
                let (a, b) = (s.pop(raw)?, s.pop(raw)?);
                s.push(a.wrapping_add(b), raw)?;
            }
            Op::Sub => {
                let (a, b) = (s.pop(raw)?, s.pop(raw)?);
                s.push(a.wrapping_sub(b), raw)?;
            }
            Op::Mul => {
                let (a, b) = (s.pop(raw)?, s.pop(raw)?);
                s.push(a.wrapping_mul(b), raw)?;
            }
            Op::Div => {
                let (a, b) = (s.pop(raw)?, s.pop(raw)?);
                s.push(if b == 0 { 0 } else { a / b }, raw)?;
            }
            Op::Sdiv => {
                let (a, b) = (s.pop(raw)? as i64, s.pop(raw)? as i64);
                s.push(if b == 0 { 0 } else { a.wrapping_div(b) as u64 }, raw)?;
            }
            Op::Mod => {
                let (a, b) = (s.pop(raw)?, s.pop(raw)?);
                s.push(if b == 0 { 0 } else { a % b }, raw)?;
            }
            Op::Smod => {
                let (a, b) = (s.pop(raw)? as i64, s.pop(raw)? as i64);
                s.push(if b == 0 { 0 } else { (a % b) as u64 }, raw)?;
            }
            Op::Addmod => {
                let (a, b, n) = (s.pop(raw)? as u128, s.pop(raw)? as u128, s.pop(raw)? as u128);
                s.push(if n == 0 { 0 } else { ((a + b) % n) as u64 }, raw)?;
            }
            Op::Mulmod => {
                let (a, b, n) = (s.pop(raw)? as u128, s.pop(raw)? as u128, s.pop(raw)? as u128);
                s.push(if n == 0 { 0 } else { ((a * b) % n) as u64 }, raw)?;
            }
            Op::Exp => {
                let (base, exp) = (s.pop(raw)?, s.pop(raw)?);
                // charge extra: 5 gas per byte of exponent (simplified)
                let exp_bytes = if exp == 0 { 0 } else { 8 - (exp.leading_zeros() / 8) as u64 };
                s.charge(5 * exp_bytes)?;
                // H-21: exp > u32::MAX silently truncated in wrapping_pow; result is 0 per EVM spec
                let result = if exp > u32::MAX as u64 { 0u64 } else { base.wrapping_pow(exp as u32) };
                s.push(result, raw)?;
            }
            Op::AddOv => {
                let (a, b) = (s.pop(raw)?, s.pop(raw)?);
                // H-20: after two pops we push two values; check room for both before first push
                if s.stack.len() + 2 > STACK_LIMIT {
                    return Err(VmError::StackOverflow);
                }
                let (result, overflow) = a.overflowing_add(b);
                s.push(result, raw)?;
                s.push(overflow as u64, raw)?;
            }
            Op::SubOv => {
                let (a, b) = (s.pop(raw)?, s.pop(raw)?);
                if s.stack.len() + 2 > STACK_LIMIT {
                    return Err(VmError::StackOverflow);
                }
                let (result, overflow) = a.overflowing_sub(b);
                s.push(result, raw)?;
                s.push(overflow as u64, raw)?;
            }
            Op::MulOv => {
                let (a, b) = (s.pop(raw)?, s.pop(raw)?);
                if s.stack.len() + 2 > STACK_LIMIT {
                    return Err(VmError::StackOverflow);
                }
                let (result, overflow) = a.overflowing_mul(b);
                s.push(result, raw)?;
                s.push(overflow as u64, raw)?;
            }

            // ── comparison & bitwise ──────────────────────────────────────
            Op::Lt     => { let (a, b) = (s.pop(raw)?, s.pop(raw)?); s.push((a < b) as u64, raw)?; }
            Op::Gt     => { let (a, b) = (s.pop(raw)?, s.pop(raw)?); s.push((a > b) as u64, raw)?; }
            Op::Slt    => { let (a, b) = (s.pop(raw)? as i64, s.pop(raw)? as i64); s.push((a < b) as u64, raw)?; }
            Op::Sgt    => { let (a, b) = (s.pop(raw)? as i64, s.pop(raw)? as i64); s.push((a > b) as u64, raw)?; }
            Op::Eq     => { let (a, b) = (s.pop(raw)?, s.pop(raw)?); s.push((a == b) as u64, raw)?; }
            Op::IsZero => { let a = s.pop(raw)?; s.push((a == 0) as u64, raw)?; }
            Op::And    => { let (a, b) = (s.pop(raw)?, s.pop(raw)?); s.push(a & b, raw)?; }
            Op::Or     => { let (a, b) = (s.pop(raw)?, s.pop(raw)?); s.push(a | b, raw)?; }
            Op::Xor    => { let (a, b) = (s.pop(raw)?, s.pop(raw)?); s.push(a ^ b, raw)?; }
            Op::Not    => { let a = s.pop(raw)?; s.push(!a, raw)?; }
            Op::Shl    => { let (shift, val) = (s.pop(raw)?, s.pop(raw)?); s.push(if shift >= 64 { 0 } else { val << shift }, raw)?; }
            Op::Shr    => { let (shift, val) = (s.pop(raw)?, s.pop(raw)?); s.push(if shift >= 64 { 0 } else { val >> shift }, raw)?; }
            Op::Sar    => {
                let (shift, val) = (s.pop(raw)?, s.pop(raw)? as i64);
                // H-22: threshold must be 64 (not 63); SAR by 63 is a valid shift
                let result = if shift >= 64 { val >> 63 } else { val >> shift };
                s.push(result as u64, raw)?;
            }

            // ── crypto ────────────────────────────────────────────────────
            Op::Blake3 => {
                let (offset, len) = (s.pop(raw)? as usize, s.pop(raw)? as usize);
                // charge 6 gas per 8-byte chunk
                s.charge(6 * ((len as u64 + 7) / 8))?;
                let data = s.mem_slice(offset, len)?;
                let hash = hash_data(&data);
                // Push low 64 bits of BLAKE3 hash (big-endian first 8 bytes)
                let word = u64::from_be_bytes(hash.as_bytes()[..8].try_into().unwrap());
                s.push(word, raw)?;
            }

            // ── context ───────────────────────────────────────────────────
            Op::Caller    => { let v = s.ctx.caller;      s.push(v, raw)?; }
            Op::CallValue => { let v = s.ctx.call_value;  s.push(v, raw)?; }
            Op::Height    => { let v = s.ctx.block_height; s.push(v, raw)?; }
            Op::Timestamp => { let v = s.ctx.timestamp;   s.push(v, raw)?; }
            Op::ChainId   => { let v = s.ctx.chain_id;    s.push(v, raw)?; }
            Op::SelfAddr  => { let v = s.ctx.self_addr;   s.push(v, raw)?; }
            Op::GasLeft   => { let v = s.gas;             s.push(v, raw)?; }

            // ── stack ─────────────────────────────────────────────────────
            Op::Pop => { s.pop(raw)?; }

            // ── memory ────────────────────────────────────────────────────
            Op::MLoad => {
                let offset = s.pop(raw)? as usize;
                let val = s.mload(offset)?;
                s.push(val, raw)?;
            }
            Op::MStore => {
                let (offset, value) = (s.pop(raw)? as usize, s.pop(raw)?);
                s.mstore(offset, value)?;
            }
            Op::MStore8 => {
                let (offset, value) = (s.pop(raw)? as usize, s.pop(raw)?);
                s.mstore8(offset, value as u8)?;
            }
            Op::MSize => {
                let sz = s.memory.len() as u64;
                s.push(sz, raw)?;
            }

            // ── storage ───────────────────────────────────────────────────
            Op::SLoad => {
                let key = s.pop(raw)?;
                // warm slot discount: already charged 100 at base; cold adds 2000
                if !s.warm_slots.contains(&key) {
                    s.charge(2_000)?;
                    s.warm_slots.insert(key);
                }
                let val = s.storage.get(&key).copied().unwrap_or(0);
                s.push(val, raw)?;
            }
            Op::SStore => {
                if s.is_static {
                    return Err(VmError::WriteProtection);
                }
                let (key, val) = (s.pop(raw)?, s.pop(raw)?);
                // Simplified: always charge 2900 (EIP-2200 "SSTORE_RESET" cost)
                // Base 100 already charged; add 2800 more for non-zero writes
                if val != 0 {
                    s.charge(2_800)?;
                }
                s.warm_slots.insert(key);
                if val == 0 {
                    s.storage.remove(&key);
                } else {
                    s.storage.insert(key, val);
                }
            }

            // ── control ───────────────────────────────────────────────────
            Op::Jump => {
                let dest = s.pop(raw)?;
                let dest_usize = dest as usize;
                if !s.jumpdests.contains(&dest_usize) {
                    return Err(VmError::InvalidJump { dest });
                }
                s.pc = dest_usize + 1; // skip JumpDest opcode itself
            }
            Op::JumpI => {
                let (dest, cond) = (s.pop(raw)?, s.pop(raw)?);
                if cond != 0 {
                    let dest_usize = dest as usize;
                    if !s.jumpdests.contains(&dest_usize) {
                        return Err(VmError::InvalidJump { dest });
                    }
                    s.pc = dest_usize + 1;
                }
            }
            Op::JumpDest => { /* marker only, no-op in execution */ }
            Op::Pc => {
                // PC of this instruction (before increment)
                s.push((s.pc - 1) as u64, raw)?;
            }

            // ── push ─────────────────────────────────────────────────────
            Op::Push1 | Op::Push2 | Op::Push3 | Op::Push4 |
            Op::Push5 | Op::Push6 | Op::Push7 | Op::Push8 => {
                let n = op.immediate_bytes();
                if s.pc + n > bytecode.len() {
                    return Err(VmError::UnknownOpcode(raw));
                }
                let mut val: u64 = 0;
                for &byte in &bytecode[s.pc..s.pc + n] {
                    val = (val << 8) | byte as u64;
                }
                s.pc += n;
                s.push(val, raw)?;
            }

            // ── dup ──────────────────────────────────────────────────────
            Op::Dup1  | Op::Dup2  | Op::Dup3  | Op::Dup4  |
            Op::Dup5  | Op::Dup6  | Op::Dup7  | Op::Dup8  |
            Op::Dup9  | Op::Dup10 | Op::Dup11 | Op::Dup12 |
            Op::Dup13 | Op::Dup14 | Op::Dup15 | Op::Dup16 => {
                let depth = (raw - 0x7F) as usize; // Dup1 = 0x80 → depth 1
                let val = s.peek(depth - 1, raw)?;
                s.push(val, raw)?;
            }

            // ── swap ─────────────────────────────────────────────────────
            Op::Swap1  | Op::Swap2  | Op::Swap3  | Op::Swap4  |
            Op::Swap5  | Op::Swap6  | Op::Swap7  | Op::Swap8  |
            Op::Swap9  | Op::Swap10 | Op::Swap11 | Op::Swap12 |
            Op::Swap13 | Op::Swap14 | Op::Swap15 | Op::Swap16 => {
                let depth = (raw - 0x8F) as usize; // Swap1 = 0x90 → depth 1
                let len = s.stack.len();
                if depth >= len {
                    return Err(VmError::StackUnderflow { op: raw });
                }
                s.stack.swap(len - 1, len - 1 - depth);
            }

            // ── logging ───────────────────────────────────────────────────
            Op::Log0 | Op::Log1 | Op::Log2 | Op::Log3 | Op::Log4 => {
                // M-09: LOG is a state-observing write; forbidden in static context
                if s.is_static {
                    return Err(VmError::WriteProtection);
                }
                let topic_count = (raw - 0xA0) as usize;
                let (offset, len) = (s.pop(raw)? as usize, s.pop(raw)? as usize);
                // charge 8 per data byte
                s.charge(8 * len as u64)?;
                let mut topics = Vec::with_capacity(topic_count);
                for _ in 0..topic_count {
                    topics.push(s.pop(raw)?);
                }
                let data = s.mem_slice(offset, len)?;
                s.logs.push(LogEntry {
                    address: s.ctx.self_addr,
                    topics,
                    data,
                });
            }

            // ── system ────────────────────────────────────────────────────
            Op::Return => {
                let (offset, len) = (s.pop(raw)? as usize, s.pop(raw)? as usize);
                let data = s.mem_slice(offset, len)?;
                let gas_used = gas_limit - s.gas;
                let storage = s.storage;
                return Ok((ExecResult::Return { data, gas_used }, Some(storage)));
            }
            Op::Revert => {
                let (offset, len) = (s.pop(raw)? as usize, s.pop(raw)? as usize);
                let data = s.mem_slice(offset, len)?;
                let gas_used = gas_limit - s.gas;
                // Revert: return None so callers cannot accidentally apply storage changes.
                return Ok((ExecResult::Revert { data, gas_used }, None));
            }

            // CREATE / CALL stubs — not implemented in the interpreter layer;
            // require node-level contract registry to resolve addresses.
            Op::Create | Op::Call | Op::StaticCall => {
                // For now push 0 (failure / zero address) — full inter-contract
                // calls are implemented at the node layer in Phase 12B.
                let n_args = match op {
                    Op::Call | Op::StaticCall => 5,
                    Op::Create => 3,
                    _ => unreachable!(),
                };
                for _ in 0..n_args {
                    s.pop(raw)?;
                }
                s.push(0, raw)?;
            }
        }
    }
}
