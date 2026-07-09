//! Integration tests: full chain construction across multiple crates.

use rupeecoin_blockchain::{
    build_block, build_coinbase, build_reward_claim, genesis_block, Chain, ANCHOR_DEPTH,
};
use rupeecoin_consensus::{CheckpointManager, CHECKPOINT_INTERVAL};
use rupeecoin_crypto::{Hash, KeyPair};
use rupeecoin_economics::{block_reward, next_base_fee, GAS_TARGET};
use rupeecoin_mempool::Mempool;
use rupeecoin_privacy::{CommitmentTree, PrivacyKeyBundle};
use rupeecoin_staking::ValidatorRegistry;
use rupeecoin_types::MICRO_RPC_PER_COIN;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_keypair() -> KeyPair {
    KeyPair::generate()
}

fn make_registry_with_validator(kp: &KeyPair, stake: u64) -> ValidatorRegistry {
    let mut reg = ValidatorRegistry::new();
    reg.register(kp.public_key.clone(), stake, 0).expect("registration should succeed");
    reg
}

// ── Chain construction ────────────────────────────────────────────────────────

#[test]
fn genesis_block_is_valid() {
    let genesis = genesis_block();
    assert_eq!(genesis.header.height, 0);
    assert_eq!(genesis.header.prev_hash, Hash::zero());
    assert!(!genesis.transactions.is_empty(), "genesis must have coinbase");
    assert!(genesis.transactions[0].is_coinbase());
}

#[test]
fn chain_starts_at_height_zero() {
    let chain = Chain::new(genesis_block());
    assert_eq!(chain.height(), 0);
    assert_eq!(chain.blocks.len(), 1);
}

#[test]
fn build_and_add_block_increments_height() {
    let kp = make_keypair();
    let mut registry = make_registry_with_validator(&kp, 10_000 * MICRO_RPC_PER_COIN);
    let mut chain = Chain::new(genesis_block());

    let block = build_block(&chain, &kp, &registry, vec![], vec![], vec![]);
    chain.add_block(block, &mut registry).expect("first block should be valid");
    assert_eq!(chain.height(), 1);
}

#[test]
fn multiple_blocks_advance_chain() {
    let kp = make_keypair();
    let mut registry = make_registry_with_validator(&kp, 10_000 * MICRO_RPC_PER_COIN);
    let mut chain = Chain::new(genesis_block());

    for expected_height in 1u64..=5 {
        let block = build_block(&chain, &kp, &registry, vec![], vec![], vec![]);
        chain.add_block(block, &mut registry).expect("block should be valid");
        assert_eq!(chain.height(), expected_height);
    }
}

#[test]
fn block_at_wrong_height_is_rejected() {
    let kp = make_keypair();
    let mut registry = make_registry_with_validator(&kp, 10_000 * MICRO_RPC_PER_COIN);
    let mut chain = Chain::new(genesis_block());

    let mut block = build_block(&chain, &kp, &registry, vec![], vec![], vec![]);
    // Tamper: set an impossible height.
    block.header.height = 999;
    assert!(
        chain.add_block(block, &mut registry).is_err(),
        "block with wrong height must be rejected"
    );
}

#[test]
fn block_with_wrong_prev_hash_is_rejected() {
    let kp = make_keypair();
    let mut registry = make_registry_with_validator(&kp, 10_000 * MICRO_RPC_PER_COIN);
    let mut chain = Chain::new(genesis_block());

    let mut block = build_block(&chain, &kp, &registry, vec![], vec![], vec![]);
    block.header.prev_hash = Hash::from_bytes([0xFFu8; 32]);
    assert!(
        chain.add_block(block, &mut registry).is_err(),
        "block with wrong prev_hash must be rejected"
    );
}

