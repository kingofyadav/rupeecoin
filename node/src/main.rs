mod rpc;
mod tor;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use libp2p::{Multiaddr, PeerId};
use tokio::time;
use tracing::{error, info, warn};

use rupeecoin_blockchain::ANCHOR_DEPTH;
use rupeecoin_blockchain::{
    build_block, build_reward_claim, build_slash_report, genesis_block_with_timestamp, Chain,
    REORG_DEPTH,
};
use rupeecoin_consensus::{
    select_proposer, slot_to_epoch, CheckpointManager, ConsensusState, WeakSubjectivityCheckpoint,
    SLOTS_PER_EPOCH, SLOT_DURATION_SECS,
};
use rupeecoin_crypto::{Hash, KeyPair, Signature};
use rupeecoin_mempool::Mempool;
use rupeecoin_network::{NetworkCommand, NetworkEvent};
use rupeecoin_privacy::{PrivacyBlockApply, PrivacyTransaction};
use rupeecoin_staking::{ValidatorRegistry, MIN_STAKE_MICRO_RPC};
use rupeecoin_storage::ChainStore;
use rupeecoin_types::{
    Attestation, Block, GenesisConfig, ValidatorAnnouncement, MICRO_RPC_PER_COIN,
};
use std::collections::HashMap as StdHashMap;

// ── CLI ───────────────────────────────────────────────────────────────────────

struct Config {
    data_dir: PathBuf,
    listen: Multiaddr,
    peers: Vec<Multiaddr>,
    block_interval_secs: u64,
    stake_micro_rpc: u64,
    rpc_port: u16,
    /// Bootstrap checkpoint supplied via --checkpoint height:hash_hex.
    trusted_checkpoint: Option<WeakSubjectivityCheckpoint>,
    /// Optional deterministic validator seed (hex-encoded 32 bytes).
    /// When set, the validator keypair is derived reproducibly so that
    /// coinbase UTXOs can be spent by external tools (e.g. fund_faucet).
    validator_seed: Option<[u8; 32]>,
    /// Phase 13.3: optional Tor SOCKS5 proxy (host:port).
    tor: Option<tor::TorConfig>,
    /// Phase 13.2: optional custom genesis config loaded from a JSON file.
    genesis_config: Option<GenesisConfig>,
    /// Phase 14.6: custom genesis timestamp (unix seconds). Default: 1_700_000_000.
    genesis_timestamp: u64,
    /// Phase 15 N-04: optional allowlist of trusted gossipsub PeerIds.
    peer_allowlist: Option<HashSet<PeerId>>,
}

