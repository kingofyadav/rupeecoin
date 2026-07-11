/// RupeeCoin Virtual Machine — opcode definitions and base gas costs.
///
/// Stack-based, 64-bit word machine.  Instruction encoding mirrors EVM
/// conventions where possible so the Phase 12.5 EVM transpiler can do a
/// largely 1-to-1 mapping.
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Op {
    // ── 0x00–0x0D  arithmetic ─────────────────────────────────────────────
    Stop = 0x00,
    Add = 0x01,
    Mul = 0x02,
    Sub = 0x03,
    Div = 0x04,
    Sdiv = 0x05,
    Mod = 0x06,
    Smod = 0x07,
    Addmod = 0x08,
    Mulmod = 0x09,
    Exp = 0x0A,
    // Checked arithmetic — do NOT trap on overflow; push (result, overflow_flag)
    AddOv = 0x0B,
    SubOv = 0x0C,
    MulOv = 0x0D,

    // ── 0x10–0x1C  comparison & bitwise ──────────────────────────────────
    Lt = 0x10,
    Gt = 0x11,
    Slt = 0x12,
    Sgt = 0x13,
    Eq = 0x14,
    IsZero = 0x15,
    And = 0x16,
    Or = 0x17,
    Xor = 0x18,
    Not = 0x19,
    Shl = 0x1A,
    Shr = 0x1B,
    Sar = 0x1C,

    // ── 0x20  crypto ──────────────────────────────────────────────────────
    // BLAKE3(offset, length) → pushes low 64 bits of hash (deterministic)
    Blake3 = 0x20,

    // ── 0x30–0x36  execution context ──────────────────────────────────────
    Caller = 0x30,
    CallValue = 0x31,
    Height = 0x32,
    Timestamp = 0x33,
    ChainId = 0x34,
    SelfAddr = 0x35,
    GasLeft = 0x36,

    // ── 0x50–0x5A  stack / memory / storage / control ─────────────────────
    Pop = 0x50,
    MLoad = 0x51,
    MStore = 0x52,
    MStore8 = 0x53,
    MSize = 0x54,
    SLoad = 0x55,
    SStore = 0x56,
    Jump = 0x57,
    JumpI = 0x58,
    JumpDest = 0x59,
    Pc = 0x5A,

    // ── 0x60–0x67  PUSH1..PUSH8 ──────────────────────────────────────────
    Push1 = 0x60,
    Push2 = 0x61,
    Push3 = 0x62,
    Push4 = 0x63,
    Push5 = 0x64,
    Push6 = 0x65,
    Push7 = 0x66,
    Push8 = 0x67,

    // ── 0x80–0x8F  DUP1..DUP16 ───────────────────────────────────────────
    Dup1 = 0x80,
    Dup2 = 0x81,
    Dup3 = 0x82,
    Dup4 = 0x83,
    Dup5 = 0x84,
    Dup6 = 0x85,
    Dup7 = 0x86,
    Dup8 = 0x87,
    Dup9 = 0x88,
    Dup10 = 0x89,
    Dup11 = 0x8A,
    Dup12 = 0x8B,
    Dup13 = 0x8C,
    Dup14 = 0x8D,
    Dup15 = 0x8E,
    Dup16 = 0x8F,

    // ── 0x90–0x9F  SWAP1..SWAP16 ─────────────────────────────────────────
    Swap1 = 0x90,
    Swap2 = 0x91,
    Swap3 = 0x92,
    Swap4 = 0x93,
    Swap5 = 0x94,
    Swap6 = 0x95,
    Swap7 = 0x96,
    Swap8 = 0x97,
    Swap9 = 0x98,
    Swap10 = 0x99,
    Swap11 = 0x9A,
    Swap12 = 0x9B,
    Swap13 = 0x9C,
    Swap14 = 0x9D,
    Swap15 = 0x9E,
    Swap16 = 0x9F,

    // ── 0xA0–0xA4  logging ────────────────────────────────────────────────
    Log0 = 0xA0,
    Log1 = 0xA1,
    Log2 = 0xA2,
    Log3 = 0xA3,
    Log4 = 0xA4,

    // ── 0xF0–0xFD  system ─────────────────────────────────────────────────
    Create = 0xF0,
    Call = 0xF1,
    StaticCall = 0xF2,
    Return = 0xF3,
    Revert = 0xFD,
}

#[derive(Debug, Error)]
#[error("unknown opcode: 0x{0:02X}")]
pub struct UnknownOpcode(pub u8);

