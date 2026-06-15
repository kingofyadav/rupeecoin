mod rpc;
mod tor;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use libp2p::Multiaddr;
use tokio::time;
use tracing::{error, info, warn};

use rupeecoin_blockchain::{build_block, build_reward_claim, build_slash_report, genesis_block, Chain};
use rupeecoin_consensus::{
    select_proposer, slot_to_epoch, CheckpointManager, ConsensusState, WeakSubjectivityCheckpoint,
    SLOT_DURATION_SECS, SLOTS_PER_EPOCH,
};
use rupeecoin_crypto::{Hash, KeyPair, Signature};
use rupeecoin_mempool::Mempool;
use rupeecoin_network::{NetworkCommand, NetworkEvent};
use rupeecoin_blockchain::ANCHOR_DEPTH;
use rupeecoin_privacy::{PrivacyBlockApply, PrivacyTransaction};
use rupeecoin_staking::{ValidatorRegistry, MIN_STAKE_MICRO_RPC};
use rupeecoin_storage::ChainStore;
use rupeecoin_types::{Attestation, Block, GenesisConfig, ValidatorAnnouncement, MICRO_RPC_PER_COIN};

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
                rpc_port = args.next().context("--rpc-port requires a value")?.parse()?;
            }
            "--no-rpc" => {
                rpc_port = 0;
            }
            "--checkpoint" => {
                // Format: height:block_hash_hex:state_root_hex
                let val = args.next().context("--checkpoint requires height:hash:state_root")?;
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
                let hex_val = args.next().context("--validator-seed requires a 64-char hex value")?;
                let raw = hex::decode(&hex_val).context("--validator-seed must be valid hex")?;
                validator_seed = Some(
                    raw.try_into()
                        .map_err(|_| anyhow::anyhow!("--validator-seed must be exactly 32 bytes (64 hex chars)"))?,
                );
            }
            "--tor-socks" => {
                let addr = args.next().context("--tor-socks requires host:port")?;
                tor_config = Some(
                    tor::TorConfig::parse(&addr)
                        .map_err(|e| anyhow::anyhow!("{e}"))?,
                );
            }
            "--genesis-config" => {
                let path = args.next().context("--genesis-config requires a file path")?;
                let json = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading genesis config from '{path}'"))?;
                let cfg = GenesisConfig::from_json(&json)
                    .with_context(|| format!("parsing genesis config from '{path}'"))?;
                cfg.validate().map_err(|e| anyhow::anyhow!("invalid genesis config: {e}"))?;
                genesis_config = Some(cfg);
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
                     \n\
                     Example: --stake 10000 --checkpoint 604800:<hash_hex>:<state_hex>\n\
                     Tor example: --tor-socks 127.0.0.1:9050 --peer /dns4/xyz.onion/tcp/30333"
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
    })
}

// ── Chain init ────────────────────────────────────────────────────────────────

fn load_or_create_chain(
    store: &ChainStore,
    checkpoint_mgr: &CheckpointManager,
) -> Result<Chain> {
    let mut empty = ValidatorRegistry::new();

    match store.get_tip().context("reading chain tip from store")? {
        None => {
            info!("No existing chain — creating genesis block");
            let genesis = genesis_block();
            store.apply_block(&genesis).context("storing genesis block")?;
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
                            let actual_sr = chain.utxo_set.state_root();
                            anyhow::ensure!(
                                actual_sr == expected_sr,
                                "checkpoint state_root mismatch at height {h}: \
                                 trusted {expected_sr}, computed {actual_sr}"
                            );
                        }
                    }

                    // Update privacy fields (finding #4).
                    let apply = PrivacyBlockApply::compute(&block.privacy_txs, &chain.commitment_tree, h);
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
                    chain
                        .add_block(block, &mut empty)
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
            info!("Saved weak-subjectivity checkpoint at height {h}  hash={}", cp.block_hash);
        }
    }
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "rupeecoin_node=info,rupeecoin_network=info".parse().unwrap()
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
    info!("Validator address (UTXO/transparent): {}", rupeecoin_crypto::encode_address(&my_transparent_addr));

    let store = Arc::new(store);
    let mut chain = load_or_create_chain(&store, &checkpoint_mgr)?;
    let mut mempool = Mempool::new(4096);

    // 8.2: restore persisted validator registry so claimable_rewards survive restart.
    let mut registry: ValidatorRegistry = store
        .load_raw_meta(b"validator_registry")
        .unwrap_or(None)
        .and_then(|b| bincode::deserialize(&b).ok())
        .unwrap_or_else(ValidatorRegistry::new);

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
    let pending_privacy_txs: Arc<Mutex<Vec<PrivacyTransaction>>> =
        Arc::new(Mutex::new(Vec::new()));
    let pending_txs: Arc<Mutex<Vec<rupeecoin_types::Transaction>>> =
        Arc::new(Mutex::new(Vec::new()));

    let is_validator = cfg.stake_micro_rpc >= MIN_STAKE_MICRO_RPC;

    if cfg.rpc_port > 0 {
        let rpc_state = Arc::new(rpc::RpcState {
            chain_snapshot: Arc::clone(&rpc_chain_snapshot),
            store: Arc::clone(&store),
            pending_privacy_txs: Arc::clone(&pending_privacy_txs),
            pending_txs: Arc::clone(&pending_txs),
        });
        let port = cfg.rpc_port;
        tokio::spawn(async move { rpc::serve(rpc_state, port).await });
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
    let (net_cmd, mut net_event) =
        rupeecoin_network::start(cfg.listen).context("starting network")?;

    for addr in cfg.peers {
        info!("Dialing seed peer {addr}");
        let _ = net_cmd.send(NetworkCommand::Dial(addr)).await;
    }

    if is_validator {
        registry
            .register(kp.public_key.clone(), cfg.stake_micro_rpc, chain.height())
            .expect("self-registration should not fail");
        info!(
            "Joined validator set — stake {} micro-RPC — active set: {}",
            cfg.stake_micro_rpc,
            registry.active_count()
        );
        let sig_data = ValidatorAnnouncement::sig_data(&kp.public_key, cfg.stake_micro_rpc);
        let ann = ValidatorAnnouncement {
            pubkey: kp.public_key.clone(),
            stake: cfg.stake_micro_rpc,
            signature: kp.sign(sig_data.as_bytes()),
        };
        let _ = net_cmd
            .send(NetworkCommand::BroadcastValidatorAnnouncement(Box::new(ann)))
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

                            let lmd_head = consensus.lmd_select_head(block.hash(), &registry);
                            info!(
                                "Accepted block {height} from {from}  hash={}  justified={}  finalized={}  lmd_head={}",
                                block.hash(),
                                consensus.finality.last_justified_epoch(),
                                consensus.finality.last_finalized_epoch(),
                                lmd_head,
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
                let mut extra_txs = mempool.drain_for_block(50);

                // Auto-claim accrued validator rewards.
                if is_validator {
                    if let Some(v) = registry.get(&my_addr) {
                        if v.claimable_rewards > 0 {
                            let claim = build_reward_claim(
                                my_addr,
                                v.claimable_rewards,
                                kp.public_key.to_address(),
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
