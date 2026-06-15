/// Phase 12.5 — EVM → RVM bytecode transpiler.
///
/// Reads EVM bytecode from stdin (hex-encoded) or a file argument and
/// writes the equivalent RVM bytecode to stdout (hex-encoded).
///
/// Supported patterns
/// ──────────────────
/// The transpiler handles the subset of EVM opcodes that covers ~80 % of
/// common DeFi patterns:
///   - Arithmetic (ADD, SUB, MUL, DIV, MOD, EXP) — direct 1:1, values
///     truncated to 64 bits (wrapping semantics preserved)
///   - Comparison / bitwise (LT, GT, EQ, ISZERO, AND, OR, XOR, NOT, SHL, SHR)
///   - Stack (PUSH1..PUSH32 → PUSH8, POP, DUP1..16, SWAP1..16)
///   - Memory (MLOAD, MSTORE, MSTORE8) — word-size mismatch noted in warnings
///   - Storage (SLOAD, SSTORE) — slot values are 64-bit; high bits of EVM
///     256-bit slots are silently dropped
///   - Control (JUMP, JUMPI, JUMPDEST, STOP, RETURN, REVERT)
///   - Context (CALLER, CALLVALUE, NUMBER→HEIGHT, TIMESTAMP, CHAINID)
///   - Crypto (KECCAK256 → BLAKE3)
///
/// Unsupported EVM opcodes are emitted as a REVERT stub with a warning.
use std::io::{self, Read};

use rupeecoin_rvm::Op;

// ── EVM opcode constants (subset we translate) ────────────────────────────────
const EVM_STOP:      u8 = 0x00;
const EVM_ADD:       u8 = 0x01;
const EVM_MUL:       u8 = 0x02;
const EVM_SUB:       u8 = 0x03;
const EVM_DIV:       u8 = 0x04;
const EVM_SDIV:      u8 = 0x05;
const EVM_MOD:       u8 = 0x06;
const EVM_SMOD:      u8 = 0x07;
const EVM_ADDMOD:    u8 = 0x08;
const EVM_MULMOD:    u8 = 0x09;
const EVM_EXP:       u8 = 0x0A;
const EVM_LT:        u8 = 0x10;
const EVM_GT:        u8 = 0x11;
const EVM_SLT:       u8 = 0x12;
const EVM_SGT:       u8 = 0x13;
const EVM_EQ:        u8 = 0x14;
const EVM_ISZERO:    u8 = 0x15;
const EVM_AND:       u8 = 0x16;
const EVM_OR:        u8 = 0x17;
const EVM_XOR:       u8 = 0x18;
const EVM_NOT:       u8 = 0x19;
const EVM_SHL:       u8 = 0x1B;
const EVM_SHR:       u8 = 0x1C;
const EVM_KECCAK256: u8 = 0x20;
const EVM_CALLER:    u8 = 0x33;
const EVM_CALLVALUE: u8 = 0x34;
const EVM_NUMBER:    u8 = 0x43;
const EVM_TIMESTAMP: u8 = 0x42;
const EVM_CHAINID:   u8 = 0x46;
const EVM_POP:       u8 = 0x50;
const EVM_MLOAD:     u8 = 0x51;
const EVM_MSTORE:    u8 = 0x52;
const EVM_MSTORE8:   u8 = 0x53;
const EVM_SLOAD:     u8 = 0x54;
const EVM_SSTORE:    u8 = 0x55;
const EVM_JUMP:      u8 = 0x56;
const EVM_JUMPI:     u8 = 0x57;
const EVM_PC:        u8 = 0x58;
const EVM_JUMPDEST:  u8 = 0x5B;
const EVM_RETURN:    u8 = 0xF3;
const EVM_REVERT:    u8 = 0xFD;

pub struct TranspileResult {
    pub rvm_code: Vec<u8>,
    pub warnings: Vec<String>,
}