impl TryFrom<u8> for Op {
    type Error = UnknownOpcode;
    fn try_from(b: u8) -> Result<Self, Self::Error> {
        use Op::*;
        Ok(match b {
            0x00 => Stop,
            0x01 => Add,
            0x02 => Mul,
            0x03 => Sub,
            0x04 => Div,
            0x05 => Sdiv,
            0x06 => Mod,
            0x07 => Smod,
            0x08 => Addmod,
            0x09 => Mulmod,
            0x0A => Exp,
            0x0B => AddOv,
            0x0C => SubOv,
            0x0D => MulOv,
            0x10 => Lt,
            0x11 => Gt,
            0x12 => Slt,
            0x13 => Sgt,
            0x14 => Eq,
            0x15 => IsZero,
            0x16 => And,
            0x17 => Or,
            0x18 => Xor,
            0x19 => Not,
            0x1A => Shl,
            0x1B => Shr,
            0x1C => Sar,
            0x20 => Blake3,
            0x30 => Caller,
            0x31 => CallValue,
            0x32 => Height,
            0x33 => Timestamp,
            0x34 => ChainId,
            0x35 => SelfAddr,
            0x36 => GasLeft,
            0x50 => Pop,
            0x51 => MLoad,
            0x52 => MStore,
            0x53 => MStore8,
            0x54 => MSize,
            0x55 => SLoad,
            0x56 => SStore,
            0x57 => Jump,
            0x58 => JumpI,
            0x59 => JumpDest,
            0x5A => Pc,
            0x60 => Push1,
            0x61 => Push2,
            0x62 => Push3,
            0x63 => Push4,
            0x64 => Push5,
            0x65 => Push6,
            0x66 => Push7,
            0x67 => Push8,
            0x80 => Dup1,
            0x81 => Dup2,
            0x82 => Dup3,
            0x83 => Dup4,
            0x84 => Dup5,
            0x85 => Dup6,
            0x86 => Dup7,
            0x87 => Dup8,
            0x88 => Dup9,
            0x89 => Dup10,
            0x8A => Dup11,
            0x8B => Dup12,
            0x8C => Dup13,
            0x8D => Dup14,
            0x8E => Dup15,
            0x8F => Dup16,
            0x90 => Swap1,
            0x91 => Swap2,
            0x92 => Swap3,
            0x93 => Swap4,
            0x94 => Swap5,
            0x95 => Swap6,
            0x96 => Swap7,
            0x97 => Swap8,
            0x98 => Swap9,
            0x99 => Swap10,
            0x9A => Swap11,
            0x9B => Swap12,
            0x9C => Swap13,
            0x9D => Swap14,
            0x9E => Swap15,
            0x9F => Swap16,
            0xA0 => Log0,
            0xA1 => Log1,
            0xA2 => Log2,
            0xA3 => Log3,
            0xA4 => Log4,
            0xF0 => Create,
            0xF1 => Call,
            0xF2 => StaticCall,
            0xF3 => Return,
            0xFD => Revert,
            b => return Err(UnknownOpcode(b)),
        })
    }
}

impl Op {
    /// How many immediate bytes follow this opcode in the bytecode stream.
    pub fn immediate_bytes(self) -> usize {
        match self {
            Op::Push1 => 1,
            Op::Push2 => 2,
            Op::Push3 => 3,
            Op::Push4 => 4,
            Op::Push5 => 5,
            Op::Push6 => 6,
            Op::Push7 => 7,
            Op::Push8 => 8,
            _ => 0,
        }
    }

    /// Base gas cost (before memory-expansion and per-byte surcharges).
    pub fn base_gas(self) -> u64 {
        use Op::*;
        match self {
            Stop | Return | Revert => 0,
            Add | Sub | Lt | Gt | Slt | Sgt | Eq | IsZero | And | Or | Xor | Not | Shl | Shr
            | Sar | AddOv | SubOv | Pop | JumpDest | Pc => 3,
            Mul | Div | Sdiv | Mod | Smod | MulOv => 5,
            Addmod | Mulmod => 8,
            Exp => 10,
            Blake3 => 30,
            Push1 | Push2 | Push3 | Push4 | Push5 | Push6 | Push7 | Push8 => 3,
            Dup1 | Dup2 | Dup3 | Dup4 | Dup5 | Dup6 | Dup7 | Dup8 | Dup9 | Dup10 | Dup11
            | Dup12 | Dup13 | Dup14 | Dup15 | Dup16 => 3,
            Swap1 | Swap2 | Swap3 | Swap4 | Swap5 | Swap6 | Swap7 | Swap8 | Swap9 | Swap10
            | Swap11 | Swap12 | Swap13 | Swap14 | Swap15 | Swap16 => 3,
            Caller | CallValue | Height | Timestamp | ChainId | SelfAddr | GasLeft => 2,
            MLoad | MStore | MStore8 | MSize => 3,
            SLoad => 100,
            SStore => 100,
            Jump => 8,
            JumpI => 10,
            Log0 => 375,
            Log1 => 375 + 375,
            Log2 => 375 + 750,
            Log3 => 375 + 1125,
            Log4 => 375 + 1500,
            Create => 32_000,
            Call | StaticCall => 100,
        }
    }
}
