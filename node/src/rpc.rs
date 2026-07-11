/// HTTP JSON-RPC server for the wallet and external tooling.
///
/// Runs in a separate tokio task on `rpc_port` (default 9944).
/// All state is read-only except `POST /rpc/privacy_tx`.
use std::sync::{Arc, Mutex};

use anyhow::Context as _;
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use std::collections::HashMap;

use rupeecoin_crypto::{Address, Hash};
use rupeecoin_privacy::{CommitmentTree, Nullifier, PrivacyTransaction, SpendEvidence};
use rupeecoin_storage::ChainStore;
use rupeecoin_types::Transaction;

// ── Chain snapshot (kept in sync by main loop) ────────────────────────────────

/// Per-validator info snapshot for the dashboard and /rpc/validator_info endpoint.
#[derive(Clone, Serialize)]
pub struct ValidatorSnapshot {
    pub stake: u64,
    pub claimable_rewards: u64,
    /// attestations_timely / attestations_assigned
    pub uptime_score: f64,
    /// slots_timely / slots_assigned
    pub proposal_score: f64,
    /// weighted composite score
    pub total_score: f64,
    pub is_active: bool,
    pub status: String,
}

/// Chain state snapshot updated after each block and passed to RPC handlers.
pub struct RpcChainSnapshot {
    pub height: u64,
    pub slot: u64,
    pub epoch: u64,
    pub last_justified_epoch: u64,
    pub last_finalized_epoch: u64,
    pub active_validators: usize,
    pub total_stake: u64,
    pub mempool_size: usize,
    pub base_fee: u64,
    pub recent_roots: Vec<Hash>,
    pub commitment_tree: CommitmentTree,
    /// address_hex → next expected nonce (number of txs sent from that address)
    pub nonces: HashMap<String, u64>,
    /// address_hex → rich validator snapshot (dashboard + /rpc/validator_info)
    pub validators: HashMap<String, ValidatorSnapshot>,
}

// ── Shared state ──────────────────────────────────────────────────────────────

pub struct RpcState {
    pub chain_snapshot: Arc<Mutex<RpcChainSnapshot>>,
    pub store: Arc<ChainStore>,
    /// Chain ID used to reject transactions from foreign networks.
    pub chain_id: u32,
    /// Pending privacy transactions submitted by wallets.
    pub pending_privacy_txs: Arc<Mutex<Vec<PrivacyTransaction>>>,
    /// Pending transparent transactions submitted via HTTP (JS SDK / explorer).
    pub pending_txs: Arc<Mutex<Vec<Transaction>>>,
}

// ── Error helper ──────────────────────────────────────────────────────────────

struct RpcError(String);

impl IntoResponse for RpcError {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, Json(json!({"error": self.0}))).into_response()
    }
}

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct HeightResp {
    height: u64,
}

#[derive(Serialize)]
struct RecentRootsResp {
    roots: Vec<String>,
    commitment_count: u64,
}

#[derive(Serialize)]
struct ShieldedOutputItem {
    index: u64,
    commitment_hex: String,
    encrypted_note_bytes: String, // hex-encoded bincode(EncryptedNote)
}

#[derive(Serialize)]
struct MerklePathResp {
    /// Each element: (sibling_hex, is_right_child).
    path: Vec<(String, bool)>,
    anchor: String,
}

#[derive(Serialize)]
struct NullifierResp {
    spent: bool,
}

// ── Handlers ─────────────────────────────────────────────────────────────────

async fn get_height(State(state): State<Arc<RpcState>>) -> impl IntoResponse {
    let height = state.chain_snapshot.lock().unwrap().height;
    Json(HeightResp { height })
}

async fn get_recent_roots(State(state): State<Arc<RpcState>>) -> impl IntoResponse {
    let snap = state.chain_snapshot.lock().unwrap();
    let roots: Vec<String> = snap
        .recent_roots
        .iter()
        .map(|h| hex::encode(h.as_bytes()))
        .collect();
    let commitment_count = snap.commitment_tree.len() as u64;
    Json(RecentRootsResp {
        roots,
        commitment_count,
    })
}

#[derive(Deserialize)]
struct PaginationParams {
    #[serde(default)]
    start: u64,
    #[serde(default = "default_count")]
    count: u64,
}

fn default_count() -> u64 {
    100
}