fn parse_args() -> Result<Config> {
    let mut data_dir = PathBuf::from("./rupeecoin-data");
    let mut listen: Multiaddr = "/ip4/0.0.0.0/tcp/30333".parse()?;
    let mut peers = Vec::new();
    let mut block_interval_secs = SLOT_DURATION_SECS;
    let mut stake_rpc = 0u64;
    let mut rpc_port: u16 = 9944;
    let mut trusted_checkpoint: Option<WeakSubjectivityCheckpoint> = None;
    let mut validator_seed: Option<[u8; 32]> = None;
    let mut tor_config: Option<tor::TorConfig> = None;
    let mut genesis_config: Option<GenesisConfig> = None;
    let mut genesis_timestamp: u64 = 1_700_000_000;
    let mut peer_allowlist: Option<HashSet<PeerId>> = None;

    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--data-dir" => {
                data_dir = PathBuf::from(args.next().context("--data-dir requires a value")?);
            }
            "--listen" => {
                listen = args.next().context("--listen requires a value")?.parse()?;
            }
            "--peer" => {
                peers.push(args.next().context("--peer requires a value")?.parse()?);
            }
            "--block-interval" => {
                block_interval_secs = args
                    .next()
                    .context("--block-interval requires a value")?
                    .parse()?;
            }
            "--stake" => {
                stake_rpc = args.next().context("--stake requires a value")?.parse()?;
            }
            "--rpc-port" => {
                rpc_port = args
                    .next()
                    .context("--rpc-port requires a value")?
                    .parse()?;
            }
            "--no-rpc" => {
                rpc_port = 0;
            }
            "--checkpoint" => {
                // Format: height:block_hash_hex:state_root_hex
                let val = args
                    .next()
                    .context("--checkpoint requires height:hash:state_root")?;
                let parts: Vec<&str> = val.splitn(3, ':').collect();
                anyhow::ensure!(
                    parts.len() == 3,
                    "--checkpoint requires height:block_hash_hex:state_root_hex"
                );
                let height: u64 = parts[0].parse().context("checkpoint height must be u64")?;
                let hash_bytes: [u8; 32] = hex::decode(parts[1])
                    .context("checkpoint block_hash must be hex")?
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("block_hash must be 32 bytes"))?;
                let state_bytes: [u8; 32] = hex::decode(parts[2])
                    .context("checkpoint state_root must be hex")?
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("state_root must be 32 bytes"))?;
                trusted_checkpoint = Some(WeakSubjectivityCheckpoint {
                    height,
                    block_hash: Hash::from_bytes(hash_bytes),
                    state_root: Hash::from_bytes(state_bytes),
                });
            }
            "--validator-seed" => {
                let hex_val = args
                    .next()
                    .context("--validator-seed requires a 64-char hex value")?;
                let raw = hex::decode(&hex_val).context("--validator-seed must be valid hex")?;
                validator_seed = Some(raw.try_into().map_err(|_| {
                    anyhow::anyhow!("--validator-seed must be exactly 32 bytes (64 hex chars)")
                })?);
            }
            "--tor-socks" => {
                let addr = args.next().context("--tor-socks requires host:port")?;
                tor_config =
                    Some(tor::TorConfig::parse(&addr).map_err(|e| anyhow::anyhow!("{e}"))?);
            }
            "--genesis-config" => {
                let path = args
                    .next()
                    .context("--genesis-config requires a file path")?;
                let json = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading genesis config from '{path}'"))?;
                let cfg = GenesisConfig::from_json(&json)
                    .with_context(|| format!("parsing genesis config from '{path}'"))?;
                cfg.validate()
                    .map_err(|e| anyhow::anyhow!("invalid genesis config: {e}"))?;
                genesis_config = Some(cfg);
            }
            "--genesis-timestamp" => {
                genesis_timestamp = args
                    .next()
                    .context("--genesis-timestamp requires a unix timestamp (seconds)")?
                    .parse()?;
            }
            "--peer-allowlist" => {
                let path = args
                    .next()
                    .context("--peer-allowlist requires a file path")?;
                let content = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading peer allowlist from '{path}'"))?;
                let mut set = HashSet::new();
                for line in content.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    let peer = line
                        .parse::<PeerId>()
                        .with_context(|| format!("invalid PeerId in allowlist: '{line}'"))?;
                    set.insert(peer);
                }
                info!("Peer allowlist loaded: {} trusted peers", set.len());
                peer_allowlist = Some(set);
            }
            "--treasury-address" => {
                let hex_addr = args
                    .next()
                    .context("--treasury-address requires a 40-char hex address")?;
                let raw = hex::decode(&hex_addr).context("--treasury-address must be valid hex")?;
                let arr: [u8; 20] = raw.try_into().map_err(|_| {
                    anyhow::anyhow!("--treasury-address must be exactly 20 bytes (40 hex chars)")
                })?;
                rupeecoin_economics::set_treasury_address(rupeecoin_crypto::Address(arr));
            }
            "--help" | "-h" => {
                println!(
                    "rupeecoin-node\n\
                     \n\
                     FLAGS:\n\
                     --data-dir <path>                       chain data directory   [default: ./rupeecoin-data]\n\
                     --listen <multiaddr>                    listen address         [default: /ip4/0.0.0.0/tcp/30333]\n\
                     --peer <multiaddr>                      dial this peer (repeatable)\n\
                     --block-interval <secs>                 slot duration          [default: {SLOT_DURATION_SECS}]\n\
                     --stake <RPC>                           stake to join validator set\n\
                     --rpc-port <port>                       HTTP JSON-RPC port     [default: 9944]\n\
                     --no-rpc                                disable HTTP RPC\n\
                     --checkpoint <height:hash:state_root>   bootstrap checkpoint (skip validation before height)\n\
                     --validator-seed <hex>                  32-byte seed for deterministic validator keypair\n\
                     --tor-socks <host:port>                 route outgoing P2P connections through Tor SOCKS5 proxy\n\
                     --genesis-config <path>                 load genesis parameters from JSON file (overrides built-in mainnet config)\n\
                     --genesis-timestamp <unix_secs>         override genesis block timestamp (default: 1700000000)\n\
                     --treasury-address <hex>                override treasury address (40 hex chars / 20 bytes)\n\
                     --peer-allowlist <path>                 restrict gossipsub to listed PeerIds (one per line)\n\
                     \n\
                     Example: --stake 10000 --checkpoint 604800:<hash_hex>:<state_hex>\n\
                     Tor example: --tor-socks 127.0.0.1:9050 --peer /dns4/xyz.onion/tcp/30333\n\
                     Allowlist: --peer-allowlist /etc/rupeecoin/trusted-peers.txt"
                );
                std::process::exit(0);
            }
            other => {
                eprintln!("unknown flag: {other}  (use --help)");
                std::process::exit(1);
            }
        }
    }

    Ok(Config {
        data_dir,
        listen,
        peers,
        block_interval_secs,
        stake_micro_rpc: stake_rpc.saturating_mul(MICRO_RPC_PER_COIN),
        rpc_port,
        trusted_checkpoint,
        validator_seed,
        tor: tor_config,
        genesis_config,
        genesis_timestamp,
        peer_allowlist,
    })
}

// ── Chain init ────────────────────────────────────────────────────────────────

