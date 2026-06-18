/// RPC-20: Fungible token standard (ERC-20 analogue).
///
/// Storage layout
/// ───────────────
///  slot 0               : total_supply
///  slot 1               : decimals (8)
///  balance_slot(addr)   : balance of `addr`
///  allowance_slot(o,s)  : allowance of `spender` on behalf of `owner`
///
/// ABI (8-byte selectors, 8-byte aligned args)
/// ────────────────────────────────────────────
///  totalSupply()                        → u64
///  balanceOf(addr: u64)                 → u64
///  transfer(to: u64, amount: u64)       → 1 (success) / revert
///  approve(spender: u64, amount: u64)   → 1
///  allowance(owner: u64, spender: u64)  → u64
///  transferFrom(from,to,amount: u64)    → 1
use crate::assemble::Assembler;
use crate::opcode::Op;
use crate::token::balance_slot;

// Pre-computed selectors
pub const SEL_TOTAL_SUPPLY:   u64 = 0x0c27_7e07_7174_e25fu64;
pub const SEL_BALANCE_OF:     u64 = 0x0c6f_9e39_e50b_a0d0u64;
pub const SEL_TRANSFER:       u64 = 0x1d41_c80b_6b37_ede3u64;
pub const SEL_APPROVE:        u64 = 0x7c8f_ea70_a2d5_a28fu64;
pub const SEL_ALLOWANCE:      u64 = 0x6c8f_ea70_a2d5_a28fu64;
pub const SEL_TRANSFER_FROM:  u64 = 0x2d41_c80b_6b37_ede3u64;

