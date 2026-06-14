mod dandelion;

use std::collections::{HashMap, HashSet};
use std::io;
use std::time::Duration;

use async_trait::async_trait;
use futures::{prelude::*, StreamExt};
use libp2p::{
    gossipsub::{self, IdentTopic, MessageAuthenticity, ValidationMode},
    identify, mdns, noise, ping,
    request_response::{self, InboundRequestId, ProtocolSupport, ResponseChannel},
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, StreamProtocol,
};
use rand::rngs::SmallRng;
use rand::SeedableRng;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::time;
use tracing::{debug, info, trace, warn};

use rupeecoin_types::{Attestation, Block, Transaction, ValidatorAnnouncement};

use dandelion::{pick_stem_peer, DandelionRouter, DandelionStemAck, DandelionStemReq, StemCodec};

// ── Gossipsub topics ─────────────────────────────────────────────────────────

const TOPIC_BLOCKS: &str = "rupeecoin/blocks/1";
const TOPIC_TXS: &str = "rupeecoin/txs/1";
const TOPIC_VALIDATORS: &str = "rupeecoin/validators/1";
const TOPIC_ATTESTATIONS: &str = "rupeecoin/attestations/1";

// ── Block-sync request/response ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRequest {
    pub from_height: u64,
    pub max_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResponse {
    pub blocks: Vec<Block>,
}

#[derive(Clone, Default)]
struct SyncCodec;

#[async_trait]
impl request_response::Codec for SyncCodec {
    type Protocol = StreamProtocol;
    type Request = SyncRequest;
    type Response = SyncResponse;

