/// RPC-721: Non-fungible token standard (ERC-721 analogue).
///
/// Storage layout
/// ───────────────
///  slot 0                : total_minted (monotonically increasing token ID counter)
///  OWNER_SLOT            : contract deployer/owner address (set at init time)
///  owner_slot(token_id)  : owner address of token `token_id` (0 = unminted/burned)
///
/// ABI
/// ────
///  mint(to: u64)                      → token_id: u64
///  ownerOf(token_id: u64)             → owner: u64
///  transfer(to: u64, token_id: u64)   → 1
///  burn(token_id: u64)                → 1
///  totalMinted()                      → u64
use std::collections::HashMap;

use crate::assemble::Assembler;
use crate::opcode::Op;

/// Storage slot reserved for the contract deployer/owner address.
/// Uses u64::MAX to avoid collision with token-ID-derived slots.
pub const OWNER_SLOT: u64 = u64::MAX;

pub const SEL_MINT:         u64 = 0xaaaa_bbbb_1111_0001;
pub const SEL_OWNER_OF:     u64 = 0xaaaa_bbbb_1111_0002;
pub const SEL_TRANSFER:     u64 = 0xaaaa_bbbb_1111_0003;
pub const SEL_BURN:         u64 = 0xaaaa_bbbb_1111_0004;
pub const SEL_TOTAL_MINTED: u64 = 0xaaaa_bbbb_1111_0005;

pub fn rpc721_bytecode() -> Vec<u8> {
    let mut a = Assembler::new();

    // ── dispatch ──────────────────────────────────────────────────────────
    a.push8(0).op(Op::MLoad); // selector
    a.op(Op::Dup1).push8(SEL_MINT).op(Op::Eq).push_label("mint").op(Op::JumpI);
    a.op(Op::Dup1).push8(SEL_OWNER_OF).op(Op::Eq).push_label("ownerOf").op(Op::JumpI);
    a.op(Op::Dup1).push8(SEL_TRANSFER).op(Op::Eq).push_label("transfer721").op(Op::JumpI);
    a.op(Op::Dup1).push8(SEL_BURN).op(Op::Eq).push_label("burn").op(Op::JumpI);
    a.op(Op::Dup1).push8(SEL_TOTAL_MINTED).op(Op::Eq).push_label("totalMinted").op(Op::JumpI);
    a.push8(0).push8(0).op(Op::Revert);

    // ── mint(to) → token_id ───────────────────────────────────────────────
    // H-24: only the contract owner (stored at OWNER_SLOT) may mint.
    a.label("mint");
    a.op(Op::Pop);                                   // drop selector; stack=[]
    // auth check: caller must equal storage[OWNER_SLOT]
    a.push8(OWNER_SLOT).op(Op::SLoad);              // [contract_owner]
    a.op(Op::Caller).op(Op::Eq).op(Op::IsZero);    // 1 if caller != owner
    a.push_label("mint_revert").op(Op::JumpI);       // revert if unauthorized
    a.push8(8).op(Op::MLoad);   // to
    // new_id = storage[0] + 1
    a.push8(0).op(Op::SLoad).push8(1).op(Op::Add); // new_id
    a.op(Op::Dup1).push8(0).op(Op::SStore);         // storage[0] = new_id
    // owner_slot(new_id) = 0x0003.. + new_id * MULT
    a.op(Op::Dup1).push8(0x517c_c1b7_2722_0a95).op(Op::Mul).push8(0x0003_0000_0000_0000).op(Op::Add);
    // stack: [to, new_id, owner_slot]
    a.op(Op::Dup3).op(Op::Swap1).op(Op::SStore); // storage[owner_slot] = to
    // emit Transfer(0 → to, token_id)
    a.op(Op::Dup1);   // token_id for data
    a.push8(0).op(Op::MStore);
    a.op(Op::Dup2);   // to
    a.push8(0);       // from = 0 (mint)
    a.push8(0x4d696E74); // "Mint"
    a.push8(8).push8(0).op(Op::Log3);
    // return new_id
    a.op(Op::Dup1).push8(0).op(Op::MStore);
    a.push8(8).push8(0).op(Op::Return);

    // ── ownerOf(token_id) ─────────────────────────────────────────────────
    a.label("ownerOf");
    a.op(Op::Pop);
    a.push8(8).op(Op::MLoad); // token_id
    a.push8(0x517c_c1b7_2722_0a95).op(Op::Mul).push8(0x0003_0000_0000_0000).op(Op::Add);
    a.op(Op::SLoad);
    a.push8(0).op(Op::MStore);
    a.push8(8).push8(0).op(Op::Return);

    // ── transfer(to, token_id) ────────────────────────────────────────────
    a.label("transfer721");
    a.op(Op::Pop);
    a.push8(8).op(Op::MLoad);   // to
    a.push8(16).op(Op::MLoad);  // token_id
    // owner_slot
    a.op(Op::Dup1).push8(0x517c_c1b7_2722_0a95).op(Op::Mul).push8(0x0003_0000_0000_0000).op(Op::Add);
    a.op(Op::Dup1).op(Op::SLoad); // current_owner
    // check caller == current_owner
    a.op(Op::Caller).op(Op::Eq).op(Op::IsZero).push_label("transfer721_revert").op(Op::JumpI);
    // set new owner
    a.op(Op::Dup3).op(Op::Swap1).op(Op::SStore); // storage[slot] = to
    a.push8(1).push8(0).op(Op::MStore);
    a.push8(8).push8(0).op(Op::Return);

    a.label("mint_revert");
    a.push8(0).push8(0).op(Op::Revert);

    a.label("transfer721_revert");
    a.push8(0).push8(0).op(Op::Revert);

    // ── burn(token_id) ────────────────────────────────────────────────────
    a.label("burn");
    a.op(Op::Pop);
    a.push8(8).op(Op::MLoad); // token_id
    a.op(Op::Dup1).push8(0x517c_c1b7_2722_0a95).op(Op::Mul).push8(0x0003_0000_0000_0000).op(Op::Add);
    a.op(Op::Dup1).op(Op::SLoad); // owner
    a.op(Op::Caller).op(Op::Eq).op(Op::IsZero).push_label("burn_revert").op(Op::JumpI);
    // set owner to 0
    a.push8(0).op(Op::Swap1).op(Op::SStore);
    a.push8(1).push8(0).op(Op::MStore);
    a.push8(8).push8(0).op(Op::Return);

    a.label("burn_revert");
    a.push8(0).push8(0).op(Op::Revert);

    // ── totalMinted() ─────────────────────────────────────────────────────
    a.label("totalMinted");
    a.op(Op::Pop);
    a.push8(0).op(Op::SLoad);
    a.push8(0).op(Op::MStore);
    a.push8(8).push8(0).op(Op::Return);

    a.build().expect("all labels defined in rpc721_bytecode")
}

/// Populate initial storage for a freshly deployed RPC-721 contract.
/// Must be called before any `mint` — sets the contract owner slot.
pub fn rpc721_init_storage(owner: u64) -> HashMap<u64, u64> {
    let mut s = HashMap::new();
    s.insert(0, 0u64);          // total_minted = 0
    s.insert(OWNER_SLOT, owner);
    s
}