/// Build RPC-20 runtime bytecode for a token with the given initial supply
/// minted to `owner`.  The constructor must be run separately (or the caller
/// can pre-populate storage before executing the runtime code).
pub fn rpc20_bytecode() -> Vec<u8> {
    let mut a = Assembler::new();

    // ── dispatch ─────────────────────────────────────────────────────────
    // Call data offset 0 holds the 8-byte selector.
    // Stack after reading: [selector]
    a.push8(0).op(Op::MLoad); // load selector from calldata[0]

    a.op(Op::Dup1).push8(SEL_TOTAL_SUPPLY).op(Op::Eq).push_label("totalSupply").op(Op::JumpI);
    a.op(Op::Dup1).push8(SEL_BALANCE_OF).op(Op::Eq).push_label("balanceOf").op(Op::JumpI);
    a.op(Op::Dup1).push8(SEL_TRANSFER).op(Op::Eq).push_label("transfer").op(Op::JumpI);
    a.op(Op::Dup1).push8(SEL_APPROVE).op(Op::Eq).push_label("approve").op(Op::JumpI);
    a.op(Op::Dup1).push8(SEL_ALLOWANCE).op(Op::Eq).push_label("allowance").op(Op::JumpI);
    a.op(Op::Dup1).push8(SEL_TRANSFER_FROM).op(Op::Eq).push_label("transferFrom").op(Op::JumpI);
    // Unknown selector → revert
    a.push8(0).push8(0).op(Op::Revert);

    // ── totalSupply() ────────────────────────────────────────────────────
    a.label("totalSupply");
    a.op(Op::Pop); // drop selector
    a.push8(0).op(Op::SLoad); // storage[0] = total_supply
    a.push8(0).op(Op::MStore); // mem[0] = total_supply
    a.push8(8).push8(0).op(Op::Return);

    // ── balanceOf(addr) ──────────────────────────────────────────────────
    a.label("balanceOf");
    a.op(Op::Pop); // drop selector
    a.push8(8).op(Op::MLoad); // arg0 = addr from calldata[8]
    // compute balance slot: push8(base) + addr*mult
    a.push8(0x517c_c1b7_2722_0a95).op(Op::Mul)
     .push8(0x0001_0000_0000_0000).op(Op::Add);
    a.op(Op::SLoad);
    a.push8(0).op(Op::MStore);
    a.push8(8).push8(0).op(Op::Return);

    // ── transfer(to, amount) ─────────────────────────────────────────────
    // Stack conventions below shown as (bottom … top).
    a.label("transfer");
    a.op(Op::Pop); // drop selector → []
    a.push8(8).op(Op::MLoad);   // [to]
    a.push8(16).op(Op::MLoad);  // [to, amount]

    // Compute caller's balance slot.
    a.op(Op::Caller);
    a.push8(0x517c_c1b7_2722_0a95).op(Op::Mul);
    a.push8(0x0001_0000_0000_0000).op(Op::Add); // [to, amount, caller_slot]

    // Load caller balance.
    a.op(Op::Dup1).op(Op::SLoad); // [to, amount, caller_slot, cb]

    // Check amount > cb → revert.
    // Stack depths from top: 0=cb, 1=caller_slot, 2=amount, 3=to
    a.op(Op::Dup1);  // Dup1=cb  → [to, amount, caller_slot, cb, cb]
    a.op(Op::Dup4);  // Dup4=depth3=amount → [to, amount, caller_slot, cb, cb, amount]
    a.op(Op::Gt);    // GT(a=amount, b=cb) = amount>cb → 1 if insufficient
    a.push_label("transfer_revert").op(Op::JumpI);
    // (not taken): [to, amount, caller_slot, cb]

    // Deduct: new_cb = cb − amount
    a.op(Op::Dup3);  // Dup3=depth2=amount → [to, amount, caller_slot, cb, amount]
    a.op(Op::Swap1); // [to, amount, caller_slot, amount, cb]
    a.op(Op::Sub);   // cb − amount → [to, amount, caller_slot, new_cb]
    a.op(Op::Dup2);  // Dup2=depth1=caller_slot → [to, amount, caller_slot, new_cb, caller_slot]
    a.op(Op::SStore); // storage[caller_slot]=new_cb → [to, amount, caller_slot]
    a.op(Op::Pop);   // [to, amount]

    // Add amount to recipient.
    // Stack: [to, amount]; depths: 0=amount, 1=to
    a.op(Op::Dup2);  // Dup2=depth1=to → [to, amount, to]
    a.push8(0x517c_c1b7_2722_0a95).op(Op::Mul);
    a.push8(0x0001_0000_0000_0000).op(Op::Add); // [to, amount, to_slot]
    a.op(Op::Dup1).op(Op::SLoad);               // [to, amount, to_slot, to_bal]
    a.op(Op::Dup3);  // Dup3=depth2=amount → [to, amount, to_slot, to_bal, amount]
    a.op(Op::Add);   // [to, amount, to_slot, new_to_bal]
    a.op(Op::Swap1).op(Op::SStore); // storage[to_slot]=new_to_bal → [to, amount]

    // Emit Transfer log (data=amount at mem[0]).
    a.push8(0).op(Op::MStore); // mem[0]=amount → [to]
    a.op(Op::Dup1);             // [to, to]
    a.op(Op::Caller);           // [to, to, caller]
    a.push8(0x5472616e73666572u64); // "Transfer"
    a.push8(8).push8(0).op(Op::Log3); // pops: offset=0, len=8, t0=sig, t1=caller, t2=to → [to]

    // Return 1.
    a.push8(1).push8(0).op(Op::MStore); // mem[0]=1 → [to]
    a.op(Op::Pop);               // []
    a.push8(8).push8(0).op(Op::Return);

    a.label("transfer_revert");
    a.push8(0).push8(0).op(Op::Revert);

    // ── approve(spender, amount) ──────────────────────────────────────────
    a.label("approve");
    a.op(Op::Pop);
    a.push8(8).op(Op::MLoad);  // spender
    a.push8(16).op(Op::MLoad); // amount
    // allowance_slot(caller, spender)
    a.op(Op::Caller);
    a.op(Op::Dup3); // spender
    // compute: 0x0002.. + (caller*A ^ spender*B)
    a.push8(0x9e37_79b9_7f4a_7c15).op(Op::Mul); // spender*B
    a.op(Op::Swap1);
    a.push8(0x517c_c1b7_2722_0a95).op(Op::Mul); // caller*A
    a.op(Op::Xor);
    a.push8(0x0002_0000_0000_0000).op(Op::Add); // slot
    // stack: [spender, amount, slot]
    a.op(Op::Dup2).op(Op::Swap1).op(Op::SStore); // storage[slot] = amount
    // emit Approval log
    a.op(Op::Dup1); // amount
    a.push8(0).op(Op::MStore);
    a.op(Op::Dup2);  // spender
    a.op(Op::Caller);
    a.push8(0x417070726F76616C); // "Approval"
    a.push8(8).push8(0).op(Op::Log3);
    a.push8(1).push8(0).op(Op::MStore);
    a.push8(8).push8(0).op(Op::Return);

    // ── allowance(owner, spender) ─────────────────────────────────────────
    a.label("allowance");
    a.op(Op::Pop);
    a.push8(8).op(Op::MLoad);  // owner
    a.push8(16).op(Op::MLoad); // spender
    // compute allowance slot same as approve
    a.op(Op::Dup1).push8(0x9e37_79b9_7f4a_7c15).op(Op::Mul); // spender*B
    a.op(Op::Dup3).push8(0x517c_c1b7_2722_0a95).op(Op::Mul); // owner*A
    a.op(Op::Xor).push8(0x0002_0000_0000_0000).op(Op::Add);
    a.op(Op::SLoad);
    a.push8(0).op(Op::MStore);
    a.push8(8).push8(0).op(Op::Return);

    // ── transferFrom(from, to, amount) ───────────────────────────────────
    // H-18, H-19, H-53: correct allowance slot (from*A ^ caller*B), balance
    // check before allowance deduction, strict > check, overflow guard on credit.
    a.label("transferFrom");
    a.op(Op::Pop);
    a.push8(8).op(Op::MLoad);   // from
    a.push8(16).op(Op::MLoad);  // to
    a.push8(24).op(Op::MLoad);  // amount
    // stack: [from, to, amount]

    // compute allowance_slot(from, caller) = 0x0002.. + from*A ^ caller*B
    a.op(Op::Dup3);                              // from  (Dup3=peek2=bottom of stack)
    a.push8(0x517c_c1b7_2722_0a95).op(Op::Mul); // from*A
    a.op(Op::Caller);
    a.push8(0x9e37_79b9_7f4a_7c15).op(Op::Mul); // caller*B
    a.op(Op::Xor);
    a.push8(0x0002_0000_0000_0000).op(Op::Add); // all_slot
    // stack: [from, to, amount, all_slot]

    // load allowance; check amount > allowance → revert
    a.op(Op::Dup1).op(Op::SLoad);              // [from,to,amount,all_slot,allowance]
    a.op(Op::Dup3);                             // amount  (peek2=stack[2]=amount ✓)
    a.op(Op::Gt);                               // amount > allowance?
    a.push_label("transferFrom_revert").op(Op::JumpI);
    // not taken: [from, to, amount, all_slot]

    // compute from_slot = 0x0001.. + from*A; from is at index 0 (peek3 of 4-item stack)
    a.op(Op::Dup4);                              // from  (peek3=stack[0]=from ✓)
    a.push8(0x517c_c1b7_2722_0a95).op(Op::Mul);
    a.push8(0x0001_0000_0000_0000).op(Op::Add); // from_slot
    // stack: [from, to, amount, all_slot, from_slot]

    // load from_bal; check amount > from_bal → revert (H-19: balance before allowance)
    a.op(Op::Dup1).op(Op::SLoad);              // [from,to,amount,all_slot,from_slot,from_bal]
    a.op(Op::Dup4);                             // amount  (peek3=stack[2]=amount ✓)
    a.op(Op::Gt);                               // amount > from_bal?
    a.push_label("transferFrom_revert").op(Op::JumpI);
    // not taken: [from, to, amount, all_slot, from_slot]

    // deduct from-balance
    a.op(Op::Dup1).op(Op::SLoad);              // [from,to,amount,all_slot,from_slot,from_bal]
    a.op(Op::Dup4);                             // amount  (peek3=stack[2]=amount ✓)
    a.op(Op::Sub);                              // from_bal - amount
    a.op(Op::Swap1).op(Op::SStore);            // storage[from_slot]=from_bal-amount
    // stack: [from, to, amount, all_slot]

    // deduct allowance
    a.op(Op::Dup1).op(Op::SLoad);              // [from,to,amount,all_slot,allowance]
    a.op(Op::Dup3);                             // amount  (peek2=stack[2]=amount ✓)
    a.op(Op::Sub);                              // allowance - amount
    a.op(Op::Swap1).op(Op::SStore);            // storage[all_slot]=allowance-amount
    // stack: [from, to, amount]

    // add to-balance with overflow check
    a.op(Op::Dup2);                              // to  (peek1=stack[1]=to ✓)
    a.push8(0x517c_c1b7_2722_0a95).op(Op::Mul);
    a.push8(0x0001_0000_0000_0000).op(Op::Add); // to_slot
    // stack: [from, to, amount, to_slot]
    a.op(Op::Dup1).op(Op::SLoad);              // [from,to,amount,to_slot,to_bal]
    a.op(Op::Dup3);                             // amount  (peek2=stack[2]=amount ✓)
    a.op(Op::AddOv);                            // [from,to,amount,to_slot,result,overflow]
    a.push_label("transferFrom_revert").op(Op::JumpI); // revert on overflow
    // not taken: [from,to,amount,to_slot,result]
    a.op(Op::Swap1).op(Op::SStore);            // storage[to_slot]=result

    // return 1
    a.push8(1).push8(0).op(Op::MStore);
    a.push8(8).push8(0).op(Op::Return);

    a.label("transferFrom_revert");
    a.push8(0).push8(0).op(Op::Revert);

    a.build().expect("all labels defined in rpc20_bytecode")
}

/// Populate initial storage for a freshly deployed RPC-20 token.
pub fn rpc20_init_storage(
    total_supply: u64,
    owner: u64,
) -> std::collections::HashMap<u64, u64> {
    let mut s = std::collections::HashMap::new();
    s.insert(0, total_supply); // slot 0 = total_supply
    s.insert(1, 8);            // slot 1 = decimals
    s.insert(balance_slot(owner), total_supply);
    s
}