async fn get_shielded_outputs(
    State(state): State<Arc<RpcState>>,
    Query(params): Query<PaginationParams>,
) -> Result<impl IntoResponse, RpcError> {
    let count = params.count.min(1000);
    let outputs = state
        .store
        .get_shielded_outputs(params.start, count)
        .map_err(|e| RpcError(e.to_string()))?;

    let items: Vec<ShieldedOutputItem> = outputs
        .into_iter()
        .map(|(idx, out)| {
            let commitment_hex = hex::encode(out.commitment.0.as_bytes());
            let enc_bytes = bincode::serialize(&out.encrypted_note)
                .map_err(|e| RpcError(format!("serialize note: {e}")))?;
            Ok(ShieldedOutputItem {
                index: idx,
                commitment_hex,
                encrypted_note_bytes: hex::encode(&enc_bytes),
            })
        })
        .collect::<Result<Vec<_>, RpcError>>()?;

    Ok(Json(items))
}

#[derive(Deserialize)]
struct MerklePathParams {
    index: u64,
}

async fn get_merkle_path(
    State(state): State<Arc<RpcState>>,
    Query(params): Query<MerklePathParams>,
) -> Result<impl IntoResponse, RpcError> {
    let snap = state.chain_snapshot.lock().unwrap();
    let mp = snap
        .commitment_tree
        .auth_path(params.index as usize)
        .ok_or_else(|| RpcError(format!("index {} out of range", params.index)))?;

    let anchor = snap.commitment_tree.root();
    let path: Vec<(String, bool)> = mp
        .path
        .iter()
        .map(|(h, r)| (hex::encode(h.as_bytes()), *r))
        .collect();

    Ok(Json(MerklePathResp {
        path,
        anchor: hex::encode(anchor.as_bytes()),
    }))
}

#[derive(Deserialize)]
struct NullifierParams {
    hex: String,
}

/// Check whether a nullifier has already been spent.
async fn get_nullifier_status(
    State(state): State<Arc<RpcState>>,
    Query(params): Query<NullifierParams>,
) -> Result<impl IntoResponse, RpcError> {
    let raw = hex::decode(&params.hex).map_err(|e| RpcError(format!("hex decode: {e}")))?;
    let arr: [u8; 32] = raw
        .try_into()
        .map_err(|_| RpcError("nullifier must be 32 bytes".into()))?;
    let nullifier = Nullifier(Hash::from_bytes(arr));
    let spent = state
        .store
        .is_nullifier_spent(&nullifier)
        .map_err(|e| RpcError(e.to_string()))?;
    Ok(Json(NullifierResp { spent }))
}

/// Submit a privacy transaction.  Body is hex-encoded bincode of `PrivacyTransaction`.
async fn submit_privacy_tx(
    State(state): State<Arc<RpcState>>,
    body: Bytes,
) -> Result<impl IntoResponse, RpcError> {
    let raw = hex::decode(body.as_ref()).map_err(|e| RpcError(format!("hex decode: {e}")))?;
    let tx: PrivacyTransaction =
        bincode::deserialize(&raw).map_err(|e| RpcError(format!("bincode decode: {e}")))?;

    tx.validate_stateless()
        .map_err(|e| RpcError(e.to_string()))?;

    let snap = state.chain_snapshot.lock().unwrap();
    // Build the spent-nullifier set from persistent storage so we reject
    // already-spent nullifiers rather than giving a false "accepted" response.
    let mut spent = std::collections::HashSet::new();
    for input in &tx.shielded_inputs {
        if state
            .store
            .is_nullifier_spent(&input.nullifier)
            .unwrap_or(false)
        {
            spent.insert(input.nullifier);
        }
    }
    tx.validate_stateful(&snap.recent_roots, &spent)
        .map_err(|e| RpcError(e.to_string()))?;
    drop(snap);

    // Verify individual per-input STARK spend proofs.
    for input in &tx.shielded_inputs {
        if let SpendEvidence::Stark(sp) = &input.spend_evidence {
            let zk = rupeecoin_zkproof::StarkSpendProof {
                proof_bytes: sp.proof_bytes.clone(),
                anchor_felts: sp.anchor_felts,
            };
            rupeecoin_zkproof::verify_spend(&zk, sp.nullifier_felts)
                .map_err(|e| RpcError(format!("STARK proof invalid: {e}")))?;
        }
    }

    // Verify the transaction-level batch STARK proof (Phase 11.2).
    //
    // The batch proof carries its own nullifier felts (public inputs pinned at
    // row 7 of each 256-row block).  The STARK verifier checks the proof is
    // consistent with exactly those values, so a wallet that tampers with
    // nullifier_felts receives a verification failure.
    if let Some(batch_proof) = &tx.batch_stark_proof {
        let batched_count = tx
            .shielded_inputs
            .iter()
            .filter(|i| matches!(i.spend_evidence, SpendEvidence::BatchedStark))
            .count();

        if batched_count != batch_proof.num_spends as usize {
            return Err(RpcError(format!(
                "batch STARK proof covers {} inputs but tx has {} BatchedStark inputs",
                batch_proof.num_spends, batched_count
            )));
        }

        rupeecoin_zkproof::verify_spend_batch(batch_proof)
            .map_err(|e| RpcError(format!("batch STARK proof invalid: {e}")))?;
    }

    state.pending_privacy_txs.lock().unwrap().push(tx);
    Ok(Json(json!({"status": "accepted"})))
}

