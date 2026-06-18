use rupeecoin_crypto::{Address, Hash};
use rupeecoin_economics::INITIAL_BASE_FEE;
use rupeecoin_types::{
    block::merkle_root, Block, BlockHeader, Transaction, TxOutput, BLOCK_REWARD, MAINNET_CHAIN_ID,
};
use rupeecoin_utxo::UtxoSet;

pub fn genesis_block() -> Block {
    genesis_block_with_timestamp(1_700_000_000)
}

pub fn genesis_block_with_timestamp(timestamp: u64) -> Block {
    let coinbase = Transaction {
        version: 1,
        chain_id: MAINNET_CHAIN_ID,
        nonce: 0,
        expiry: 0,
        inputs: vec![],
        outputs: vec![TxOutput {
            amount: BLOCK_REWARD,
            recipient: Address::zero(),
        }],
        coinbase_data: Some(
            b"RupeeCoin Genesis \xe2\x80\x94 From India, For the World, Owned by No One".to_vec(),
        ),
    };

    let transactions = vec![coinbase];
    let mr = merkle_root(&transactions);

    // Compute the actual state_root after applying the genesis coinbase.
    let mut utxo_snap = UtxoSet::new();
    let genesis_placeholder = Block {
        header: BlockHeader {
            version: 1,
            prev_hash: Hash::zero(),
            merkle_root: mr,
            timestamp,
            height: 0,
            slot: 0,
            proposer_index: 0,
            privacy_root: Hash::zero(),
            base_fee: INITIAL_BASE_FEE,
            gas_used: 0,
            state_root: Hash::zero(),
            vrf_output: Hash::zero(),
        },
        transactions: transactions.clone(),
        attestations: vec![],
        proposer_signature: None,
        privacy_txs: vec![],
        vrf_proof: None,
    };
    utxo_snap
        .apply_block(&genesis_placeholder)
        .expect("genesis coinbase is valid");
    let genesis_state_root = utxo_snap.state_root().expect("UTXO serialization failed");

    let header = BlockHeader {
        version: 1,
        prev_hash: Hash::zero(),
        merkle_root: mr,
        timestamp,
        height: 0,
        slot: 0,
        proposer_index: 0,
        privacy_root: Hash::zero(),
        base_fee: INITIAL_BASE_FEE,
        gas_used: 0,
        state_root: genesis_state_root,
        vrf_output: Hash::zero(),
    };

    Block {
        header,
        transactions,
        attestations: vec![],
        proposer_signature: None,
        privacy_txs: vec![],
        vrf_proof: None,
    }
}