    async fn read_request<T>(&mut self, _: &StreamProtocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut len = [0u8; 4];
        io.read_exact(&mut len).await?;
        let mut buf = vec![0u8; u32::from_be_bytes(len) as usize];
        io.read_exact(&mut buf).await?;
        bincode::deserialize(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    async fn write_request<T>(
        &mut self,
        _: &StreamProtocol,
        io: &mut T,
        req: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let bytes =
            bincode::serialize(&req).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        io.write_all(&(bytes.len() as u32).to_be_bytes()).await?;
        io.write_all(&bytes).await
    }

    async fn read_response<T>(
        &mut self,
        _: &StreamProtocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut len = [0u8; 4];
        io.read_exact(&mut len).await?;
        let mut buf = vec![0u8; u32::from_be_bytes(len) as usize];
        io.read_exact(&mut buf).await?;
        bincode::deserialize(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    async fn write_response<T>(
        &mut self,
        _: &StreamProtocol,
        io: &mut T,
        resp: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let bytes =
            bincode::serialize(&resp).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        io.write_all(&(bytes.len() as u32).to_be_bytes()).await?;
        io.write_all(&bytes).await
    }
}

// ── Public API types ─────────────────────────────────────────────────────────

/// Opaque identifier for an inbound sync request, used to route the response.
pub type SyncRequestId = InboundRequestId;

#[derive(Debug)]
pub enum NetworkEvent {
    BlockReceived {
        block: Box<Block>,
        from: PeerId,
    },
    TransactionReceived {
        tx: Box<Transaction>,
    },
    ValidatorAnnounced(Box<ValidatorAnnouncement>),
    AttestationReceived(Box<Attestation>),
    PeerConnected(PeerId),
    PeerDisconnected(PeerId),
    ListeningOn(Multiaddr),
    /// A peer is requesting blocks — node must respond with `SendSyncResponse`.
    SyncRequested {
        id: SyncRequestId,
        peer: PeerId,
        from_height: u64,
    },
    /// Blocks received in response to our sync request.
    SyncBlocks(Vec<Block>),
}

pub enum NetworkCommand {
    BroadcastBlock(Box<Block>),
    BroadcastTransaction(Box<Transaction>),
    BroadcastValidatorAnnouncement(Box<ValidatorAnnouncement>),
    BroadcastAttestation(Box<Attestation>),
    Dial(Multiaddr),
    /// Node hands back blocks to fulfil a pending sync request.
    SendSyncResponse {
        id: SyncRequestId,
        blocks: Vec<Block>,
    },
    /// Ask a specific peer for blocks starting at `from_height`.
    RequestBlocks {
        peer: PeerId,
        from_height: u64,
    },
}

// ── libp2p behaviour ─────────────────────────────────────────────────────────

#[derive(NetworkBehaviour)]
struct RupeeCoinBehaviour {
    gossipsub: gossipsub::Behaviour,
    identify: identify::Behaviour,
    mdns: mdns::tokio::Behaviour,
    ping: ping::Behaviour,
    block_sync: request_response::Behaviour<SyncCodec>,
    /// Dandelion++ single-peer stem forwarding.
    dandelion_stem: request_response::Behaviour<StemCodec>,
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn start(
    listen_addr: Multiaddr,
) -> anyhow::Result<(mpsc::Sender<NetworkCommand>, mpsc::Receiver<NetworkEvent>)> {
    let (cmd_tx, cmd_rx) = mpsc::channel::<NetworkCommand>(64);
    let (event_tx, event_rx) = mpsc::channel::<NetworkEvent>(256);

    let block_topic = IdentTopic::new(TOPIC_BLOCKS);
    let tx_topic = IdentTopic::new(TOPIC_TXS);
    let validator_topic = IdentTopic::new(TOPIC_VALIDATORS);
    let attestation_topic = IdentTopic::new(TOPIC_ATTESTATIONS);

    let bt = block_topic.clone();
    let tt = tx_topic.clone();
    let vt = validator_topic.clone();
    let at = attestation_topic.clone();

    let sync_proto = StreamProtocol::new("/rupeecoin/sync/1");
    let stem_proto = StreamProtocol::new("/rupeecoin/dandelion/1");

    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|key| {
            let gs_cfg = gossipsub::ConfigBuilder::default()
                .heartbeat_interval(Duration::from_millis(700))
                .validation_mode(ValidationMode::Strict)
                .flood_publish(true)
                .build()
                .expect("valid gossipsub config");
            let gossipsub =
                gossipsub::Behaviour::new(MessageAuthenticity::Signed(key.clone()), gs_cfg)?;

            let identify = identify::Behaviour::new(identify::Config::new(
                "/rupeecoin/1.0.0".to_string(),
                key.public(),
            ));

            let mdns =
                mdns::tokio::Behaviour::new(mdns::Config::default(), key.public().to_peer_id())?;

            let ping = ping::Behaviour::new(ping::Config::new());

            let block_sync = request_response::Behaviour::new(
                vec![(sync_proto.clone(), ProtocolSupport::Full)],
                request_response::Config::default(),
            );

            let dandelion_stem = request_response::Behaviour::new(
                vec![(stem_proto.clone(), ProtocolSupport::Full)],
                request_response::Config::default(),
            );

            Ok(RupeeCoinBehaviour {
                gossipsub,
                identify,
                mdns,
                ping,
                block_sync,
                dandelion_stem,
            })
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    swarm.behaviour_mut().gossipsub.subscribe(&block_topic)?;
    swarm.behaviour_mut().gossipsub.subscribe(&tx_topic)?;
    swarm
        .behaviour_mut()
        .gossipsub
        .subscribe(&validator_topic)?;
    swarm
        .behaviour_mut()
        .gossipsub
        .subscribe(&attestation_topic)?;
    swarm.listen_on(listen_addr)?;

    tokio::spawn(run(swarm, cmd_rx, event_tx, bt, tt, vt, at));

    Ok((cmd_tx, event_rx))
}

// ── Swarm event loop ──────────────────────────────────────────────────────────

async fn run(
    mut swarm: libp2p::Swarm<RupeeCoinBehaviour>,
    mut cmd_rx: mpsc::Receiver<NetworkCommand>,
    event_tx: mpsc::Sender<NetworkEvent>,
    block_topic: IdentTopic,
    tx_topic: IdentTopic,
    validator_topic: IdentTopic,
    attestation_topic: IdentTopic,
) {
    let mut pending_sync: HashMap<SyncRequestId, ResponseChannel<SyncResponse>> = HashMap::new();
    let mut connected_peers: HashSet<PeerId> = HashSet::new();
    let mut dandelion = DandelionRouter::new();
    let mut rng = SmallRng::from_entropy();
    // Tick every second to flush stem transactions whose timeout has elapsed.
    let mut stem_timer = time::interval(Duration::from_secs(1));

    loop {
        tokio::select! {
            Some(event) = swarm.next() => {
                on_event(
                    &mut swarm, event, &event_tx,
                    &block_topic, &tx_topic, &validator_topic, &attestation_topic,
                    &mut pending_sync,
                    &mut connected_peers,
                    &mut dandelion,
                    &mut rng,
                ).await;
            }
            Some(cmd) = cmd_rx.recv() => {
                on_cmd(
                    &mut swarm, cmd,
                    &block_topic, &tx_topic, &validator_topic, &attestation_topic,
                    &mut pending_sync,
                    &connected_peers,
                    &mut dandelion,
                    &mut rng,
                );
            }
            _ = stem_timer.tick() => {
                // Flush any stem transactions that have not been seen via
                // gossipsub within the timeout window.
                let timed_out = dandelion.drain_timed_out();
                for (txid, tx_bytes) in timed_out {
                    warn!("dandelion: stem timeout for {txid:?}, fluffing");
                    if let Err(e) = swarm
                        .behaviour_mut()
                        .gossipsub
                        .publish(tx_topic.clone(), tx_bytes)
                    {
                        warn!("dandelion timeout fluff: {e}");
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn on_event(
    swarm: &mut libp2p::Swarm<RupeeCoinBehaviour>,
    event: SwarmEvent<RupeeCoinBehaviourEvent>,
    event_tx: &mpsc::Sender<NetworkEvent>,
    block_topic: &IdentTopic,
    tx_topic: &IdentTopic,
    validator_topic: &IdentTopic,
    attestation_topic: &IdentTopic,
    pending_sync: &mut HashMap<SyncRequestId, ResponseChannel<SyncResponse>>,
    connected_peers: &mut HashSet<PeerId>,
    dandelion: &mut DandelionRouter,
    rng: &mut SmallRng,
) {
    match event {
        // ── Gossipsub ──────────────────────────────────────────────────────────
        SwarmEvent::Behaviour(RupeeCoinBehaviourEvent::Gossipsub(gossipsub::Event::Message {
            message,
            ..
        })) => {
            if message.topic == block_topic.hash() {
                match bincode::deserialize::<Block>(&message.data) {
                    Ok(block) => {
                        let from = message.source.unwrap_or_else(PeerId::random);
                        debug!(
                            "gossipsub: block height {} from {from}",
                            block.header.height
                        );
                        let _ = event_tx
                            .send(NetworkEvent::BlockReceived {
                                block: Box::new(block),
                                from,
                            })
                            .await;
                    }
                    Err(e) => warn!("gossipsub: bad block: {e}"),
                }
            } else if message.topic == tx_topic.hash() {
                match bincode::deserialize::<Transaction>(&message.data) {
                    Ok(tx) => {
                        // If we were tracking this tx in stem phase (we are the
                        // originator or an intermediate node), remove it — a
                        // downstream peer has already fluffed it.
                        dandelion.remove(&tx.txid());
                        let _ = event_tx
                            .send(NetworkEvent::TransactionReceived { tx: Box::new(tx) })
                            .await;
                    }
                    Err(e) => warn!("gossipsub: bad tx: {e}"),
                }
            } else if message.topic == validator_topic.hash() {
                match bincode::deserialize::<ValidatorAnnouncement>(&message.data) {
                    Ok(ann) => {
                        let _ = event_tx
                            .send(NetworkEvent::ValidatorAnnounced(Box::new(ann)))
                            .await;
                    }
                    Err(e) => warn!("gossipsub: bad validator announcement: {e}"),
                }
            } else if message.topic == attestation_topic.hash() {
                match bincode::deserialize::<Attestation>(&message.data) {
                    Ok(att) => {
                        let _ = event_tx
                            .send(NetworkEvent::AttestationReceived(Box::new(att)))
                            .await;
                    }
                    Err(e) => warn!("gossipsub: bad attestation: {e}"),
                }
            }
        }

        // ── Dandelion++ stem: inbound request ─────────────────────────────────
        SwarmEvent::Behaviour(RupeeCoinBehaviourEvent::DandelionStem(
            request_response::Event::Message {
                peer: sender,
                message:
                    request_response::Message::Request {
                        request_id: _,
                        request: DandelionStemReq { tx_bytes, remaining_hops },
                        channel,
                    },
            },
        )) => {
            // Acknowledge immediately to release the sender from the RPC call.
            let _ = swarm
                .behaviour_mut()
                .dandelion_stem
                .send_response(channel, DandelionStemAck);

            match bincode::deserialize::<Transaction>(&tx_bytes) {
                Ok(tx) => {
                    let txid = tx.txid();
                    debug!(
                        "dandelion: received stem tx {txid} remaining_hops={remaining_hops} from {sender}"
                    );

                    // Surface the transaction to the application layer.
                    let _ = event_tx
                        .send(NetworkEvent::TransactionReceived { tx: Box::new(tx) })
                        .await;

                    // Track for timeout fallback at this node.
                    dandelion.enter_stem(txid, tx_bytes.clone());

                    if remaining_hops > 0 {
                        // Continue the stem: forward to one random non-sender peer.
                        if let Some(next_peer) =
                            pick_stem_peer(rng, connected_peers, Some(&sender))
                        {
                            swarm.behaviour_mut().dandelion_stem.send_request(
                                &next_peer,
                                DandelionStemReq {
                                    tx_bytes,
                                    remaining_hops: remaining_hops - 1,
                                },
                            );
                            debug!(
                                "dandelion: relayed stem {txid} hops_left={} → {next_peer}",
                                remaining_hops - 1
                            );
                        } else {
                            // No eligible forward peer — fluff immediately.
                            dandelion.remove(&txid);
                            if let Err(e) = swarm
                                .behaviour_mut()
                                .gossipsub
                                .publish(tx_topic.clone(), tx_bytes)
                            {
                                warn!("dandelion fluff (no peer): {e}");
                            }
                        }
                    } else {
                        // Hops exhausted — transition to fluff phase.
                        dandelion.remove(&txid);
                        if let Err(e) = swarm
                            .behaviour_mut()
                            .gossipsub
                            .publish(tx_topic.clone(), tx_bytes)
                        {
                            warn!("dandelion fluff (hops=0): {e}");
                        }
                    }
                }
                Err(e) => warn!("dandelion: invalid tx bytes in stem: {e}"),
            }
        }

        // ── Block-sync: inbound request ────────────────────────────────────────
        SwarmEvent::Behaviour(RupeeCoinBehaviourEvent::BlockSync(
            request_response::Event::Message {
                peer,
                message:
                    request_response::Message::Request {
                        request_id,
                        request,
                        channel,
                    },
            },
        )) => {
            debug!("sync request from {peer}: from={}", request.from_height);
            pending_sync.insert(request_id, channel);
            let _ = event_tx
                .send(NetworkEvent::SyncRequested {
                    id: request_id,
                    peer,
                    from_height: request.from_height,
                })
                .await;
        }

        // ── Block-sync: inbound response ───────────────────────────────────────
        SwarmEvent::Behaviour(RupeeCoinBehaviourEvent::BlockSync(
            request_response::Event::Message {
                message: request_response::Message::Response { response, .. },
                ..
            },
        )) => {
            let count = response.blocks.len();
            debug!("sync response: {count} blocks");
            if !response.blocks.is_empty() {
                let _ = event_tx
                    .send(NetworkEvent::SyncBlocks(response.blocks))
                    .await;
            }
        }

        // ── mDNS ──────────────────────────────────────────────────────────────
        SwarmEvent::Behaviour(RupeeCoinBehaviourEvent::Mdns(mdns::Event::Discovered(peers))) => {
            for (peer_id, addr) in &peers {
                info!("mDNS: discovered {peer_id} at {addr}");
                swarm.behaviour_mut().gossipsub.add_explicit_peer(peer_id);
            }
        }
        SwarmEvent::Behaviour(RupeeCoinBehaviourEvent::Mdns(mdns::Event::Expired(peers))) => {
            for (peer_id, _) in &peers {
                swarm
                    .behaviour_mut()
                    .gossipsub
                    .remove_explicit_peer(peer_id);
            }
        }

        // ── Connection lifecycle ───────────────────────────────────────────────
        SwarmEvent::NewListenAddr { address, .. } => {
            info!("Listening on {address}");
            let _ = event_tx.send(NetworkEvent::ListeningOn(address)).await;
        }
        SwarmEvent::ConnectionEstablished { peer_id, .. } => {
            info!("Connected to {peer_id}");
            connected_peers.insert(peer_id);
            let _ = event_tx.send(NetworkEvent::PeerConnected(peer_id)).await;
        }
        SwarmEvent::ConnectionClosed { peer_id, .. } => {
            debug!("Disconnected from {peer_id}");
            connected_peers.remove(&peer_id);
            let _ = event_tx.send(NetworkEvent::PeerDisconnected(peer_id)).await;
        }

        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn on_cmd(
    swarm: &mut libp2p::Swarm<RupeeCoinBehaviour>,
    cmd: NetworkCommand,
    block_topic: &IdentTopic,
    tx_topic: &IdentTopic,
    validator_topic: &IdentTopic,
    attestation_topic: &IdentTopic,
    pending_sync: &mut HashMap<SyncRequestId, ResponseChannel<SyncResponse>>,
    connected_peers: &HashSet<PeerId>,
    dandelion: &mut DandelionRouter,
    rng: &mut SmallRng,
) {
    match cmd {
        NetworkCommand::BroadcastBlock(block) => {
            if let Ok(data) = bincode::serialize(&*block) {
                match swarm.behaviour_mut().gossipsub.publish(block_topic.clone(), data) {
                    Ok(_) => {}
                    Err(gossipsub::PublishError::InsufficientPeers) => trace!("gossipsub publish block: no peers yet"),
                    Err(e) => warn!("gossipsub publish block: {e}"),
                }
            }
        }
        NetworkCommand::BroadcastTransaction(tx) => {
            let txid = tx.txid();
            if let Ok(data) = bincode::serialize(&*tx) {
                // Apply Dandelion++ stem/fluff routing.
                if let Some(peer) = pick_stem_peer(rng, connected_peers, None) {
                    let hops = DandelionRouter::initial_hops(rng);
                    dandelion.enter_stem(txid, data.clone());
                    swarm.behaviour_mut().dandelion_stem.send_request(
                        &peer,
                        DandelionStemReq {
                            tx_bytes: data,
                            remaining_hops: hops,
                        },
                    );
                    debug!("dandelion: originated stem {txid} hops={hops} → {peer}");
                } else {
                    // No connected peers — fall back to immediate gossipsub broadcast.
                    match swarm.behaviour_mut().gossipsub.publish(tx_topic.clone(), data) {
                        Ok(_) => {}
                        Err(gossipsub::PublishError::InsufficientPeers) => trace!("gossipsub publish tx: no peers yet"),
                        Err(e) => warn!("gossipsub publish tx: {e}"),
                    }
                }
            }
        }
        NetworkCommand::BroadcastValidatorAnnouncement(ann) => {
            if let Ok(data) = bincode::serialize(&*ann) {
                match swarm.behaviour_mut().gossipsub.publish(validator_topic.clone(), data) {
                    Ok(_) => {}
                    Err(gossipsub::PublishError::InsufficientPeers) => trace!("gossipsub publish validator announcement: no peers yet"),
                    Err(e) => warn!("gossipsub publish validator announcement: {e}"),
                }
            }
        }
        NetworkCommand::BroadcastAttestation(att) => {
            if let Ok(data) = bincode::serialize(&*att) {
                match swarm.behaviour_mut().gossipsub.publish(attestation_topic.clone(), data) {
                    Ok(_) => {}
                    Err(gossipsub::PublishError::InsufficientPeers) => trace!("gossipsub publish attestation: no peers yet"),
                    Err(e) => warn!("gossipsub publish attestation: {e}"),
                }
            }
        }
        NetworkCommand::Dial(addr) => {
            info!("Dialing {addr}");
            if let Err(e) = swarm.dial(addr) {
                warn!("dial: {e}");
            }
        }
        NetworkCommand::SendSyncResponse { id, blocks } => {
            if let Some(channel) = pending_sync.remove(&id) {
                if swarm
                    .behaviour_mut()
                    .block_sync
                    .send_response(channel, SyncResponse { blocks })
                    .is_err()
                {
                    warn!("sync response: channel closed");
                }
            }
        }
        NetworkCommand::RequestBlocks { peer, from_height } => {
            debug!("Requesting blocks from {peer} at height {from_height}");
            swarm.behaviour_mut().block_sync.send_request(
                &peer,
                SyncRequest {
                    from_height,
                    max_count: 64,
                },
            );
        }
    }
}