#[test]
fn state_root_is_committed_in_header() {
    let kp = make_keypair();
    let mut registry = make_registry_with_validator(&kp, 10_000 * MICRO_RPC_PER_COIN);
    let mut chain = Chain::new(genesis_block());

    let block = build_block(&chain, &kp, &registry, vec![], vec![], vec![]);
    let claimed = block.header.state_root;
    chain.add_block(block, &mut registry).unwrap();

    // State root = BLAKE3(utxo_root || commitment_tree_root) — verifies both
    // transparent and shielded pool state are committed in the block header.
    let utxo_root = chain.utxo_set.state_root().expect("UTXO serialization failed");
    let privacy_root = chain.commitment_tree.root();
    let combined = rupeecoin_crypto::hash_data(
        &[utxo_root.as_bytes().as_slice(), privacy_root.as_bytes().as_slice()].concat(),
    );
    assert_eq!(combined, claimed);
}

#[test]
fn vrf_output_is_chained() {
    let kp = make_keypair();
    let mut registry = make_registry_with_validator(&kp, 10_000 * MICRO_RPC_PER_COIN);
    let mut chain = Chain::new(genesis_block());

    let b1 = build_block(&chain, &kp, &registry, vec![], vec![], vec![]);
    let vrf_1 = b1.header.vrf_output;
    chain.add_block(b1, &mut registry).unwrap();

    let b2 = build_block(&chain, &kp, &registry, vec![], vec![], vec![]);
    let vrf_2 = b2.header.vrf_output;

    // The second block's vrf_output must differ from the first's.
    assert_ne!(vrf_1, vrf_2, "vrf_output should change each block");
}

// ── Block-keep window ─────────────────────────────────────────────────────────

#[test]
fn block_keep_window_limits_ram() {
    use rupeecoin_blockchain::BLOCK_KEEP_LAST;
    let kp = make_keypair();
    let mut registry = make_registry_with_validator(&kp, 10_000 * MICRO_RPC_PER_COIN);
    let mut chain = Chain::new(genesis_block());

    let target = BLOCK_KEEP_LAST + 10;
    for _ in 0..target {
        let block = build_block(&chain, &kp, &registry, vec![], vec![], vec![]);
        chain.add_block(block, &mut registry).unwrap();
    }
    assert!(
        chain.blocks.len() <= BLOCK_KEEP_LAST,
        "in-memory block window exceeded: {} > {}",
        chain.blocks.len(),
        BLOCK_KEEP_LAST
    );
}

// ── Mempool integration ───────────────────────────────────────────────────────

#[test]
fn mempool_rejects_coinbase() {
    use rupeecoin_mempool::MempoolError;

    let kp = make_keypair();
    let registry = make_registry_with_validator(&kp, 10_000 * MICRO_RPC_PER_COIN);
    let chain = Chain::new(genesis_block());
    let mut mempool = Mempool::new(4096);

    let coinbase = build_coinbase(1, &kp.public_key, &kp.public_key.to_hash(), &registry);
    let err = mempool.add(coinbase, &chain.utxo_set, 1).unwrap_err();
    assert!(matches!(err, MempoolError::CoinbaseRejected));
}

// ── Checkpoint ────────────────────────────────────────────────────────────────

#[test]
fn checkpoint_should_checkpoint_at_interval() {
    assert!(CheckpointManager::should_checkpoint(CHECKPOINT_INTERVAL));
    assert!(!CheckpointManager::should_checkpoint(CHECKPOINT_INTERVAL - 1));
    assert!(CheckpointManager::should_checkpoint(2 * CHECKPOINT_INTERVAL));
}

// ── Economics ─────────────────────────────────────────────────────────────────

#[test]
fn block_reward_matches_era_boundaries() {
    use rupeecoin_economics::{HALVING_1, HALVING_2, HALVING_3, HALVING_4, TAIL_EMISSION};
    use rupeecoin_types::MICRO_RPC_PER_COIN;

    let initial = 50 * MICRO_RPC_PER_COIN;
    assert_eq!(block_reward(0), initial);
    assert_eq!(block_reward(HALVING_1), initial / 2);
    assert_eq!(block_reward(HALVING_2), initial / 4);
    assert_eq!(block_reward(HALVING_3), initial / 8);
    assert_eq!(block_reward(HALVING_4), TAIL_EMISSION);
}

#[test]
fn fee_market_at_target_is_stable() {
    let fee = 1_000u64;
    assert_eq!(next_base_fee(fee, GAS_TARGET), fee);
}

// ── Privacy integration ───────────────────────────────────────────────────────

