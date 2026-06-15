/// RPC-1155: Multi-token standard (ERC-1155 analogue).
///
/// Storage layout
/// ───────────────
///  multi_balance_slot(account, token_id) : balance of `account` for `token_id`
///
/// ABI (8-byte selectors, 8-byte aligned args)
/// ────────────────────────────────────────────
///  mint(to: u64, id: u64, amount: u64)              → 1
///  burn(from: u64, id: u64, amount: u64)             → 1
///  balanceOf(account: u64, id: u64)                  → u64
///  transfer(from: u64, to: u64, id: u64, amt: u64)   → 1
use crate::assemble::Assembler;
use crate::opcode::Op;

pub const SEL_MINT:       u64 = 0xcccc_dddd_2222_0001;
pub const SEL_BURN:       u64 = 0xcccc_dddd_2222_0002;
pub const SEL_BALANCE_OF: u64 = 0xcccc_dddd_2222_0003;
pub const SEL_TRANSFER:   u64 = 0xcccc_dddd_2222_0004;

const MULT_A: u64 = 0x517c_c1b7_2722_0a95;
const MULT_B: u64 = 0x9e37_79b9_7f4a_7c15;
const BASE:   u64 = 0x0004_0000_0000_0000;

/// Emit RVM bytecode that computes multi_balance_slot(account, id).
///
/// Stack before: [..., account, id]   (id is on top)
/// Stack after:  [..., slot]
///
/// slot = BASE + (account*MULT_A ^ id*MULT_B)
fn emit_slot(a: &mut Assembler) {
    // Stack: [..., account, id]
    a.op(Op::Swap1);               // [..., id, account]
    a.push8(MULT_A).op(Op::Mul);   // [..., id, account*MULT_A]
    a.op(Op::Swap1);               // [..., account*MULT_A, id]
    a.push8(MULT_B).op(Op::Mul);   // [..., account*MULT_A, id*MULT_B]
    a.op(Op::Xor);                  // [..., mixed]
    a.push8(BASE).op(Op::Add);     // [..., slot]
}

