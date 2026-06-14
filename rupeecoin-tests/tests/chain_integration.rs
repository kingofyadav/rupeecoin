//! Integration tests: full chain construction across multiple crates.

use rupeecoin_blockchain::{build_block, build_coinbase, genesis_block, Chain, ANCHOR_DEPTH};
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

    // After applying the block, the chain's UTXO set state root must match
    // what the block header committed.
    assert_eq!(chain.utxo_set.state_root(), claimed);
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