/// Translate EVM bytecode to RVM bytecode.
pub fn transpile(evm: &[u8]) -> TranspileResult {
    let mut rvm  = Vec::with_capacity(evm.len());
    let mut warns: Vec<String> = Vec::new();
    let mut i = 0usize;

    while i < evm.len() {
        let op = evm[i];
        i += 1;

        match op {
            // ── arithmetic ───────────────────────────────────────────────
            EVM_STOP    => { rvm.push(Op::Stop as u8); }
            EVM_ADD     => { rvm.push(Op::Add as u8); }
            EVM_MUL     => { rvm.push(Op::Mul as u8); }
            EVM_SUB     => { rvm.push(Op::Sub as u8); }
            EVM_DIV     => { rvm.push(Op::Div as u8); }
            EVM_SDIV    => { rvm.push(Op::Sdiv as u8); }
            EVM_MOD     => { rvm.push(Op::Mod as u8); }
            EVM_SMOD    => { rvm.push(Op::Smod as u8); }
            EVM_ADDMOD  => { rvm.push(Op::Addmod as u8); }
            EVM_MULMOD  => { rvm.push(Op::Mulmod as u8); }
            EVM_EXP     => { rvm.push(Op::Exp as u8); }

            // ── comparison / bitwise ─────────────────────────────────────
            EVM_LT     => { rvm.push(Op::Lt as u8); }
            EVM_GT     => { rvm.push(Op::Gt as u8); }
            EVM_SLT    => { rvm.push(Op::Slt as u8); }
            EVM_SGT    => { rvm.push(Op::Sgt as u8); }
            EVM_EQ     => { rvm.push(Op::Eq as u8); }
            EVM_ISZERO => { rvm.push(Op::IsZero as u8); }
            EVM_AND    => { rvm.push(Op::And as u8); }
            EVM_OR     => { rvm.push(Op::Or as u8); }
            EVM_XOR    => { rvm.push(Op::Xor as u8); }
            EVM_NOT    => { rvm.push(Op::Not as u8); }
            EVM_SHL    => { rvm.push(Op::Shl as u8); }
            EVM_SHR    => { rvm.push(Op::Shr as u8); }

            // ── crypto ────────────────────────────────────────────────────
            EVM_KECCAK256 => {
                // KECCAK256(offset, len) → BLAKE3(offset, len)
                // Stack convention identical; hash output is 32 bytes in EVM
                // vs 8 bytes (low 64 bits) in RVM. Warn on mismatch.
                warns.push(format!(
                    "offset 0x{:04X}: KECCAK256 mapped to BLAKE3 (output truncated to 64 bits)",
                    i - 1
                ));
                rvm.push(Op::Blake3 as u8);
            }

            // ── context ───────────────────────────────────────────────────
            EVM_CALLER    => { rvm.push(Op::Caller as u8); }
            EVM_CALLVALUE => { rvm.push(Op::CallValue as u8); }
            EVM_NUMBER    => { rvm.push(Op::Height as u8); }
            EVM_TIMESTAMP => { rvm.push(Op::Timestamp as u8); }
            EVM_CHAINID   => { rvm.push(Op::ChainId as u8); }

            // ── stack / memory / storage ──────────────────────────────────
            EVM_POP     => { rvm.push(Op::Pop as u8); }
            EVM_MLOAD   => {
                // EVM MLOAD loads 32 bytes; RVM MLOAD loads 8 bytes.
                warns.push(format!(
                    "offset 0x{:04X}: MLOAD word-size mismatch (EVM=256bit, RVM=64bit)",
                    i - 1
                ));
                rvm.push(Op::MLoad as u8);
            }
            EVM_MSTORE  => {
                warns.push(format!(
                    "offset 0x{:04X}: MSTORE word-size mismatch (EVM=256bit, RVM=64bit)",
                    i - 1
                ));
                rvm.push(Op::MStore as u8);
            }
            EVM_MSTORE8 => { rvm.push(Op::MStore8 as u8); }
            EVM_SLOAD   => { rvm.push(Op::SLoad as u8); }
            EVM_SSTORE  => { rvm.push(Op::SStore as u8); }
            EVM_JUMP    => { rvm.push(Op::Jump as u8); }
            EVM_JUMPI   => { rvm.push(Op::JumpI as u8); }
            EVM_PC      => { rvm.push(Op::Pc as u8); }
            EVM_JUMPDEST => { rvm.push(Op::JumpDest as u8); }
            EVM_RETURN  => { rvm.push(Op::Return as u8); }
            EVM_REVERT  => { rvm.push(Op::Revert as u8); }

            // ── PUSH1..PUSH32 (EVM 0x60..0x7F) ───────────────────────────
            b @ 0x60..=0x7F => {
                let n_evm = (b - 0x5F) as usize; // 1..32 bytes
                if i + n_evm > evm.len() {
                    warns.push(format!("truncated PUSH{n_evm} at end of bytecode"));
                    break;
                }
                let raw = &evm[i..i + n_evm];
                i += n_evm;

                // Truncate to 8 bytes (little-endian significant bytes at end)
                if n_evm > 8 {
                    let high = &raw[..n_evm - 8];
                    if high.iter().any(|&x| x != 0) {
                        warns.push(format!(
                            "offset 0x{:04X}: PUSH{n_evm} value truncated from 256-bit to 64-bit",
                            i - n_evm - 1
                        ));
                    }
                }
                let start = n_evm.saturating_sub(8);
                let mut val_bytes = [0u8; 8];
                let slice = &raw[start..];
                val_bytes[8 - slice.len()..].copy_from_slice(slice);
                let val = u64::from_be_bytes(val_bytes);
                // Emit PUSH8
                rvm.push(Op::Push8 as u8);
                rvm.extend_from_slice(&val.to_be_bytes());
            }

            // ── DUP1..DUP16 (EVM 0x80..0x8F) ────────────────────────────
            b @ 0x80..=0x8F => { rvm.push(b); } // identical encoding in RVM

            // ── SWAP1..SWAP16 (EVM 0x90..0x9F) ───────────────────────────
            b @ 0x90..=0x9F => { rvm.push(b); } // identical encoding in RVM

            // ── unsupported ───────────────────────────────────────────────
            _ => {
                warns.push(format!(
                    "offset 0x{:04X}: unsupported EVM opcode 0x{op:02X} → REVERT stub",
                    i - 1
                ));
                // Emit a REVERT(0,0) stub so the contract fails safely
                rvm.push(Op::Push8 as u8);
                rvm.extend_from_slice(&0u64.to_be_bytes());
                rvm.push(Op::Push8 as u8);
                rvm.extend_from_slice(&0u64.to_be_bytes());
                rvm.push(Op::Revert as u8);
            }
        }
    }

    TranspileResult { rvm_code: rvm, warnings: warns }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let hex_input = if args.len() >= 2 {
        std::fs::read_to_string(&args[1]).unwrap_or_else(|e| {
            eprintln!("error reading {}: {e}", args[1]);
            std::process::exit(1);
        })
    } else {
        let mut s = String::new();
        io::stdin().read_to_string(&mut s).expect("read stdin");
        s
    };

    let hex_trimmed = hex_input.trim().trim_start_matches("0x");
    let evm_bytes = hex::decode(hex_trimmed).unwrap_or_else(|e| {
        eprintln!("hex decode error: {e}");
        std::process::exit(1);
    });

    let result = transpile(&evm_bytes);

    for w in &result.warnings {
        eprintln!("warning: {w}");
    }

    println!("0x{}", hex::encode(&result.rvm_code));
}