fn load_or_create_chain(
    store: &ChainStore,
    checkpoint_mgr: &CheckpointManager,
    genesis_timestamp: u64,
) -> Result<Chain> {
    match store.get_tip().context("reading chain tip from store")? {
        None => {
            info!("No existing chain — creating genesis block");
            let genesis = genesis_block_with_timestamp(genesis_timestamp);
            store
                .apply_block(&genesis)
                .context("storing genesis block")?;
            Ok(Chain::new(genesis))
        }
        Some((_, tip_height)) => {
            info!("Replaying chain from store (height {tip_height})…");
            let genesis = store
                .get_block_by_height(0)?
                .context("genesis block missing from store")?;
            let mut chain = Chain::new(genesis);

            // Skip full validation for heights at or below the trusted checkpoint.
            let (checkpoint_height, checkpoint_block_hash, checkpoint_state_root) = checkpoint_mgr
                .latest()
                .map(|c| (c.height, Some(c.block_hash), Some(c.state_root)))
                .unwrap_or((0, None, None));
            if checkpoint_height > 0 {
                info!("Checkpoint at height {checkpoint_height} — skipping validation up to that point");
            }

            for h in 1..=tip_height {
                let block = store
                    .get_block_by_height(h)?
                    .with_context(|| format!("block at height {h} missing"))?;

                if h <= checkpoint_height {
                    // Verify block hash AND state_root at the checkpoint boundary.
                    if h == checkpoint_height {
                        if let Some(expected) = checkpoint_block_hash {
                            let actual = block.hash();
                            anyhow::ensure!(
                                actual == expected,
                                "checkpoint hash mismatch at height {h}: \
                                 trusted {expected}, store has {actual}"
                            );
                        }
                    }
                    // Fast-path: apply UTXO + privacy state without consensus validation.
                    chain.utxo_set.apply_block(&block).ok();

                    // At the checkpoint boundary, verify the UTXO state_root so a
                    // manipulated UTXO set cannot pass checkpoint validation.
                    if h == checkpoint_height {
                        if let Some(expected_sr) = checkpoint_state_root {
                            let actual_sr = chain
                                .utxo_set
                                .state_root()
                                .expect("UTXO serialization failed");
                            anyhow::ensure!(
                                actual_sr == expected_sr,
                                "checkpoint state_root mismatch at height {h}: \
                                 trusted {expected_sr}, computed {actual_sr}"
                            );
                        }
                    }

                    // Update privacy fields (finding #4).
                    let apply =
                        PrivacyBlockApply::compute(&block.privacy_txs, &chain.commitment_tree, h);
                    for cm in &apply.new_commitments {
                        chain.commitment_tree.append(*cm);
                    }
                    for (nullifier, _) in &apply.new_nullifiers {
                        chain.spent_nullifiers.insert(*nullifier);
                    }
                    chain.recent_roots.push(apply.new_tree_root);
                    if chain.recent_roots.len() > ANCHOR_DEPTH + 1 {
                        chain.recent_roots.remove(0);
                    }
                    chain.blocks.push(block);
                } else {
                    // Trust locally-persisted, previously-validated blocks: rebuild
                    // UTXO/privacy state without re-running consensus validation.
                    // (Historical validator set for VRF/proposer checks is not
                    // reconstructable; the registry is restored from metadata.)
                    chain
                        .replay_block(block)
                        .with_context(|| format!("replaying block {h}"))?;
                }
            }
            info!("Chain loaded — height {}", chain.height());
            Ok(chain)
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn attest(keypair: &KeyPair, block: &Block) -> Attestation {
    let block_hash = block.hash();
    let sig_data = Attestation::sig_data(block.header.slot, &block_hash);
    Attestation {
        slot: block.header.slot,
        block_hash,
        validator_pubkey: keypair.public_key.clone(),
        signature: keypair.sign(sig_data.as_bytes()),
    }
}

fn update_rpc_snapshot(
    snapshot: &Arc<Mutex<rpc::RpcChainSnapshot>>,
    chain: &Chain,
    registry: &ValidatorRegistry,
    mempool: &rupeecoin_mempool::Mempool,
    consensus: &ConsensusState,
) {
    match snapshot.try_lock() {
        Ok(mut snap) => {
            let height = chain.height();
            snap.height = height;
            snap.slot = height;
            snap.epoch = height / SLOTS_PER_EPOCH;
            snap.last_justified_epoch = consensus.finality.last_justified_epoch();
            snap.last_finalized_epoch = consensus.finality.last_finalized_epoch();
            snap.active_validators = registry.active_count();
            snap.total_stake = registry.total_active_stake();
            snap.mempool_size = mempool.len();
            snap.base_fee = chain.tip().header.base_fee;
            snap.recent_roots = chain.recent_roots.clone();
            snap.commitment_tree = chain.commitment_tree.clone();
            snap.nonces = chain
                .utxo_set
                .nonces
                .iter()
                .map(|(addr, &n)| (hex::encode(addr.0), n))
                .collect();
            snap.validators = registry
                .iter_active()
                .map(|v| {
                    let addr_hex = hex::encode(v.address.as_bytes());
                    let c = &v.contribution;
                    let uptime_score = if c.attestations_assigned == 0 {
                        1.0
                    } else {
                        c.attestations_timely as f64 / c.attestations_assigned as f64
                    };
                    let proposal_score = if c.slots_assigned == 0 {
                        1.0
                    } else {
                        c.slots_timely as f64 / c.slots_assigned as f64
                    };
                    let vs = rpc::ValidatorSnapshot {
                        stake: v.stake,
                        claimable_rewards: v.claimable_rewards,
                        uptime_score,
                        proposal_score,
                        total_score: c.score(),
                        is_active: matches!(v.status, rupeecoin_staking::ValidatorStatus::Active),
                        status: format!("{:?}", v.status),
                    };
                    (addr_hex, vs)
                })
                .collect();
        }
        Err(_) => warn!("RPC snapshot update skipped: lock held by RPC handler"),
    }
}

fn persist_contract_storage(store: &ChainStore, block: &rupeecoin_types::Block, height: u64) {
    use rupeecoin_rvm::{run, ExecContext, ExecResult};
    use rupeecoin_types::contract::{ContractCall, ContractDeploy};
    for tx in &block.transactions {
        let Some(ref data) = tx.coinbase_data else {
            continue;
        };
        if let Some(deploy) = ContractDeploy::decode(data) {
            let ctx = ExecContext {
                caller: 0,
                call_value: deploy.value,
                block_height: height,
                timestamp: block.header.timestamp,
                chain_id: 1,
                self_addr: tx.txid().as_bytes()[..8]
                    .try_into()
                    .map(u64::from_le_bytes)
                    .unwrap_or(0),
                gas_limit: deploy.gas_limit,
                calldata: deploy.constructor_args,
            };
            let contract_addr = ctx.self_addr;
            match store.load_contract_storage(contract_addr) {
                Ok(storage) => {
                    if let Ok((ExecResult::Return { .. }, Some(new_storage))) =
                        run(&deploy.bytecode, storage, ctx)
                    {
                        if let Err(e) = store.save_contract_storage(contract_addr, &new_storage) {
                            error!("persist contract storage (deploy) addr={contract_addr}: {e}");
                        }
                        if let Err(e) = store.save_contract_code(contract_addr, &deploy.bytecode) {
                            error!("persist contract code (deploy) addr={contract_addr}: {e}");
                        }
                    }
                }
                Err(e) => error!("load contract storage (deploy) addr={contract_addr}: {e}"),
            }
        } else if let Some(call) = ContractCall::decode(data) {
            match store.load_contract_code(call.contract_addr) {
                Ok(Some(bytecode)) => match store.load_contract_storage(call.contract_addr) {
                    Ok(storage) => {
                        let ctx = ExecContext {
                            caller: 0,
                            call_value: call.value,
                            block_height: height,
                            timestamp: block.header.timestamp,
                            chain_id: 1,
                            self_addr: call.contract_addr,
                            gas_limit: call.gas_limit,
                            calldata: call.calldata,
                        };
                        if let Ok((ExecResult::Return { .. }, Some(new_storage))) =
                            run(&bytecode, storage, ctx)
                        {
                            if let Err(e) =
                                store.save_contract_storage(call.contract_addr, &new_storage)
                            {
                                error!(
                                    "persist contract storage (call) addr={}: {e}",
                                    call.contract_addr
                                );
                            }
                        }
                    }
                    Err(e) => error!(
                        "load contract storage (call) addr={}: {e}",
                        call.contract_addr
                    ),
                },
                Ok(None) => warn!(
                    "RVM call to {}: contract not deployed on this node",
                    call.contract_addr
                ),
                Err(e) => error!("load contract code (call) addr={}: {e}", call.contract_addr),
            }
        }
    }
}

fn persist_registry(store: &ChainStore, registry: &ValidatorRegistry) {
    match bincode::serialize(registry) {
        Ok(bytes) => {
            if let Err(e) = store.save_raw_meta(b"validator_registry", &bytes) {
                error!("Failed to persist validator registry: {e}");
            }
        }
        Err(e) => error!("Failed to serialize validator registry: {e}"),
    }
}

fn maybe_save_checkpoint(store: &ChainStore, chain: &Chain) {
    let h = chain.height();
    if CheckpointManager::should_checkpoint(h) {
        let cp = WeakSubjectivityCheckpoint {
            height: h,
            block_hash: chain.tip().hash(),
            state_root: chain.tip().header.state_root,
        };
        if let Err(e) = store.save_checkpoint(&cp) {
            error!("Failed to save checkpoint at height {h}: {e}");
        } else {
            info!(
                "Saved weak-subjectivity checkpoint at height {h}  hash={}",
                cp.block_hash
            );
        }
    }
}

// ── LMD-GHOST reorg helper ────────────────────────────────────────────────────

struct ReorgResult {
    rolled_back: usize,
    applied: usize,
}

/// Attempt to reorg the canonical chain so it ends at `lmd_head`.
///
/// Returns `Some(ReorgResult)` when the reorg succeeded, `None` when it cannot
/// be performed (missing fork blocks, divergence exceeds REORG_DEPTH, etc.).
fn try_reorg(
    chain: &mut Chain,
    registry: &mut rupeecoin_staking::ValidatorRegistry,
    consensus: &ConsensusState,
    fork_blocks: &StdHashMap<Hash, rupeecoin_types::Block>,
    lmd_head: Hash,
) -> Option<ReorgResult> {
    // Walk back from lmd_head to find the path to the common ancestor.
    let mut fork_path: Vec<Hash> = vec![lmd_head];
    let mut cursor = lmd_head;
    loop {
        let parent = *consensus.known_block_parents.get(&cursor)?;
        if chain.contains_hash(&parent) {
            break; // found common ancestor
        }
        fork_path.push(parent);
        cursor = parent;
        if fork_path.len() > REORG_DEPTH {
            return None; // too deep — refuse the reorg
        }
    }

    // Collect actual Block objects for the fork path (oldest first).
    fork_path.reverse();
    let fork_blocks_ordered: Vec<rupeecoin_types::Block> = fork_path
        .iter()
        .map(|h| fork_blocks.get(h).cloned())
        .collect::<Option<Vec<_>>>()?;

    // Determine how many canonical blocks to roll back so the common ancestor
    // becomes the new tip.  The common ancestor is the parent of fork_path[0].
    let canon_tip_height = chain.height();
    // Derive ancestor height from the oldest fork block (first in ordered list).
    let ancestor_height = fork_blocks_ordered
        .first()
        .map(|b| b.header.height.saturating_sub(1))
        .unwrap_or(0);
    let rollback_count = (canon_tip_height.saturating_sub(ancestor_height)) as usize;

    let removed = chain.rollback(rollback_count);
    let rolled_back = removed.len();
    let applied = fork_blocks_ordered.len();
    chain.apply_fork(fork_blocks_ordered, registry).ok()?;

    Some(ReorgResult {
        rolled_back,
        applied,
    })
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "rupeecoin_node=info,rupeecoin_network=info"
                    .parse()
                    .unwrap()
            }),
        )
        .init();

    let cfg = parse_args()?;

    std::fs::create_dir_all(&cfg.data_dir)?;
    info!("Opening store at {}", cfg.data_dir.display());
    let store = ChainStore::open(&cfg.data_dir).context("opening RocksDB")?;

    // ── Checkpoint manager ────────────────────────────────────────────────────
    let mut checkpoint_mgr = CheckpointManager::new();
    // Load all previously saved checkpoints.
    for cp in store.load_checkpoints().unwrap_or_default() {
        checkpoint_mgr.add_trusted(cp);
    }
    // Apply any checkpoint supplied on the command line.
    if let Some(cp) = cfg.trusted_checkpoint {
        info!(
            "Trusting checkpoint at height {} — hash={}",
            cp.height, cp.block_hash
        );
        checkpoint_mgr.add_trusted(cp);
    }
    if let Some(latest) = checkpoint_mgr.latest() {
        info!("Latest trusted checkpoint: height={}", latest.height);
    }

    let kp = match cfg.validator_seed {
        Some(seed) => {
            info!("Using deterministic validator keypair (--validator-seed)");
            KeyPair::from_seed(&seed)
        }
        None => {
            info!("Generating random Dilithium3 validator keypair…");
            KeyPair::generate()
        }
    };
    let my_addr = kp.public_key.to_hash();
    let my_transparent_addr = kp.public_key.to_address();
    info!("Validator address (registry): {my_addr}");
    info!(
        "Validator address (UTXO/transparent): {}",
        rupeecoin_crypto::encode_address(&my_transparent_addr)
    );

    let store = Arc::new(store);
    let mut chain = load_or_create_chain(&store, &checkpoint_mgr, cfg.genesis_timestamp)?;
    let mut mempool = Mempool::new(4096);

    // 8.2: restore persisted validator registry so claimable_rewards survive restart.
    let mut registry: ValidatorRegistry = match store.load_raw_meta(b"validator_registry") {
        Ok(Some(b)) => match bincode::deserialize(&b) {
            Ok(reg) => reg,
            Err(e) => {
                warn!("validator_registry deserialization failed ({e}); starting fresh");
                ValidatorRegistry::new()
            }
        },
        Ok(None) => ValidatorRegistry::new(),
        Err(e) => {
            warn!("validator_registry load failed ({e}); starting fresh");
            ValidatorRegistry::new()
        }
    };

    let mut consensus = ConsensusState::new();
    let mut pending_attestations: Vec<Attestation> = Vec::new();
    // Heights for which a sync request is already in-flight (deduplication).
    let mut pending_sync_heights: HashSet<u64> = HashSet::new();

    let rpc_chain_snapshot = Arc::new(Mutex::new(rpc::RpcChainSnapshot {
        height: chain.height(),
        slot: chain.height(),
        epoch: chain.height() / SLOTS_PER_EPOCH,
        last_justified_epoch: 0,
        last_finalized_epoch: 0,
        active_validators: 0,
        total_stake: 0,
        mempool_size: 0,
        base_fee: chain.tip().header.base_fee,
        recent_roots: chain.recent_roots.clone(),
        commitment_tree: chain.commitment_tree.clone(),
        nonces: std::collections::HashMap::new(),
        validators: std::collections::HashMap::new(),
    }));
    let mut pending_slash_reports: Vec<rupeecoin_types::SlashingEvidence> = Vec::new();
    // Fork blocks cache: blocks on competing forks, keyed by block hash.
    // Used by the LMD-GHOST reorg logic to find fork chain paths.
    let mut fork_blocks: StdHashMap<Hash, rupeecoin_types::Block> = StdHashMap::new();
    let pending_privacy_txs: Arc<Mutex<Vec<PrivacyTransaction>>> = Arc::new(Mutex::new(Vec::new()));
    let pending_txs: Arc<Mutex<Vec<rupeecoin_types::Transaction>>> =
        Arc::new(Mutex::new(Vec::new()));

    let is_validator = cfg.stake_micro_rpc >= MIN_STAKE_MICRO_RPC;

    let node_chain_id = cfg
        .genesis_config
        .as_ref()
        .map_or_else(|| GenesisConfig::mainnet().chain_id, |g| g.chain_id);

    if cfg.rpc_port > 0 {
        let rpc_state = Arc::new(rpc::RpcState {
            chain_snapshot: Arc::clone(&rpc_chain_snapshot),
            store: Arc::clone(&store),
            chain_id: node_chain_id,
            pending_privacy_txs: Arc::clone(&pending_privacy_txs),
            pending_txs: Arc::clone(&pending_txs),
        });
        let port = cfg.rpc_port;
        tokio::spawn(async move {
            if let Err(e) = rpc::serve(rpc_state, port).await {
                tracing::error!("RPC server error: {e:#}");
            }
        });
    }

    // ── Phase 13.3: Tor / SOCKS5 ─────────────────────────────────────────────
    if let Some(ref tor_cfg) = cfg.tor {
        info!("Tor mode enabled — SOCKS5 proxy at {}", tor_cfg.proxy_addr);
        match tor::check_proxy_reachability(tor_cfg.proxy_addr).await {
            Ok(()) => info!("Tor SOCKS5 proxy is reachable and responding"),
            Err(e) => warn!("{e}  (proceeding without Tor — check that Tor is running)"),
        }
    }

    // ── Phase 13.2: Genesis config ────────────────────────────────────────────
    if let Some(ref gen_cfg) = cfg.genesis_config {
        info!(
            "Custom genesis config loaded: chain_id={} network={}",
            gen_cfg.chain_id, gen_cfg.network_name
        );
    } else {
        let default_gen = GenesisConfig::mainnet();
        info!(
            "Using built-in genesis config: chain_id={} network={}",
            default_gen.chain_id, default_gen.network_name
        );
    }

    info!("Starting P2P network on {}", cfg.listen);
    let tor_proxy = cfg.tor.as_ref().map(|t| t.proxy_addr);
    let (net_cmd, mut net_event) =
        rupeecoin_network::start(cfg.listen, tor_proxy, cfg.peer_allowlist)
            .context("starting network")?;

    for addr in cfg.peers {
        info!("Dialing seed peer {addr}");
        let _ = net_cmd.send(NetworkCommand::Dial(addr)).await;
    }

    if is_validator {
        // On a fresh start this registers the validator; on restart the validator
        // is already present in the registry restored from persisted metadata, in
        // which case AlreadyRegistered is expected and harmless.
        match registry.register(kp.public_key.clone(), cfg.stake_micro_rpc, chain.height()) {
            Ok(()) => info!(
                "Joined validator set — stake {} micro-RPC — active set: {}",
                cfg.stake_micro_rpc,
                registry.active_count()
            ),
            Err(rupeecoin_staking::StakingError::AlreadyRegistered) => info!(
                "Validator already present in restored registry — active set: {}",
                registry.active_count()
            ),
            Err(e) => panic!("validator self-registration failed: {e}"),
        }
        let sig_data = ValidatorAnnouncement::sig_data(&kp.public_key, cfg.stake_micro_rpc);
        let ann = ValidatorAnnouncement {
            pubkey: kp.public_key.clone(),
            stake: cfg.stake_micro_rpc,
            signature: kp.sign(sig_data.as_bytes()),
        };
        let _ = net_cmd
            .send(NetworkCommand::BroadcastValidatorAnnouncement(Box::new(
                ann,
            )))
            .await;
    } else if cfg.stake_micro_rpc > 0 {
        warn!(
            "Stake {} micro-RPC below minimum {} — running as observer",
            cfg.stake_micro_rpc, MIN_STAKE_MICRO_RPC
        );
    }

    let slot_start = time::Instant::now() + Duration::from_secs(cfg.block_interval_secs);
    let mut slot_timer =
        time::interval_at(slot_start, Duration::from_secs(cfg.block_interval_secs));
    slot_timer.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

    let mut announce_timer = time::interval(Duration::from_secs(2));
    announce_timer.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);

    info!(
        "Node running — height {} — validator={} — first slot in {}s",
        chain.height(),
        is_validator,
        cfg.block_interval_secs,
    );

    loop {
        tokio::select! {
            Some(ev) = net_event.recv() => match ev {

                // ── Block from gossipsub ──────────────────────────────────────────
                NetworkEvent::BlockReceived { block, from } => {
                    let height = block.header.height;
                    pending_sync_heights.remove(&height);
                    match chain.add_block(*block.clone(), &mut registry) {
                        Ok(()) => {
                            if let Err(e) = store.apply_block(&block) {
                                error!("persist block {height}: {e}");
                            }
                            // 14.4: persist RVM contract storage for contract txs.
                            persist_contract_storage(&store, &block, height);
                            // 8.2: persist registry so rewards survive restart.
                            persist_registry(&store, &registry);
                            maybe_save_checkpoint(&store, &chain);
                            update_rpc_snapshot(&rpc_chain_snapshot, &chain, &registry, &mempool, &consensus);
                            mempool.evict_conflicts(
                                &block.transactions.iter().map(|t| t.txid()).collect::<Vec<_>>(),
                            );
                            consensus.on_block_accepted(&block, &registry);

                            // 8.3: detect double-proposal slashing.
                            if let Some(sig) = &block.proposer_signature {
                                let idx = block.header.proposer_index as usize;
                                if let Some(proposer) = registry
                                    .active_set()
                                    .get(idx)
                                    .and_then(|a| registry.get(a))
                                {
                                    if let Some(evidence) = consensus.check_and_record_proposal(
                                        block.header.slot,
                                        block.hash(),
                                        &proposer.pubkey.clone(),
                                        sig,
                                    ) {
                                        warn!("Slashable double-proposal detected at slot {}!", block.header.slot);
                                        pending_slash_reports.push(evidence);
                                    }
                                }
                            }

                            // 8.3: detect double-vote slashing from embedded attestations.
                            for att in &block.attestations {
                                if let Some(evidence) = consensus.slashing_detector.record_attestation(att.clone()) {
                                    warn!("Slashable double-vote detected at slot {}!", att.slot);
                                    pending_slash_reports.push(evidence);
                                }
                            }

                            if is_validator {
                                let att = attest(&kp, &block);
                                pending_attestations.push(att.clone());
                                let _ = net_cmd
                                    .send(NetworkCommand::BroadcastAttestation(Box::new(att)))
                                    .await;
                            }

                            // 14.3: LMD-GHOST reorg — check if the canonical head has shifted.
                            let tip_hash = chain.tip().hash();
                            let lmd_head = consensus.lmd_select_head(tip_hash, &registry);
                            if lmd_head != tip_hash {
                                // The heaviest attestation-weighted subtree is not at our tip.
                                // Attempt a reorg if we have the fork blocks cached and the
                                // common ancestor is within REORG_DEPTH.
                                if let Some(reorg_result) = try_reorg(
                                    &mut chain,
                                    &mut registry,
                                    &consensus,
                                    &fork_blocks,
                                    lmd_head,
                                ) {
                                    info!(
                                        "Reorg to LMD-GHOST head {}  rolled_back={}  applied={}",
                                        lmd_head,
                                        reorg_result.rolled_back,
                                        reorg_result.applied,
                                    );
                                    persist_registry(&store, &registry);
                                    update_rpc_snapshot(&rpc_chain_snapshot, &chain, &registry, &mempool, &consensus);
                                } else {
                                    warn!(
                                        "LMD-GHOST head {lmd_head} diverges from tip {tip_hash} — reorg not possible yet"
                                    );
                                }
                            }

                            info!(
                                "Accepted block {height} from {from}  hash={}  justified={}  finalized={}  lmd_head={}",
                                chain.tip().hash(),
                                consensus.finality.last_justified_epoch(),
                                consensus.finality.last_finalized_epoch(),
                                lmd_head,
                            );
                        }
                        Err(rupeecoin_blockchain::ChainError::InvalidPrevHash) => {
                            // Block is on a fork — register it so LMD-GHOST can see it.
                            let block_hash = block.hash();
                            let parent_hash = block.header.prev_hash;
                            consensus.register_fork_block(block_hash, parent_hash);
                            // Evict stale fork blocks beyond reorg depth, then cache.
                            let min_height = chain.height().saturating_sub(REORG_DEPTH as u64);
                            fork_blocks.retain(|_, b| b.header.height >= min_height);
                            if fork_blocks.len() < 128 {
                                fork_blocks.insert(block_hash, *block);
                            }
                            warn!(
                                "Fork block {height} from {from}  hash={block_hash}  parent={parent_hash}"
                            );
                        }
                        Err(e) => warn!("Rejected block {height} from {from}: {e}"),
                    }
                }

                // ── Transaction from gossipsub ────────────────────────────────────
                NetworkEvent::TransactionReceived { tx } => {
                    if let Err(e) = mempool.add(*tx, &chain.utxo_set, chain.height()) {
                        use rupeecoin_mempool::MempoolError;
                        if !matches!(e, MempoolError::Duplicate) {
                            warn!("Mempool rejected tx: {e}");
                        }
                    }
                }

                // ── Validator announcement ────────────────────────────────────────
                NetworkEvent::ValidatorAnnounced(ann) => {
                    let sig_data = ValidatorAnnouncement::sig_data(&ann.pubkey, ann.stake);
                    match ann.pubkey.verify(sig_data.as_bytes(), &ann.signature) {
                        Ok(()) => {
                            let addr = ann.pubkey.to_hash();
                            if registry.get(&addr).is_none() {
                                match registry.register(ann.pubkey.clone(), ann.stake, chain.height()) {
                                    Ok(()) => info!(
                                        "Registered validator {addr}  stake={}  active={}",
                                        ann.stake,
                                        registry.active_count()
                                    ),
                                    Err(e) => warn!("Validator registration rejected: {e}"),
                                }
                            }
                        }
                        Err(_) => warn!("Validator announcement: invalid signature"),
                    }
                }

                // ── Attestation from gossipsub ────────────────────────────────────
                NetworkEvent::AttestationReceived(att) => {
                    let slot = att.slot;
                    let bh = att.block_hash;
                    // 8.3: check for double-vote before adding to pool.
                    if let Some(evidence) = consensus.slashing_detector.record_attestation(*att.clone()) {
                        warn!("Slashable double-vote detected from gossip at slot {slot}!");
                        pending_slash_reports.push(evidence);
                    }
                    match consensus.add_attestation(*att, slot, &bh, &registry) {
                        Ok(true) => info!(
                            "Epoch {} justified  finalized={}",
                            slot_to_epoch(slot),
                            consensus.finality.last_finalized_epoch(),
                        ),
                        Ok(false) => {}
                        Err(rupeecoin_consensus::ConsensusError::DuplicateAttestation(_)) => {}
                        Err(e) => warn!("Attestation rejected: {e}"),
                    }
                }

                // ── Peer connected — request blocks we're missing ─────────────────
                NetworkEvent::PeerConnected(peer) => {
                    info!("Peer connected: {peer}");
                    let from = chain.height() + 1;
                    if !pending_sync_heights.contains(&from) {
                        pending_sync_heights.insert(from);
                        let _ = net_cmd
                            .send(NetworkCommand::RequestBlocks { peer: peer.clone(), from_height: from })
                            .await;
                    }
                    if is_validator {
                        let sd = ValidatorAnnouncement::sig_data(&kp.public_key, cfg.stake_micro_rpc);
                        let ann = ValidatorAnnouncement {
                            pubkey: kp.public_key.clone(),
                            stake: cfg.stake_micro_rpc,
                            signature: kp.sign(sd.as_bytes()),
                        };
                        let _ = net_cmd
                            .send(NetworkCommand::BroadcastValidatorAnnouncement(Box::new(ann)))
                            .await;
                    }
                }

                // ── Peer requests blocks ──────────────────────────────────────────
                NetworkEvent::SyncRequested { id, peer, from_height } => {
                    info!("Sync request from {peer}: from={from_height}");
                    let tip = chain.height();
                    let mut blocks = Vec::new();
                    let max = from_height.saturating_add(64).min(tip + 1);
                    for h in from_height..max {
                        match store.get_block_by_height(h) {
                            Ok(Some(b)) => blocks.push(b),
                            Ok(None) => break,
                            Err(e) => { error!("store read {h}: {e}"); break; }
                        }
                    }
                    let _ = net_cmd.send(NetworkCommand::SendSyncResponse { id, blocks }).await;
                }

                // ── Sync response ─────────────────────────────────────────────────
                NetworkEvent::SyncBlocks(blocks) => {
                    info!("Sync: received {} blocks", blocks.len());
                    for block in blocks {
                        let height = block.header.height;
                        pending_sync_heights.remove(&height);
                        match chain.add_block(block.clone(), &mut registry) {
                            Ok(()) => {
                                if let Err(e) = store.apply_block(&block) {
                                    error!("persist synced block {height}: {e}");
                                }
                                // 8.2: persist registry after sync.
                                persist_registry(&store, &registry);
                                maybe_save_checkpoint(&store, &chain);
                                update_rpc_snapshot(&rpc_chain_snapshot, &chain, &registry, &mempool, &consensus);
                                mempool.evict_conflicts(
                                    &block.transactions.iter().map(|t| t.txid()).collect::<Vec<_>>(),
                                );
                                consensus.on_block_accepted(&block, &registry);
                                info!("Synced block {height}  hash={}", block.hash());
                            }
                            Err(e) => warn!("Sync block {height} rejected: {e}"),
                        }
                    }
                }

                NetworkEvent::PeerDisconnected(peer) => info!("Peer disconnected: {peer}"),
                NetworkEvent::ListeningOn(addr) => info!("P2P listening on {addr}"),
            },

            // ── Slot timer ────────────────────────────────────────────────────────
            _ = slot_timer.tick() => {
                // Drain HTTP-submitted transparent txs into the mempool.
                for tx in pending_txs.lock().unwrap().drain(..) {
                    if let Err(e) = mempool.add(tx.clone(), &chain.utxo_set, chain.height()) {
                        use rupeecoin_mempool::MempoolError;
                        if !matches!(e, MempoolError::Duplicate) {
                            warn!("HTTP tx rejected by mempool: {e}");
                        }
                    } else {
                        let _ = net_cmd.send(rupeecoin_network::NetworkCommand::BroadcastTransaction(Box::new(tx))).await;
                    }
                }

                let next_slot = chain.height() + 1;
                let active_count = registry.active_count();

                // 8.4: only build on the LMD-GHOST canonical head; skip if we're on a fork.
                let tip_hash = chain.tip().hash();
                let lmd_head = consensus.lmd_select_head(tip_hash, &registry);
                if active_count > 0 && lmd_head != tip_hash {
                    warn!(
                        "LMD-GHOST head {lmd_head} diverges from chain tip {tip_hash} — skipping slot {next_slot}"
                    );
                    continue;
                }

                let should_propose = if active_count == 0 {
                    true
                } else {
                    let idx = select_proposer(
                        next_slot,
                        &chain.tip().header.vrf_output,
                        active_count,
                    );
                    registry.active_set().get(idx).copied() == Some(my_addr)
                };

                if !should_propose {
                    continue;
                }

                let atts: Vec<Attestation> = pending_attestations.drain(..).collect();
                // Only include transfers that pay at least the next block's base
                // fee, matching the chain's fee floor so we never build a block
                // our own validation would reject.
                let next_base_fee = rupeecoin_economics::next_base_fee(
                    chain.tip().header.base_fee,
                    chain.tip().header.gas_used,
                );
                let mut extra_txs = mempool.drain_for_block(50, next_base_fee);

                // Auto-claim accrued validator rewards.
                if is_validator {
                    if let Some(v) = registry.get(&my_addr) {
                        if v.claimable_rewards > 0 {
                            let claim = build_reward_claim(
                                my_addr,
                                v.claimable_rewards,
                                kp.public_key.to_address(),
                                &kp,
                            );
                            extra_txs.insert(0, claim);
                        }
                    }
                }

                // Include any pending slashing reports.
                for evidence in pending_slash_reports.drain(..) {
                    if let Some(slash_tx) =
                        build_slash_report(&evidence, kp.public_key.to_address(), &registry)
                    {
                        extra_txs.push(slash_tx);
                    }
                }

                let privacy_txs: Vec<PrivacyTransaction> = {
                    let drained: Vec<PrivacyTransaction> =
                        pending_privacy_txs.lock().unwrap().drain(..).collect();
                    drained.into_iter().filter(|tx| {
                        tx.validate_stateless().is_ok()
                            && tx.validate_stateful(&chain.recent_roots, &chain.spent_nullifiers).is_ok()
                    }).collect()
                };

                let block = build_block(&chain, &kp, &registry, atts, extra_txs, privacy_txs);

                let empty_sig = Signature::empty();
                let proposer_sig = block.proposer_signature.as_ref().unwrap_or(&empty_sig);
                if let Some(evidence) = consensus.check_and_record_proposal(
                    block.header.slot,
                    block.hash(),
                    &kp.public_key,
                    proposer_sig,
                ) {
                    error!("Equivocation in own proposal: {evidence:?}");
                    continue;
                }

                match chain.add_block(block.clone(), &mut registry) {
                    Ok(()) => {
                        if let Err(e) = store.apply_block(&block) {
                            error!("persist own block: {e}");
                        }
                        // 8.2: persist registry so rewards survive restart.
                        persist_registry(&store, &registry);
                        maybe_save_checkpoint(&store, &chain);
                        update_rpc_snapshot(&rpc_chain_snapshot, &chain, &registry, &mempool, &consensus);
                        consensus.on_block_accepted(&block, &registry);
                        if is_validator {
                            registry.record_proposal(&my_addr, true);
                            let self_att = attest(&kp, &block);
                            let _ = consensus.add_attestation(
                                self_att.clone(),
                                block.header.slot,
                                &block.hash(),
                                &registry,
                            );
                            let _ = net_cmd
                                .send(NetworkCommand::BroadcastAttestation(Box::new(self_att)))
                                .await;
                        }
                        info!(
                            "Produced block {}  txs={}  atts={}  vrf={}  justified={}  finalized={}",
                            chain.height(),
                            block.transactions.len(),
                            block.attestations.len(),
                            block.header.vrf_output,
                            consensus.finality.last_justified_epoch(),
                            consensus.finality.last_finalized_epoch(),
                        );
                        let _ = net_cmd
                            .send(NetworkCommand::BroadcastBlock(Box::new(block)))
                            .await;
                    }
                    Err(e) => error!("own block rejected: {e}"),
                }
            }

            // ── Periodic validator re-announcement ────────────────────────────────
            _ = announce_timer.tick() => {
                if is_validator {
                    let sd = ValidatorAnnouncement::sig_data(&kp.public_key, cfg.stake_micro_rpc);
                    let ann = ValidatorAnnouncement {
                        pubkey: kp.public_key.clone(),
                        stake: cfg.stake_micro_rpc,
                        signature: kp.sign(sd.as_bytes()),
                    };
                    let _ = net_cmd
                        .send(NetworkCommand::BroadcastValidatorAnnouncement(Box::new(ann)))
                        .await;
                }
            }

            // ── Graceful shutdown ─────────────────────────────────────────────────
            _ = &mut ctrl_c => {
                info!("Shutting down — height {}", chain.height());
                break;
            }
        }
    }

    Ok(())
}