pub fn rpc1155_bytecode() -> Vec<u8> {
    let mut a = Assembler::new();

    // ── dispatch ──────────────────────────────────────────────────────────
    a.push8(0).op(Op::MLoad);  // selector from calldata[0]
    a.op(Op::Dup1).push8(SEL_MINT).op(Op::Eq).push_label("mint1155").op(Op::JumpI);
    a.op(Op::Dup1).push8(SEL_BURN).op(Op::Eq).push_label("burn1155").op(Op::JumpI);
    a.op(Op::Dup1).push8(SEL_BALANCE_OF).op(Op::Eq).push_label("balanceOf1155").op(Op::JumpI);
    a.op(Op::Dup1).push8(SEL_TRANSFER).op(Op::Eq).push_label("transfer1155").op(Op::JumpI);
    a.push8(0).push8(0).op(Op::Revert);

    // ── mint(to, id, amount) ──────────────────────────────────────────────
    // Stack after arg load: [to, id, amount]
    a.label("mint1155");
    a.op(Op::Pop); // drop selector
    a.push8(8).op(Op::MLoad);   // [to]
    a.push8(16).op(Op::MLoad);  // [to, id]
    a.push8(24).op(Op::MLoad);  // [to, id, amount]

    // Compute slot(to, id).
    // Stack: [to, id, amount]; depths: 0=amount, 1=id, 2=to
    a.op(Op::Dup3);  // Dup3=depth2=to  → [to, id, amount, to]
    a.op(Op::Dup3);  // Dup3=depth2=id  → [to, id, amount, to, id]
    emit_slot(&mut a);  // [to, id, amount, slot]

    // current = storage[slot]; new_balance = current + amount.
    // Stack: [to, id, amount, slot]; depths: 0=slot, 1=amount, 2=id, 3=to
    a.op(Op::Dup1).op(Op::SLoad);  // [to, id, amount, slot, current]
    a.op(Op::Dup3);  // Dup3=depth2=amount → [to, id, amount, slot, current, amount]
    a.op(Op::Add);   // [to, id, amount, slot, new_balance]
    a.op(Op::Swap1).op(Op::SStore); // storage[slot]=new_balance → [to, id, amount]

    // Emit log: Log4(offset=0, len=8, t0=sig, t1=from=0, t2=to, t3=id)
    // Data at mem[0] = amount. Stack: [to, id, amount]; depths: 0=amount, 1=id, 2=to
    a.push8(0).op(Op::MStore);   // mem[0]=amount → [to, id]
    // Push topics in reverse pop order: last-popped first.
    // LOG4 pops: offset, len, t0, t1, t2, t3 (t0 first out of topics)
    // Topics: t0=sig, t1=0(from), t2=to, t3=id
    // So build stack (bottom→top): id, to, 0, sig, 8, 0
    // Stack: [to, id]
    a.op(Op::Dup1);              // [to, id, id]  — Dup1=depth0=id
    a.op(Op::Dup3);              // [to, id, id, to]  — Dup3=depth2=to
    a.push8(0);                   // from=0 → [to, id, id, to, 0]
    a.push8(0x4d696e74_31313535u64); // sig → [to, id, id, to, 0, sig]
    a.push8(8).push8(0).op(Op::Log4);
    // Log4 pops 6: offset=0, len=8, t0=sig, t1=0, t2=to, t3=id → [to, id]

    // Return 1.
    a.push8(1).push8(0).op(Op::MStore); // mem[0]=1 → [to, id]
    a.push8(8).push8(0).op(Op::Return);

    // ── burn(from, id, amount) ────────────────────────────────────────────
    // Stack after arg load: [from, id, amount]
    a.label("burn1155");
    a.op(Op::Pop);
    a.push8(8).op(Op::MLoad);   // [from]
    a.push8(16).op(Op::MLoad);  // [from, id]
    a.push8(24).op(Op::MLoad);  // [from, id, amount]

    // Compute from_slot.
    // Stack: [from, id, amount]; depths: 0=amount, 1=id, 2=from
    a.op(Op::Dup3);  // depth2=from → [from, id, amount, from]
    a.op(Op::Dup3);  // depth2=id   → [from, id, amount, from, id]
    emit_slot(&mut a);  // [from, id, amount, slot]

    // Check: amount > current → revert.
    // Stack: [from, id, amount, slot]; depths: 0=slot, 1=amount
    a.op(Op::Dup1).op(Op::SLoad);  // [from, id, amount, slot, current]
    a.op(Op::Dup3);  // Dup3=depth2=amount → [from, id, amount, slot, current, amount]
    a.op(Op::Swap1); // [from, id, amount, slot, amount, current]
    a.op(Op::Lt);    // LT(a=current, b=amount) = (current < amount) = 1 if insufficient
    a.push_label("burn1155_revert").op(Op::JumpI);
    // (not taken): [from, id, amount, slot]

    // Deduct.
    a.op(Op::Dup1).op(Op::SLoad);  // [from, id, amount, slot, current]
    a.op(Op::Dup3);  // depth2=amount → [from, id, amount, slot, current, amount]
    a.op(Op::Swap1); // [from, id, amount, slot, amount, current]
    a.op(Op::Sub);   // current − amount → [from, id, amount, slot, new_balance]
    a.op(Op::Swap1).op(Op::SStore); // storage[slot]=new_balance → [from, id, amount]

    a.push8(1).push8(0).op(Op::MStore);
    a.push8(8).push8(0).op(Op::Return);

    a.label("burn1155_revert");
    a.push8(0).push8(0).op(Op::Revert);

    // ── balanceOf(account, id) ────────────────────────────────────────────
    a.label("balanceOf1155");
    a.op(Op::Pop);
    a.push8(8).op(Op::MLoad);   // [account]
    a.push8(16).op(Op::MLoad);  // [account, id]
    emit_slot(&mut a);           // [slot]
    a.op(Op::SLoad);
    a.push8(0).op(Op::MStore);
    a.push8(8).push8(0).op(Op::Return);

    // ── transfer(from, to, id, amount) ────────────────────────────────────
    // Stack after arg load: [from, to, id, amount]
    a.label("transfer1155");
    a.op(Op::Pop);
    a.push8(8).op(Op::MLoad);   // [from]
    a.push8(16).op(Op::MLoad);  // [from, to]
    a.push8(24).op(Op::MLoad);  // [from, to, id]
    a.push8(32).op(Op::MLoad);  // [from, to, id, amount]

    // Compute from_slot.
    // Stack: [from, to, id, amount]; depths: 0=amount, 1=id, 2=to, 3=from
    a.op(Op::Dup4);  // depth3=from → [from, to, id, amount, from]
    // After Dup4: depths 0=from, 1=amount, 2=id, 3=to, 4=from(orig)
    a.op(Op::Dup3);  // depth2=id   → [from, to, id, amount, from, id]
    emit_slot(&mut a);  // [from, to, id, amount, from_slot]

    // Check from_balance >= amount.
    // Stack: [from, to, id, amount, from_slot]; depths: 0=from_slot, 1=amount
    a.op(Op::Dup1).op(Op::SLoad);  // [from, to, id, amount, from_slot, from_bal]
    a.op(Op::Dup3);  // Dup3=depth2=amount → [from, to, id, amount, from_slot, from_bal, amount]
    a.op(Op::Swap1); // [from, to, id, amount, from_slot, amount, from_bal]
    a.op(Op::Lt);    // LT(a=from_bal, b=amount) = (from_bal < amount) = 1 if insufficient
    a.push_label("transfer1155_revert").op(Op::JumpI);
    // (not taken): [from, to, id, amount, from_slot]

    // Deduct from sender.
    a.op(Op::Dup1).op(Op::SLoad);  // [from, to, id, amount, from_slot, from_bal]
    a.op(Op::Dup3);  // amount → [from, to, id, amount, from_slot, from_bal, amount]
    a.op(Op::Swap1);  // [from, to, id, amount, from_slot, amount, from_bal]
    a.op(Op::Sub);    // from_bal − amount → [from, to, id, amount, from_slot, new_from_bal]
    a.op(Op::Swap1).op(Op::SStore); // [from, to, id, amount]
    // Wait: after Sub: [from, to, id, amount, from_slot, new_from_bal]
    // Swap1: [from, to, id, amount, new_from_bal, from_slot]
    // SStore: key=from_slot, val=new_from_bal → [from, to, id, amount]

    // Compute to_slot and add.
    // Stack: [from, to, id, amount]; depths: 0=amount, 1=id, 2=to, 3=from
    a.op(Op::Dup3);  // depth2=to  → [from, to, id, amount, to]
    a.op(Op::Dup3);  // depth2=id  → [from, to, id, amount, to, id]
    emit_slot(&mut a);  // [from, to, id, amount, to_slot]
    a.op(Op::Dup1).op(Op::SLoad);  // [from, to, id, amount, to_slot, to_bal]
    a.op(Op::Dup3);  // depth2=amount → [from, to, id, amount, to_slot, to_bal, amount]
    a.op(Op::Add);   // [from, to, id, amount, to_slot, new_to_bal]
    a.op(Op::Swap1).op(Op::SStore); // [from, to, id, amount]

    a.push8(1).push8(0).op(Op::MStore);
    a.push8(8).push8(0).op(Op::Return);

    a.label("transfer1155_revert");
    a.push8(0).push8(0).op(Op::Revert);

    a.build()
}
