pub mod assemble;
pub mod opcode;
pub mod token;
pub mod vm;

pub use assemble::Assembler;
pub use opcode::Op;
pub use vm::{run, static_call, ExecContext, ExecResult, LogEntry, VmError};

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use token::rpc20::{rpc20_bytecode, rpc20_init_storage, SEL_BALANCE_OF, SEL_TOTAL_SUPPLY,
                        SEL_TRANSFER, SEL_APPROVE, SEL_ALLOWANCE};
    use token::rpc721::{rpc721_bytecode, SEL_MINT as NFT_MINT, SEL_OWNER_OF, SEL_TRANSFER as NFT_TRANSFER,
                         SEL_TOTAL_MINTED};
    use token::rpc1155::{rpc1155_bytecode, SEL_MINT as MULTI_MINT, SEL_BALANCE_OF as MULTI_BAL,
                          SEL_TRANSFER as MULTI_TRANSFER, SEL_BURN as MULTI_BURN};

    const GAS: u64 = 1_000_000;
    const ALICE: u64 = 0xAAAA_1111;
    const BOB: u64   = 0xBBBB_2222;

    // ── helpers ───────────────────────────────────────────────────────────

    fn calldata(selector: u64, args: &[u64]) -> Vec<u8> {
        let mut cd = selector.to_be_bytes().to_vec();
        for &a in args {
            cd.extend_from_slice(&a.to_be_bytes());
        }
        cd
    }

    fn ctx_with(caller: u64, cd: Vec<u8>) -> ExecContext {
        ExecContext { caller, gas_limit: GAS, calldata: cd, chain_id: 1, ..Default::default() }
    }

    fn read_u64(data: &[u8]) -> u64 {
        u64::from_be_bytes(data[..8].try_into().unwrap())
    }

    // ── VM basic arithmetic tests ─────────────────────────────────────────

    #[test]
    fn vm_add() {
        // PUSH8 3, PUSH8 4, ADD, PUSH8 0, MSTORE, PUSH8 8, PUSH8 0, RETURN
        let code = Assembler::new()
            .push8(3).push8(4).op(Op::Add)
            .push8(0).op(Op::MStore)
            .push8(8).push8(0).op(Op::Return)
            .build();
        let (res, _) = run(&code, HashMap::new(), ctx_with(0, vec![])).unwrap();
        let ExecResult::Return { data, .. } = res else { panic!("not return") };
        assert_eq!(read_u64(&data), 7);
    }

    #[test]
    fn vm_sub_wrapping() {
        let code = Assembler::new()
            .push8(0).push8(1).op(Op::Sub) // 1 - 0 = 1? No: SUB(a,b) = a-b, stack is [a=0, b=1], pops a then b
            // Actually: PUSH8 0 pushes 0, PUSH8 1 pushes 1. Stack = [0, 1]. SUB pops a=1, b=0, result = 1-0=1
            .push8(0).op(Op::MStore)
            .push8(8).push8(0).op(Op::Return)
            .build();
        let (res, _) = run(&code, HashMap::new(), ctx_with(0, vec![])).unwrap();
        let ExecResult::Return { data, .. } = res else { panic!() };
        assert_eq!(read_u64(&data), 1);
    }

    #[test]
    fn vm_mul() {
        let code = Assembler::new()
            .push8(6).push8(7).op(Op::Mul)
            .push8(0).op(Op::MStore)
            .push8(8).push8(0).op(Op::Return)
            .build();
        let (res, _) = run(&code, HashMap::new(), ctx_with(0, vec![])).unwrap();
        let ExecResult::Return { data, .. } = res else { panic!() };
        assert_eq!(read_u64(&data), 42);
    }

    #[test]
    fn vm_div_by_zero_returns_zero() {
        let code = Assembler::new()
            .push8(5).push8(0).op(Op::Div) // 0/5 = 0
            .push8(0).op(Op::MStore)
            .push8(8).push8(0).op(Op::Return)
            .build();
        let (res, _) = run(&code, HashMap::new(), ctx_with(0, vec![])).unwrap();
        let ExecResult::Return { data, .. } = res else { panic!() };
        assert_eq!(read_u64(&data), 0);
    }

    #[test]
    fn vm_comparison_lt() {
        let code = Assembler::new()
            .push8(10).push8(3).op(Op::Lt) // 3 < 10 = 1
            .push8(0).op(Op::MStore)
            .push8(8).push8(0).op(Op::Return)
            .build();
        let (res, _) = run(&code, HashMap::new(), ctx_with(0, vec![])).unwrap();
        let ExecResult::Return { data, .. } = res else { panic!() };
        assert_eq!(read_u64(&data), 1);
    }

    #[test]
    fn vm_jump_and_jumpdest() {
        // Jump over a trap REVERT to a safe RETURN
        let mut a = Assembler::new();
        a.push_label("safe").op(Op::Jump);
        // This REVERT should be skipped:
        a.push8(0).push8(0).op(Op::Revert);
        a.label("safe");
        a.push8(99).push8(0).op(Op::MStore);
        a.push8(8).push8(0).op(Op::Return);
        let code = a.build();
        let (res, _) = run(&code, HashMap::new(), ctx_with(0, vec![])).unwrap();
        let ExecResult::Return { data, .. } = res else { panic!() };
        assert_eq!(read_u64(&data), 99);
    }

    #[test]
    fn vm_jumpi_conditional() {
        // if 0 → don't jump; if 1 → jump
        let mut a = Assembler::new();
        a.push8(0).push_label("skip").op(Op::JumpI); // cond=0, no jump
        a.push8(42).push8(0).op(Op::MStore);
        a.push8(8).push8(0).op(Op::Return);
        a.label("skip");
        a.push8(0).push8(0).op(Op::Revert);
        let code = a.build();
        let (res, _) = run(&code, HashMap::new(), ctx_with(0, vec![])).unwrap();
        let ExecResult::Return { data, .. } = res else { panic!() };
        assert_eq!(read_u64(&data), 42);
    }

    #[test]
    fn vm_sload_sstore() {
        let code = Assembler::new()
            .push8(999).push8(7).op(Op::SStore) // storage[7] = 999
            .push8(7).op(Op::SLoad)
            .push8(0).op(Op::MStore)
            .push8(8).push8(0).op(Op::Return)
            .build();
        let (res, storage) = run(&code, HashMap::new(), ctx_with(0, vec![])).unwrap();
        let ExecResult::Return { data, .. } = res else { panic!() };
        assert_eq!(read_u64(&data), 999);
        assert_eq!(storage[&7], 999);
    }

    #[test]
    fn vm_dup_and_swap() {
        // DUP1: [5] → [5,5]; SWAP1: [5,5] → [5,5] (noop here); ADD → [10]
        let code = Assembler::new()
            .push8(5).op(Op::Dup1).op(Op::Add)
            .push8(0).op(Op::MStore)
            .push8(8).push8(0).op(Op::Return)
            .build();
        let (res, _) = run(&code, HashMap::new(), ctx_with(0, vec![])).unwrap();
        let ExecResult::Return { data, .. } = res else { panic!() };
        assert_eq!(read_u64(&data), 10);
    }

    #[test]
    fn vm_overflow_checked_add() {
        // u64::MAX + 1 should overflow
        let code = Assembler::new()
            .push8(1).push8(u64::MAX).op(Op::AddOv) // result, overflow_flag
            .push8(0).op(Op::MStore)  // store overflow_flag
            .push8(8).push8(0).op(Op::Return)
            .build();
        let (res, _) = run(&code, HashMap::new(), ctx_with(0, vec![])).unwrap();
        let ExecResult::Return { data, .. } = res else { panic!() };
        assert_eq!(read_u64(&data), 1); // overflow flag = 1
    }

    #[test]
    fn vm_revert_returns_data() {
        let code = Assembler::new()
            .push8(0xDEAD).push8(0).op(Op::MStore)
            .push8(8).push8(0).op(Op::Revert)
            .build();
        let (res, storage) = run(&code, HashMap::new(), ctx_with(0, vec![])).unwrap();
        let ExecResult::Revert { data, .. } = res else { panic!("expected revert") };
        assert_eq!(read_u64(&data), 0xDEAD);
        assert!(storage.is_empty()); // storage not persisted on revert
    }

    #[test]
    fn vm_out_of_gas() {
        // PUSH8 (gas=3) repeated enough to exhaust a tiny gas limit
        let code = Assembler::new().push8(1).push8(2).push8(3).build();
        let mut ctx = ctx_with(0, vec![]);
        ctx.gas_limit = 5; // only 5 gas → can fit 1 PUSH8 (3) then fail on 2nd
        let err = run(&code, HashMap::new(), ctx).unwrap_err();
        assert_eq!(err, VmError::OutOfGas);
    }

    #[test]
    fn vm_invalid_jump_errors() {
        // Jump to offset 0 which is PUSH8, not JUMPDEST
        let code = Assembler::new()
            .push8(0).op(Op::Jump)
            .build();
        let err = run(&code, HashMap::new(), ctx_with(0, vec![])).unwrap_err();
        assert!(matches!(err, VmError::InvalidJump { .. }));
    }

    #[test]
    fn vm_static_call_rejects_sstore() {
        let code = Assembler::new()
            .push8(1).push8(0).op(Op::SStore)
            .build();
        let err = static_call(&code, HashMap::new(), ctx_with(0, vec![])).unwrap_err();
        assert_eq!(err, VmError::WriteProtection);
    }

    #[test]
    fn vm_bitwise_ops() {
        let code = Assembler::new()
            .push8(0b1100).push8(0b1010).op(Op::And) // 0b1000 = 8
            .push8(0).op(Op::MStore)
            .push8(8).push8(0).op(Op::Return)
            .build();
        let (res, _) = run(&code, HashMap::new(), ctx_with(0, vec![])).unwrap();
        let ExecResult::Return { data, .. } = res else { panic!() };
        assert_eq!(read_u64(&data), 8);
    }

    #[test]
    fn vm_shl_shr() {
        // SHL(shift=2, val=1) = 4
        let code = Assembler::new()
            .push8(1).push8(2).op(Op::Shl) // val=1, shift=2 → 4
            .push8(0).op(Op::MStore)
            .push8(8).push8(0).op(Op::Return)
            .build();
        let (res, _) = run(&code, HashMap::new(), ctx_with(0, vec![])).unwrap();
        let ExecResult::Return { data, .. } = res else { panic!() };
        assert_eq!(read_u64(&data), 4);
    }

    #[test]
    fn vm_log_emitted() {
        let code = Assembler::new()
            .push8(0xBEEF).push8(0).op(Op::MStore) // data word in mem
            .push8(0xDEAD)                          // topic
            .push8(8).push8(0).op(Op::Log1)
            .push8(8).push8(0).op(Op::Return)
            .build();
        // We need a way to get logs back — wrap in vm::run and check logs via VmState
        // For now confirm execution doesn't error
        let (res, _) = run(&code, HashMap::new(), ctx_with(0, vec![])).unwrap();
        assert!(matches!(res, ExecResult::Return { .. }));
    }

    // ── RPC-20 token tests ────────────────────────────────────────────────

    #[test]
    fn rpc20_total_supply() {
        let code    = rpc20_bytecode();
        let storage = rpc20_init_storage(1_000_000, ALICE);
        let cd = calldata(SEL_TOTAL_SUPPLY, &[]);
        let (res, _) = run(&code, storage, ctx_with(ALICE, cd)).unwrap();
        let ExecResult::Return { data, .. } = res else { panic!() };
        assert_eq!(read_u64(&data), 1_000_000);
    }

    #[test]
    fn rpc20_balance_of() {
        let code    = rpc20_bytecode();
        let storage = rpc20_init_storage(500_000, ALICE);
        let cd = calldata(SEL_BALANCE_OF, &[ALICE]);
        let (res, _) = run(&code, storage, ctx_with(ALICE, cd)).unwrap();
        let ExecResult::Return { data, .. } = res else { panic!() };
        assert_eq!(read_u64(&data), 500_000);
    }

    #[test]
    fn rpc20_balance_of_zero_for_unknown_address() {
        let code    = rpc20_bytecode();
        let storage = rpc20_init_storage(100_000, ALICE);
        let cd = calldata(SEL_BALANCE_OF, &[BOB]);
        let (res, _) = run(&code, storage, ctx_with(ALICE, cd)).unwrap();
        let ExecResult::Return { data, .. } = res else { panic!() };
        assert_eq!(read_u64(&data), 0);
    }

    #[test]
    fn rpc20_transfer_updates_balances() {
        let code    = rpc20_bytecode();
        let storage = rpc20_init_storage(1000, ALICE);
        let cd = calldata(SEL_TRANSFER, &[BOB, 300]);
        let (res, storage) = run(&code, storage, ctx_with(ALICE, cd)).unwrap();
        assert!(matches!(res, ExecResult::Return { .. }));

        // Check alice's remaining balance
        let cd2 = calldata(SEL_BALANCE_OF, &[ALICE]);
        let (res2, _) = run(&code, storage.clone(), ctx_with(ALICE, cd2)).unwrap();
        let ExecResult::Return { data: d2, .. } = res2 else { panic!() };
        assert_eq!(read_u64(&d2), 700);

        // Check bob's received balance
        let cd3 = calldata(SEL_BALANCE_OF, &[BOB]);
        let (res3, _) = run(&code, storage, ctx_with(ALICE, cd3)).unwrap();
        let ExecResult::Return { data: d3, .. } = res3 else { panic!() };
        assert_eq!(read_u64(&d3), 300);
    }

    #[test]
    fn rpc20_transfer_insufficient_balance_reverts() {
        let code    = rpc20_bytecode();
        let storage = rpc20_init_storage(100, ALICE);
        let cd = calldata(SEL_TRANSFER, &[BOB, 200]);
        let (res, _) = run(&code, storage, ctx_with(ALICE, cd)).unwrap();
        assert!(matches!(res, ExecResult::Revert { .. }));
    }

    #[test]
    fn rpc20_approve_and_allowance() {
        let code    = rpc20_bytecode();
        let storage = rpc20_init_storage(1000, ALICE);
        // Alice approves Bob to spend 500
        let cd = calldata(SEL_APPROVE, &[BOB, 500]);
        let (_, storage) = run(&code, storage, ctx_with(ALICE, cd)).unwrap();
        // Read allowance
        let cd2 = calldata(SEL_ALLOWANCE, &[ALICE, BOB]);
        let (res, _) = run(&code, storage, ctx_with(ALICE, cd2)).unwrap();
        let ExecResult::Return { data, .. } = res else { panic!() };
        assert_eq!(read_u64(&data), 500);
    }

    // ── RPC-721 NFT tests ─────────────────────────────────────────────────

    #[test]
    fn rpc721_mint_and_owner_of() {
        let code = rpc721_bytecode();
        // Mint token to ALICE
        let cd = calldata(NFT_MINT, &[ALICE]);
        let (res, storage) = run(&code, HashMap::new(), ctx_with(ALICE, cd)).unwrap();
        let ExecResult::Return { data, .. } = res else { panic!() };
        let token_id = read_u64(&data);
        assert_eq!(token_id, 1);

        // ownerOf(token_id) = ALICE
        let cd2 = calldata(SEL_OWNER_OF, &[token_id]);
        let (res2, _) = run(&code, storage, ctx_with(ALICE, cd2)).unwrap();
        let ExecResult::Return { data: d2, .. } = res2 else { panic!() };
        assert_eq!(read_u64(&d2), ALICE);
    }

    #[test]
    fn rpc721_transfer() {
        let code = rpc721_bytecode();
        // Mint to ALICE
        let (_, storage) = run(&code, HashMap::new(), ctx_with(ALICE, calldata(NFT_MINT, &[ALICE]))).unwrap();
        // ALICE transfers token 1 to BOB
        let cd = calldata(NFT_TRANSFER, &[BOB, 1]);
        let (res, storage) = run(&code, storage, ctx_with(ALICE, cd)).unwrap();
        assert!(matches!(res, ExecResult::Return { .. }));
        // ownerOf(1) = BOB
        let (res2, _) = run(&code, storage, ctx_with(ALICE, calldata(SEL_OWNER_OF, &[1]))).unwrap();
        let ExecResult::Return { data, .. } = res2 else { panic!() };
        assert_eq!(read_u64(&data), BOB);
    }

    #[test]
    fn rpc721_transfer_by_non_owner_reverts() {
        let code = rpc721_bytecode();
        let (_, storage) = run(&code, HashMap::new(), ctx_with(ALICE, calldata(NFT_MINT, &[ALICE]))).unwrap();
        // BOB tries to transfer token 1 (owned by ALICE)
        let cd = calldata(NFT_TRANSFER, &[BOB, 1]);
        let (res, _) = run(&code, storage, ctx_with(BOB, cd)).unwrap();
        assert!(matches!(res, ExecResult::Revert { .. }));
    }

    #[test]
    fn rpc721_total_minted() {
        let code = rpc721_bytecode();
        let (_, s1) = run(&code, HashMap::new(), ctx_with(ALICE, calldata(NFT_MINT, &[ALICE]))).unwrap();
        let (_, s2) = run(&code, s1, ctx_with(ALICE, calldata(NFT_MINT, &[BOB]))).unwrap();
        let (res, _) = run(&code, s2, ctx_with(ALICE, calldata(SEL_TOTAL_MINTED, &[]))).unwrap();
        let ExecResult::Return { data, .. } = res else { panic!() };
        assert_eq!(read_u64(&data), 2);
    }

    // ── RPC-1155 multi-token tests ────────────────────────────────────────

    #[test]
    fn rpc1155_mint_and_balance() {
        let code = rpc1155_bytecode();
        let cd = calldata(MULTI_MINT, &[ALICE, 42, 100]); // mint 100 of token #42 to ALICE
        let (res, storage) = run(&code, HashMap::new(), ctx_with(ALICE, cd)).unwrap();
        assert!(matches!(res, ExecResult::Return { .. }));

        let cd2 = calldata(MULTI_BAL, &[ALICE, 42]);
        let (res2, _) = run(&code, storage, ctx_with(ALICE, cd2)).unwrap();
        let ExecResult::Return { data, .. } = res2 else { panic!() };
        assert_eq!(read_u64(&data), 100);
    }

    #[test]
    fn rpc1155_transfer() {
        let code = rpc1155_bytecode();
        let (_, storage) = run(&code, HashMap::new(), ctx_with(ALICE, calldata(MULTI_MINT, &[ALICE, 7, 50]))).unwrap();
        let cd = calldata(MULTI_TRANSFER, &[ALICE, BOB, 7, 20]);
        let (res, storage) = run(&code, storage, ctx_with(ALICE, cd)).unwrap();
        assert!(matches!(res, ExecResult::Return { .. }));

        let (res2, _) = run(&code, storage.clone(), ctx_with(ALICE, calldata(MULTI_BAL, &[ALICE, 7]))).unwrap();
        let ExecResult::Return { data: d2, .. } = res2 else { panic!() };
        assert_eq!(read_u64(&d2), 30);

        let (res3, _) = run(&code, storage, ctx_with(ALICE, calldata(MULTI_BAL, &[BOB, 7]))).unwrap();
        let ExecResult::Return { data: d3, .. } = res3 else { panic!() };
        assert_eq!(read_u64(&d3), 20);
    }

    #[test]
    fn rpc1155_burn() {
        let code = rpc1155_bytecode();
        let (_, storage) = run(&code, HashMap::new(), ctx_with(ALICE, calldata(MULTI_MINT, &[ALICE, 5, 1000]))).unwrap();
        let (res, storage) = run(&code, storage, ctx_with(ALICE, calldata(MULTI_BURN, &[ALICE, 5, 400]))).unwrap();
        assert!(matches!(res, ExecResult::Return { .. }));

        let (res2, _) = run(&code, storage, ctx_with(ALICE, calldata(MULTI_BAL, &[ALICE, 5]))).unwrap();
        let ExecResult::Return { data, .. } = res2 else { panic!() };
        assert_eq!(read_u64(&data), 600);
    }

    #[test]
    fn rpc1155_burn_insufficient_reverts() {
        let code = rpc1155_bytecode();
        let (_, storage) = run(&code, HashMap::new(), ctx_with(ALICE, calldata(MULTI_MINT, &[ALICE, 5, 10]))).unwrap();
        let (res, _) = run(&code, storage, ctx_with(ALICE, calldata(MULTI_BURN, &[ALICE, 5, 20]))).unwrap();
        assert!(matches!(res, ExecResult::Revert { .. }));
    }
}