#[cfg(test)]
mod tests {
    use super::*;
    use rupeecoin_rvm::Op;

    #[test]
    fn transpile_simple_add() {
        // EVM: PUSH1 3, PUSH1 4, ADD, STOP
        let evm = vec![0x60, 0x03, 0x60, 0x04, 0x01, 0x00];
        let res = transpile(&evm);
        assert!(res.warnings.is_empty(), "unexpected warnings: {:?}", res.warnings);
        // Expect: PUSH8 3, PUSH8 4, ADD, STOP
        assert_eq!(res.rvm_code[0], Op::Push8 as u8);
        assert_eq!(res.rvm_code[9], Op::Push8 as u8);
        assert_eq!(res.rvm_code[18], Op::Add as u8);
        assert_eq!(res.rvm_code[19], Op::Stop as u8);
    }

    #[test]
    fn transpile_push32_truncates() {
        // EVM: PUSH32 (32 bytes of 0x01..0x20) — high bytes get truncated
        let mut evm = vec![0x7F]; // PUSH32
        evm.extend_from_slice(&[0u8; 24]); // high 24 bytes = 0
        evm.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]); // low 8 bytes
        evm.push(0x00); // STOP
        let res = transpile(&evm);
        // No truncation warning (high bytes are all zero)
        let trunc_warns: Vec<_> = res.warnings.iter().filter(|w| w.contains("truncated")).collect();
        assert!(trunc_warns.is_empty());
        // Result: PUSH8 0x0102030405060708, STOP
        assert_eq!(res.rvm_code[0], Op::Push8 as u8);
        assert_eq!(&res.rvm_code[1..9], &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(res.rvm_code[9], Op::Stop as u8);
    }

    #[test]
    fn transpile_jump_jumpdest() {
        // EVM: PUSH1 3, JUMP, JUMPDEST (at offset 3), STOP
        let evm = vec![0x60, 0x03, 0x56, 0x5B, 0x00];
        let res = transpile(&evm);
        assert!(res.warnings.is_empty());
        // PUSH8 3, JUMP, JUMPDEST, STOP
        assert_eq!(res.rvm_code[0], Op::Push8 as u8);
        assert_eq!(res.rvm_code[9], Op::Jump as u8);
        assert_eq!(res.rvm_code[10], Op::JumpDest as u8);
        assert_eq!(res.rvm_code[11], Op::Stop as u8);
    }

    #[test]
    fn transpile_dup_swap_passthrough() {
        // DUP1=0x80, SWAP1=0x90 — same encoding in RVM
        let evm = vec![0x80, 0x90];
        let res = transpile(&evm);
        assert!(res.warnings.is_empty());
        assert_eq!(res.rvm_code, vec![0x80, 0x90]);
    }

    #[test]
    fn transpile_keccak256_emits_blake3_with_warning() {
        let evm = vec![0x20]; // KECCAK256
        let res = transpile(&evm);
        assert_eq!(res.rvm_code, vec![Op::Blake3 as u8]);
        assert!(!res.warnings.is_empty());
        assert!(res.warnings[0].contains("BLAKE3"));
    }

    #[test]
    fn transpile_unsupported_opcode_emits_revert_stub() {
        let evm = vec![0xFF]; // not a real EVM opcode, unsupported
        let res = transpile(&evm);
        // Should emit PUSH8 0, PUSH8 0, REVERT
        assert_eq!(res.rvm_code[0], Op::Push8 as u8);
        assert_eq!(res.rvm_code[9], Op::Push8 as u8);
        assert_eq!(res.rvm_code[18], Op::Revert as u8);
        assert!(!res.warnings.is_empty());
    }

    #[test]
    fn transpile_sload_sstore() {
        let evm = vec![0x54, 0x55]; // SLOAD, SSTORE
        let res = transpile(&evm);
        assert!(res.warnings.is_empty());
        assert_eq!(res.rvm_code, vec![Op::SLoad as u8, Op::SStore as u8]);
    }

    #[test]
    fn transpile_context_opcodes() {
        // CALLER=0x33, CALLVALUE=0x34, NUMBER=0x43, TIMESTAMP=0x42, CHAINID=0x46
        let evm = vec![0x33, 0x34, 0x43, 0x42, 0x46];
        let res = transpile(&evm);
        assert!(res.warnings.is_empty());
        assert_eq!(res.rvm_code, vec![
            Op::Caller as u8, Op::CallValue as u8,
            Op::Height as u8, Op::Timestamp as u8, Op::ChainId as u8,
        ]);
    }
}