#[test]
fn commitment_tree_grows_with_notes() {
    let bundle = PrivacyKeyBundle::generate();
    let mut tree = CommitmentTree::new();
    let initial_root = tree.root();

    let note = rupeecoin_privacy::Note::new(1_000_000, bundle.payment_address.clone());
    tree.append(note.commitment());
    let after_root = tree.root();

    assert_ne!(initial_root, after_root, "tree root must change after appending a commitment");
    assert_eq!(tree.len(), 1);
}

#[test]
fn anchor_depth_limits_usable_roots() {
    // recent_roots should never grow beyond ANCHOR_DEPTH + 1 entries.
    let kp = make_keypair();
    let mut registry = make_registry_with_validator(&kp, 10_000 * MICRO_RPC_PER_COIN);
    let mut chain = Chain::new(genesis_block());

    for _ in 0..ANCHOR_DEPTH + 5 {
        let block = build_block(&chain, &kp, &registry, vec![], vec![], vec![]);
        chain.add_block(block, &mut registry).unwrap();
    }
    assert!(
        chain.recent_roots.len() <= ANCHOR_DEPTH + 1,
        "recent_roots window exceeded: {}",
        chain.recent_roots.len()
    );
}

// ── SDK integration ───────────────────────────────────────────────────────────

#[test]
fn sdk_wallet_addresses_are_deterministic() {
    use rupeecoin_sdk::Wallet;

    let seed = [0xBEu8; 32];
    let w1 = Wallet::from_seed(seed);
    let w2 = Wallet::from_seed(seed);
    assert_eq!(w1.address_at(0), w2.address_at(0));
    assert_eq!(w1.address_at(5), w2.address_at(5));
}

#[test]
fn sdk_coins_to_micro_roundtrip() {
    use rupeecoin_sdk::tx::coins_to_micro;
    assert_eq!(coins_to_micro(1.0), MICRO_RPC_PER_COIN);
    assert_eq!(coins_to_micro(0.0), 0);
}

// ── Reward-claim authentication (audit H-10) ────────────────────────────────────

#[test]
fn signed_reward_claim_to_own_address_is_accepted() {
    let kp = make_keypair();
    let addr = kp.public_key.to_hash();
    let mut registry = make_registry_with_validator(&kp, 10_000 * MICRO_RPC_PER_COIN);
    registry.credit_rewards(&addr, 5 * MICRO_RPC_PER_COIN);
    let mut chain = Chain::new(genesis_block());

    let claim = build_reward_claim(addr, 5 * MICRO_RPC_PER_COIN, kp.public_key.to_address(), &kp);
    let block = build_block(&chain, &kp, &registry, vec![], vec![claim], vec![]);
    chain
        .add_block(block, &mut registry)
        .expect("a properly signed reward claim to the validator's own address must be valid");
}

#[test]
fn reward_claim_redirected_to_attacker_is_rejected() {
    let kp = make_keypair();
    let attacker = make_keypair();
    let addr = kp.public_key.to_hash();
    let mut registry = make_registry_with_validator(&kp, 10_000 * MICRO_RPC_PER_COIN);
    registry.credit_rewards(&addr, 5 * MICRO_RPC_PER_COIN);
    let mut chain = Chain::new(genesis_block());

    // Take a legitimately signed claim and tamper the payout recipient. The
    // signature covers the outputs, so verification must now fail.
    let mut forged =
        build_reward_claim(addr, 5 * MICRO_RPC_PER_COIN, kp.public_key.to_address(), &kp);
    forged.outputs[0].recipient = attacker.public_key.to_address();
    let block = build_block(&chain, &kp, &registry, vec![], vec![forged], vec![]);
    assert!(
        chain.add_block(block, &mut registry).is_err(),
        "a reward claim whose payout was redirected must be rejected"
    );
}