// ── Validator info ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ValidatorInfoParams {
    addr: String,
}

#[derive(Serialize)]
struct ValidatorInfoResp {
    claimable_rewards: u64,
    stake: u64,
    uptime_score: f64,
    proposal_score: f64,
    total_score: f64,
    is_active: bool,
    status: String,
}

async fn get_validator_info(
    State(state): State<Arc<RpcState>>,
    Query(params): Query<ValidatorInfoParams>,
) -> impl IntoResponse {
    let snap = state.chain_snapshot.lock().unwrap();
    match snap.validators.get(&params.addr) {
        Some(v) => Json(ValidatorInfoResp {
            claimable_rewards: v.claimable_rewards,
            stake: v.stake,
            uptime_score: v.uptime_score,
            proposal_score: v.proposal_score,
            total_score: v.total_score,
            is_active: v.is_active,
            status: v.status.clone(),
        })
        .into_response(),
        None => Json(ValidatorInfoResp {
            claimable_rewards: 0,
            stake: 0,
            uptime_score: 0.0,
            proposal_score: 0.0,
            total_score: 0.0,
            is_active: false,
            status: "Unknown".to_string(),
        })
        .into_response(),
    }
}

// ── Node status (dashboard) ───────────────────────────────────────────────────

#[derive(Serialize)]
struct StatusResp {
    height: u64,
    slot: u64,
    epoch: u64,
    last_justified_epoch: u64,
    last_finalized_epoch: u64,
    active_validators: usize,
    total_stake: u64,
    mempool_size: usize,
    pending_privacy_txs: usize,
    commitment_count: u64,
    base_fee: u64,
}

async fn get_status(State(state): State<Arc<RpcState>>) -> impl IntoResponse {
    let snap = state.chain_snapshot.lock().unwrap();
    let pending_privacy_txs = state.pending_privacy_txs.lock().unwrap().len();
    let commitment_count = snap.commitment_tree.len() as u64;
    Json(StatusResp {
        height: snap.height,
        slot: snap.slot,
        epoch: snap.epoch,
        last_justified_epoch: snap.last_justified_epoch,
        last_finalized_epoch: snap.last_finalized_epoch,
        active_validators: snap.active_validators,
        total_stake: snap.total_stake,
        mempool_size: snap.mempool_size,
        pending_privacy_txs,
        commitment_count,
        base_fee: snap.base_fee,
    })
}

// ── Transparent tx submission (JS SDK / explorer) ─────────────────────────────

/// Accept a hex-encoded bincode-serialised `Transaction` and queue it for the
/// mempool.  The main loop drains `pending_txs` every slot.
async fn submit_tx(
    State(state): State<Arc<RpcState>>,
    body: Bytes,
) -> Result<impl IntoResponse, RpcError> {
    let raw = hex::decode(body.as_ref()).map_err(|e| RpcError(format!("hex decode: {e}")))?;
    let tx: Transaction =
        bincode::deserialize(&raw).map_err(|e| RpcError(format!("bincode decode: {e}")))?;
    if tx.inputs.is_empty() {
        return Err(RpcError("transaction has no inputs".into()));
    }
    if tx.chain_id != state.chain_id {
        return Err(RpcError(format!(
            "wrong chain_id: got {}, expected {}",
            tx.chain_id, state.chain_id
        )));
    }
    let mut q = state.pending_txs.lock().unwrap();
    if q.len() >= 4096 {
        return Err(RpcError("mempool full".into()));
    }
    q.push(tx);
    Ok(Json(json!({"status": "accepted"})))
}

// ── Per-address nonce (transparent transaction replay protection) ─────────────

#[derive(Deserialize)]
struct NonceParams {
    addr: String,
}

#[derive(Serialize)]
struct NonceResp {
    nonce: u64,
}

/// Return the next expected nonce for a transparent address.
/// nonce = number of transactions the address has already sent.
/// A fresh address always starts at 0.
async fn get_nonce(
    State(state): State<Arc<RpcState>>,
    Query(params): Query<NonceParams>,
) -> Result<impl IntoResponse, RpcError> {
    let addr_bytes =
        if params.addr.len() == 40 && params.addr.chars().all(|c| c.is_ascii_hexdigit()) {
            hex::decode(&params.addr).map_err(|e| RpcError(format!("hex decode: {e}")))?
        } else {
            let addr = rupeecoin_crypto::decode_address(&params.addr)
                .map_err(|e| RpcError(format!("invalid address: {e}")))?;
            addr.0.to_vec()
        };
    let addr_hex = hex::encode(&addr_bytes);
    let nonce = state
        .chain_snapshot
        .lock()
        .unwrap()
        .nonces
        .get(&addr_hex)
        .copied()
        .unwrap_or(0);
    Ok(Json(NonceResp { nonce }))
}

// ── UTXO scan by address (for JS SDK wallet UI) ───────────────────────────────

#[derive(Deserialize)]
struct UtxoParams {
    addr: String,
}

#[derive(Serialize)]
struct UtxoEntry {
    txid: String,
    vout: u32,
    amount_micro: u64,
    /// Whole-coin value as a float (amount_micro / 100_000_000). Consumed by the
    /// wallet CLI and SDK client.
    amount_rpc: f64,
    /// Integer whole-coin part (amount_micro / 100_000_000).
    amount_rpc_whole: u64,
    /// Integer fractional part (amount_micro % 100_000_000), 8-digit precision.
    amount_rpc_frac: u64,
}

async fn get_utxos(
    State(state): State<Arc<RpcState>>,
    Query(params): Query<UtxoParams>,
) -> Result<impl IntoResponse, RpcError> {
    // Accept both hex (40 chars) and Base58Check.
    let addr = if params.addr.len() == 40 && params.addr.chars().all(|c| c.is_ascii_hexdigit()) {
        let bytes =
            hex::decode(&params.addr).map_err(|_| RpcError("invalid hex address".into()))?;
        let mut arr = [0u8; 20];
        arr.copy_from_slice(&bytes);
        Address(arr)
    } else {
        rupeecoin_crypto::decode_address(&params.addr)
            .map_err(|e| RpcError(format!("invalid address: {e}")))?
    };

    let utxos = state
        .store
        .scan_utxos_for_address(&addr)
        .map_err(|e| RpcError(e.to_string()))?;

    let entries: Vec<UtxoEntry> = utxos
        .into_iter()
        .map(|(op, out)| UtxoEntry {
            txid: hex::encode(op.txid.as_bytes()),
            vout: op.vout,
            amount_micro: out.amount,
            amount_rpc: out.amount as f64 / 100_000_000.0,
            amount_rpc_whole: out.amount / 100_000_000,
            amount_rpc_frac: out.amount % 100_000_000,
        })
        .collect();

    Ok(Json(entries))
}

// ── Router ────────────────────────────────────────────────────────────────────

pub fn router(state: Arc<RpcState>) -> Router {
    Router::new()
        .route("/rpc/height", get(get_height))
        .route("/rpc/recent_roots", get(get_recent_roots))
        .route("/rpc/shielded_outputs", get(get_shielded_outputs))
        .route("/rpc/merkle_path", get(get_merkle_path))
        .route("/rpc/nullifier_status", get(get_nullifier_status))
        .route("/rpc/privacy_tx", post(submit_privacy_tx))
        .route("/rpc/tx", post(submit_tx))
        .route("/rpc/utxos", get(get_utxos))
        .route("/rpc/nonce", get(get_nonce))
        .route("/rpc/validator_info", get(get_validator_info))
        .route("/rpc/status", get(get_status))
        .layer(DefaultBodyLimit::max(512 * 1024))
        .with_state(state)
}

pub async fn serve(state: Arc<RpcState>, port: u16) -> anyhow::Result<()> {
    let app = router(state);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    tracing::info!("RPC listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding RPC port {port}"))?;
    axum::serve(listener, app).await.context("axum serve")?;
    Ok(())
}