#[test]
fn reward_claim_for_foreign_validator_is_rejected() {
    let victim = make_keypair();
    let attacker = make_keypair();
    let victim_addr = victim.public_key.to_hash();
    let mut registry = make_registry_with_validator(&victim, 10_000 * MICRO_RPC_PER_COIN);
    registry.credit_rewards(&victim_addr, 5 * MICRO_RPC_PER_COIN);
    let mut chain = Chain::new(genesis_block());

    // Attacker claims the victim's accrued rewards, signing with their own key.
    // pk.to_hash() != victim_addr, so the claim is unauthenticated → rejected.
    let forged = build_reward_claim(
        victim_addr,
        5 * MICRO_RPC_PER_COIN,
        attacker.public_key.to_address(),
        &attacker,
    );
    let block = build_block(&chain, &victim, &registry, vec![], vec![forged], vec![]);
    assert!(
        chain.add_block(block, &mut registry).is_err(),
        "a reward claim for a validator the signer does not control must be rejected"
    );
}

#[test]
fn double_reward_claim_in_same_block_is_rejected() {
    let kp = make_keypair();
    let addr = kp.public_key.to_hash();
    let mut registry = make_registry_with_validator(&kp, 10_000 * MICRO_RPC_PER_COIN);
    registry.credit_rewards(&addr, 5 * MICRO_RPC_PER_COIN);
    let mut chain = Chain::new(genesis_block());

    // Two valid signed claims for the full balance; cumulative payout (10) would
    // exceed the 5 available, so the block must be rejected.
    let c1 = build_reward_claim(addr, 5 * MICRO_RPC_PER_COIN, kp.public_key.to_address(), &kp);
    let c2 = build_reward_claim(addr, 5 * MICRO_RPC_PER_COIN, kp.public_key.to_address(), &kp);
    let block = build_block(&chain, &kp, &registry, vec![], vec![c1, c2], vec![]);
    assert!(
        chain.add_block(block, &mut registry).is_err(),
        "two full-balance reward claims in one block must be rejected"
    );
}

// ── Restart / replay ────────────────────────────────────────────────────────────

#[test]
fn replay_reconstructs_state_without_registry() {
    // Regression: on restart a node must rebuild chain state from its persisted,
    // already-validated blocks WITHOUT the validator registry (the historical
    // validator set is not reconstructable from block data). Re-running full
    // validation against an empty registry previously failed VRF verification and
    // made the node unable to restart at all.
    let kp = make_keypair();
    let mut registry = make_registry_with_validator(&kp, 10_000 * MICRO_RPC_PER_COIN);
    let mut chain = Chain::new(genesis_block());

    // Produce blocks the normal way; keep them to stand in for the store.
    let mut stored = Vec::new();
    for _ in 0..6 {
        let block = build_block(&chain, &kp, &registry, vec![], vec![], vec![]);
        stored.push(block.clone());
        chain.add_block(block, &mut registry).expect("production block should be valid");
    }
    let expected_height = chain.height();
    let expected_utxo_root = chain.utxo_set.state_root().expect("UTXO serialization failed");
    let expected_privacy_root = chain.commitment_tree.root();

    // Restart: fresh chain, no registry available, replay the stored blocks.
    let mut replayed = Chain::new(genesis_block());
    for block in stored {
        replayed
            .replay_block(block)
            .expect("replay of a persisted, previously-validated block must succeed");
    }

    assert_eq!(replayed.height(), expected_height, "replayed height must match production");
    assert_eq!(
        replayed.utxo_set.state_root().expect("UTXO serialization failed"),
        expected_utxo_root,
        "replayed UTXO state must match production",
    );
    assert_eq!(
        replayed.commitment_tree.root(),
        expected_privacy_root,
        "replayed privacy state must match production",
    );
}

#[test]
fn replay_detects_corrupted_state_root() {
    // The integrity guard: if a persisted block's committed state_root does not
    // match the recomputed state, replay must reject it rather than load bad state.
    let kp = make_keypair();
    let mut registry = make_registry_with_validator(&kp, 10_000 * MICRO_RPC_PER_COIN);
    let mut chain = Chain::new(genesis_block());

    let mut block = build_block(&chain, &kp, &registry, vec![], vec![], vec![]);
    chain.add_block(block.clone(), &mut registry).unwrap();

    // Tamper with the committed state root, then replay onto a fresh chain.
    block.header.state_root = Hash::from_bytes([0xAB; 32]);
    let mut replayed = Chain::new(genesis_block());
    assert!(
        replayed.replay_block(block).is_err(),
        "replay must reject a block whose state_root does not match recomputed state",
    );
}
