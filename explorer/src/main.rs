/// RupeeCoin Block Explorer — standalone binary crate.
///
/// Usage: rupeecoin-explorer --data-dir ./data [--port 8080]
///
/// Serves a REST API and a minimal SPA over the local RocksDB chain store.
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::get,
    Router,
};
use clap::Parser;
use rupeecoin_crypto::{decode_address, encode_address, Hash};
use rupeecoin_privacy::Nullifier;
use rupeecoin_storage::ChainStore;
use rupeecoin_types::{Block, Transaction};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::RwLock;

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "rupeecoin-explorer", about = "RupeeCoin block explorer")]
struct Args {
    /// Path to the node data directory (must contain a valid RocksDB store).
    #[arg(long, default_value = "./data")]
    data_dir: PathBuf,

    /// HTTP port to listen on.
    #[arg(long, default_value_t = 8080)]
    port: u16,

    /// IP address to bind. Defaults to 127.0.0.1 (loopback only).
    #[arg(long, default_value = "127.0.0.1")]
    bind: IpAddr,

    /// Secondary RocksDB path (scratch dir for WAL replay).
    /// When set, the explorer opens as a RocksDB secondary instance and
    /// automatically picks up new blocks from a running node on each request.
    #[arg(long)]
    secondary_path: Option<PathBuf>,
}

// ── State ─────────────────────────────────────────────────────────────────────

struct ExplorerState {
    store: ChainStore,
    /// txid (hex) → block height — built at startup, refreshed on demand.
    tx_index: RwLock<HashMap<String, u64>>,
    /// Highest block height already indexed into `tx_index`.
    last_indexed_height: AtomicU64,
}

// ── JSON helpers ──────────────────────────────────────────────────────────────

fn h(hash: &Hash) -> String {
    hex::encode(hash.as_bytes())
}

fn block_summary(block: &Block) -> Value {
    json!({
        "height":    block.header.height,
        "hash":      h(&block.hash()),
        "prev_hash": h(&block.header.prev_hash),
        "timestamp": block.header.timestamp,
        "slot":      block.header.slot,
        "proposer":  block.header.proposer_index,
        "tx_count":  block.transactions.len(),
        "privacy_tx_count": block.privacy_txs.len(),
        "base_fee":  block.header.base_fee,
        "gas_used":  block.header.gas_used,
        "merkle_root":  h(&block.header.merkle_root),
        "state_root":   h(&block.header.state_root),
        "privacy_root": h(&block.header.privacy_root),
    })
}

fn tx_json(tx: &Transaction) -> Value {
    let kind = if tx.is_coinbase() {
        "coinbase"
    } else if tx.is_reward_claim() {
        "reward_claim"
    } else if tx.is_slashing_report() {
        "slashing_report"
    } else {
        "transfer"
    };

    let inputs: Vec<Value> = tx
        .inputs
        .iter()
        .map(|i| {
            json!({
                "txid": h(&i.outpoint.txid),
                "vout": i.outpoint.vout,
                "pubkey": hex::encode(i.public_key.as_bytes()),
            })
        })
        .collect();

    let outputs: Vec<Value> = tx
        .outputs
        .iter()
        .map(|o| {
            json!({
                "amount_micro": o.amount,
                "amount_rpc":   o.amount as f64 / 1e8,
                "recipient":    encode_address(&o.recipient),
                "recipient_hex": hex::encode(o.recipient.as_bytes()),
            })
        })
        .collect();

    json!({
        "txid":    h(&tx.txid()),
        "kind":    kind,
        "version": tx.version,
        "chain_id": tx.chain_id,
        "nonce":   tx.nonce,
        "expiry":  tx.expiry,
        "inputs":  inputs,
        "outputs": outputs,
    })
}

fn full_block_json(block: &Block) -> Value {
    let txs: Vec<Value> = block.transactions.iter().map(tx_json).collect();
    let ptxs: Vec<Value> = block
        .privacy_txs
        .iter()
        .map(|pt| {
            json!({
                "hash": hex::encode(pt.hash().as_bytes()),
                "shielded_inputs": pt.shielded_inputs.len(),
                "shielded_outputs": pt.shielded_outputs.len(),
            })
        })
        .collect();

    let mut summary = block_summary(block);
    summary["transactions"] = json!(txs);
    summary["privacy_transactions"] = json!(ptxs);
    summary
}

// ── Error helper ──────────────────────────────────────────────────────────────

fn not_found(msg: &str) -> Response {
    (StatusCode::NOT_FOUND, Json(json!({"error": msg}))).into_response()
}

fn bad_req(msg: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({"error": msg}))).into_response()
}

fn internal(msg: String) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": msg})),
    )
        .into_response()
}

// ── Tx-index refresh ─────────────────────────────────────────────────────────

async fn refresh_if_needed(state: &Arc<ExplorerState>) {
    let old_tip = state.last_indexed_height.load(Ordering::Relaxed);
    state.store.try_catch_up_with_primary();
    let new_tip = match state.store.get_tip() {
        Ok(Some((_, tip))) => tip,
        _ => return,
    };
    if new_tip > old_tip {
        let mut index = state.tx_index.write().await;
        for height in (old_tip + 1)..=new_tip {
            if let Ok(Some(block)) = state.store.get_block_by_height(height) {
                for tx in &block.transactions {
                    index.insert(h(&tx.txid()), height);
                }
            }
        }
        state.last_indexed_height.store(new_tip, Ordering::Relaxed);
    }
}

// ── Handlers ─────────────────────────────────────────────────────────────────

async fn serve_spa() -> Response {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        SPA_HTML,
    )
        .into_response()
}

async fn api_height(State(s): State<Arc<ExplorerState>>) -> Response {
    refresh_if_needed(&s).await;
    match s.store.get_tip() {
        Ok(Some((hash, height))) => Json(json!({ "height": height, "hash": h(&hash) })).into_response(),
        Ok(None) => Json(json!({ "height": 0, "hash": null })).into_response(),
        Err(e) => internal(e.to_string()),
    }
}

async fn api_recent(State(s): State<Arc<ExplorerState>>) -> Response {
    refresh_if_needed(&s).await;
    match s.store.get_recent_blocks(20) {
        Ok(blocks) => Json(blocks.iter().map(block_summary).collect::<Vec<_>>()).into_response(),
        Err(e) => internal(e.to_string()),
    }
}

async fn api_block_by_height(
    State(s): State<Arc<ExplorerState>>,
    Path(height): Path<u64>,
) -> Response {
    refresh_if_needed(&s).await;
    match s.store.get_block_by_height(height) {
        Ok(Some(block)) => Json(full_block_json(&block)).into_response(),
        Ok(None) => not_found(&format!("no block at height {height}")),
        Err(e) => internal(e.to_string()),
    }
}

async fn api_block_by_hash(
    State(s): State<Arc<ExplorerState>>,
    Path(hash_hex): Path<String>,
) -> Response {
    refresh_if_needed(&s).await;
    let bytes = match hex::decode(&hash_hex) {
        Ok(b) if b.len() == 32 => b,
        _ => return bad_req("hash must be 64 hex chars"),
    };
    let arr: [u8; 32] = bytes.try_into().unwrap();
    let hash = Hash::from_bytes(arr);
    match s.store.get_block_by_hash(&hash) {
        Ok(Some(block)) => Json(full_block_json(&block)).into_response(),
        Ok(None) => not_found("block not found"),
        Err(e) => internal(e.to_string()),
    }
}

async fn api_tx(State(s): State<Arc<ExplorerState>>, Path(txid_hex): Path<String>) -> Response {
    refresh_if_needed(&s).await;
    let txid_hex = txid_hex.to_lowercase();
    let index = s.tx_index.read().await;
    let height = match index.get(&txid_hex) {
        Some(&h) => h,
        None => return not_found("transaction not found"),
    };
    drop(index);

    let block = match s.store.get_block_by_height(height) {
        Ok(Some(b)) => b,
        Ok(None) => return internal("tx index points to missing block".into()),
        Err(e) => return internal(e.to_string()),
    };

    for tx in &block.transactions {
        if h(&tx.txid()) == txid_hex {
            return Json(json!({
                "transaction": tx_json(tx),
                "block_height": height,
                "block_hash": h(&block.hash()),
            }))
            .into_response();
        }
    }
    not_found("transaction not found in block")
}

async fn api_address(
    State(s): State<Arc<ExplorerState>>,
    Path(addr_str): Path<String>,
) -> Response {
    refresh_if_needed(&s).await;
    // Accept both Base58Check and raw hex.
    let addr = if let Ok(a) = decode_address(&addr_str) {
        a
    } else if let Ok(bytes) = hex::decode(&addr_str) {
        if bytes.len() == 20 {
            let mut arr = [0u8; 20];
            arr.copy_from_slice(&bytes);
            rupeecoin_crypto::Address(arr)
        } else {
            return bad_req("address must be Base58Check or 40 hex chars");
        }
    } else {
        return bad_req("invalid address");
    };

    match s.store.scan_utxos_for_address(&addr) {
        Ok(utxos) => {
            let total: u64 = utxos.iter().map(|(_, o)| o.amount).sum();
            let utxo_list: Vec<Value> = utxos
                .iter()
                .map(|(op, out)| {
                    json!({
                        "txid": h(&op.txid),
                        "vout": op.vout,
                        "amount_micro": out.amount,
                        "amount_rpc":   out.amount as f64 / 1e8,
                    })
                })
                .collect();
            Json(json!({
                "address": encode_address(&addr),
                "address_hex": hex::encode(addr.as_bytes()),
                "balance_micro": total,
                "balance_rpc": total as f64 / 1e8,
                "utxos": utxo_list,
            }))
            .into_response()
        }
        Err(e) => internal(e.to_string()),
    }
}

async fn api_nullifier(
    State(s): State<Arc<ExplorerState>>,
    Path(hex_str): Path<String>,
) -> Response {
    let bytes = match hex::decode(&hex_str) {
        Ok(b) if b.len() == 32 => b,
        _ => return bad_req("nullifier must be 64 hex chars"),
    };
    let arr: [u8; 32] = bytes.try_into().unwrap();
    let nullifier = Nullifier(Hash::from_bytes(arr));
    match s.store.is_nullifier_spent(&nullifier) {
        Ok(spent) => Json(json!({ "nullifier": hex_str, "spent": spent })).into_response(),
        Err(e) => internal(e.to_string()),
    }
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

async fn api_search(
    State(s): State<Arc<ExplorerState>>,
    Query(params): Query<SearchQuery>,
) -> Response {
    refresh_if_needed(&s).await;
    let q = params.q.trim().to_string();

    // 1. Pure numeric → block by height
    if let Ok(height) = q.parse::<u64>() {
        return match s.store.get_block_by_height(height) {
            Ok(Some(block)) => Json(json!({
                "type": "block",
                "data": block_summary(&block),
            }))
            .into_response(),
            Ok(None) => not_found("no block at that height"),
            Err(e) => internal(e.to_string()),
        };
    }

    // 2. 64-char hex → try block hash, then txid
    if q.len() == 64 && q.chars().all(|c| c.is_ascii_hexdigit()) {
        let q_lower = q.to_lowercase();
        let bytes: [u8; 32] = hex::decode(&q_lower).unwrap().try_into().unwrap();
        let hash = Hash::from_bytes(bytes);

        if let Ok(Some(block)) = s.store.get_block_by_hash(&hash) {
            return Json(json!({ "type": "block", "data": block_summary(&block) }))
                .into_response();
        }

        let index = s.tx_index.read().await;
        if let Some(&height) = index.get(&q_lower) {
            drop(index);
            if let Ok(Some(block)) = s.store.get_block_by_height(height) {
                for tx in &block.transactions {
                    if h(&tx.txid()) == q_lower {
                        return Json(json!({
                            "type": "transaction",
                            "data": tx_json(tx),
                            "block_height": height,
                        }))
                        .into_response();
                    }
                }
            }
        }

        return not_found("no block or transaction with that hash");
    }

    // 3. Otherwise → try address
    if let Ok(addr) = decode_address(&q) {
        return match s.store.scan_utxos_for_address(&addr) {
            Ok(utxos) => {
                let total: u64 = utxos.iter().map(|(_, o)| o.amount).sum();
                Json(json!({
                    "type": "address",
                    "data": {
                        "address": encode_address(&addr),
                        "balance_rpc": total as f64 / 1e8,
                        "utxo_count": utxos.len(),
                    },
                }))
                .into_response()
            }
            Err(e) => internal(e.to_string()),
        };
    }

    bad_req("could not interpret query as height, hash, or address")
}

// ── Tx index ─────────────────────────────────────────────────────────────────

/// Scan all stored blocks and build txid_hex → block_height index.
fn build_tx_index(store: &ChainStore) -> HashMap<String, u64> {
    let mut index = HashMap::new();
    let tip_height = match store.get_tip() {
        Ok(Some((_, h))) => h,
        _ => return index,
    };
    for height in 0..=tip_height {
        if let Ok(Some(block)) = store.get_block_by_height(height) {
            for tx in &block.transactions {
                index.insert(h(&tx.txid()), height);
            }
        }
    }
    index
}

// ── Router ────────────────────────────────────────────────────────────────────

fn router(state: Arc<ExplorerState>) -> Router {
    Router::new()
        .route("/", get(serve_spa))
        .route("/api/height", get(api_height))
        .route("/api/recent", get(api_recent))
        // Static segment "hash" must come before the :height capture.
        .route("/api/block/hash/:hash", get(api_block_by_hash))
        .route("/api/block/:height", get(api_block_by_height))
        .route("/api/tx/:txid", get(api_tx))
        .route("/api/address/:addr", get(api_address))
        .route("/api/nullifier/:hex", get(api_nullifier))
        .route("/api/search", get(api_search))
        .with_state(state)
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rupeecoin_explorer=info".parse().unwrap()),
        )
        .init();

    let args = Args::parse();

    let store = if let Some(ref sec) = args.secondary_path {
        tracing::info!("opening chain store (secondary) at {} secondary={}", args.data_dir.display(), sec.display());
        ChainStore::open_secondary(&args.data_dir, sec).expect("failed to open data dir as secondary")
    } else {
        tracing::info!("opening chain store (read-only) at {}", args.data_dir.display());
        ChainStore::open_read_only(&args.data_dir).expect("failed to open data dir")
    };

    tracing::info!("building tx index…");
    let tx_index = build_tx_index(&store);
    tracing::info!("indexed {} transactions", tx_index.len());
    let indexed_tip = match store.get_tip() {
        Ok(Some((_, tip))) => tip,
        _ => 0,
    };

    let state = Arc::new(ExplorerState {
        store,
        tx_index: RwLock::new(tx_index),
        last_indexed_height: AtomicU64::new(indexed_tip),
    });

    let app = router(state);
    let addr = std::net::SocketAddr::new(args.bind, args.port);
    tracing::info!("explorer listening on http://{addr}");
    println!("RupeeCoin Explorer → http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ── Embedded SPA ─────────────────────────────────────────────────────────────

const SPA_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width,initial-scale=1.0">
  <title>RupeeCoin Explorer</title>
  <style>
    *{margin:0;padding:0;box-sizing:border-box}
    body{font-family:'Segoe UI',system-ui,sans-serif;background:#0d1117;color:#e6edf3;min-height:100vh}
    header{background:#161b22;border-bottom:1px solid #30363d;padding:14px 24px;display:flex;align-items:center;gap:12px}
    .logo{font-size:20px;font-weight:700;color:#f78166;letter-spacing:-0.5px}
    .badge{background:#21262d;border:1px solid #30363d;border-radius:6px;padding:4px 10px;font-size:12px;color:#8b949e}
    .badge b{color:#79c0ff}
    main{max-width:1120px;margin:28px auto;padding:0 20px}
    .search-row{display:flex;gap:8px;margin-bottom:28px}
    #sq{flex:1;padding:10px 14px;background:#161b22;border:1px solid #30363d;border-radius:6px;color:#e6edf3;font-size:14px;outline:none;transition:border-color .15s}
    #sq:focus{border-color:#58a6ff}
    #sb{padding:10px 18px;background:#238636;border:1px solid #2ea043;border-radius:6px;color:#fff;cursor:pointer;font-size:14px;font-weight:600;transition:background .15s}
    #sb:hover{background:#2ea043}
    #result{margin-bottom:28px;background:#161b22;border:1px solid #30363d;border-radius:8px;overflow:hidden;display:none}
    .result-hdr{padding:10px 16px;background:#21262d;border-bottom:1px solid #30363d;font-size:13px;font-weight:600;color:#8b949e}
    #result-body{padding:14px 16px}
    #result-body pre{font-family:'Courier New',monospace;font-size:12px;color:#e6edf3;white-space:pre-wrap;word-break:break-all}
    .section-hdr{display:flex;align-items:center;justify-content:space-between;margin-bottom:12px}
    .section-hdr h2{font-size:16px;font-weight:600;color:#e6edf3}
    .section-hdr .sub{font-size:12px;color:#8b949e}
    table{width:100%;border-collapse:collapse;background:#161b22;border:1px solid #30363d;border-radius:8px;overflow:hidden}
    th{background:#21262d;padding:9px 14px;text-align:left;font-size:11px;font-weight:600;color:#8b949e;text-transform:uppercase;letter-spacing:.06em;border-bottom:1px solid #30363d}
    td{padding:9px 14px;font-size:13px;border-bottom:1px solid #1c2128;vertical-align:middle}
    tr:last-child td{border-bottom:none}
    tbody tr:hover td{background:#1c2128;cursor:pointer}
    .hash{font-family:'Courier New',monospace;font-size:11px;color:#79c0ff}
    .mono{font-family:'Courier New',monospace}
    .tag{display:inline-block;padding:2px 7px;border-radius:4px;font-size:10px;font-weight:600;background:#1f6feb22;color:#58a6ff;border:1px solid #1f6feb44}
    .spinner{display:inline-block;width:14px;height:14px;border:2px solid #30363d;border-top-color:#58a6ff;border-radius:50%;animation:spin .8s linear infinite;vertical-align:middle}
    @keyframes spin{to{transform:rotate(360deg)}}
    .loading{text-align:center;padding:36px;color:#8b949e;font-size:14px}
    .err{color:#f85149}
    .green{color:#3fb950}
  </style>
</head>
<body>
<header>
  <div class="logo">₹ RupeeCoin</div>
  <div class="badge">Explorer</div>
  <div class="badge">Chain height: <b id="tip">…</b></div>
  <div class="badge" style="margin-left:auto">Devnet / Testnet</div>
</header>
<main>
  <div class="search-row">
    <input id="sq" type="text" placeholder="Search by block height, block hash (64 hex), txid, or Base58 address…" />
    <button id="sb" onclick="doSearch()">Search</button>
  </div>
  <div id="result">
    <div class="result-hdr">Search Result</div>
    <div id="result-body"><pre id="rp"></pre></div>
  </div>
  <div class="section-hdr">
    <h2>Recent Blocks</h2>
    <span class="sub" id="refreshed"></span>
  </div>
  <div id="blocks"><div class="loading"><span class="spinner"></span>&nbsp; Loading chain data…</div></div>
</main>
<script>
const $ = id => document.getElementById(id);

async function json(url){
  const r = await fetch(url);
  if(!r.ok) throw new Error(await r.text());
  return r.json();
}

function ago(ts){
  const d = Math.floor(Date.now()/1000 - ts);
  if(d<5) return 'just now';
  if(d<60) return d+'s ago';
  if(d<3600) return Math.floor(d/60)+'m ago';
  return Math.floor(d/3600)+'h ago';
}

function short(h){ return h ? h.slice(0,10)+'…'+h.slice(-6) : '—'; }

function rpc(micro){ return (micro/1e8).toFixed(4)+' RPC'; }

async function loadBlocks(){
  try{
    const [tip, blocks] = await Promise.all([json('/api/height'), json('/api/recent')]);
    $('tip').textContent = '#'+tip.height;
    const rev = [...blocks].reverse();
    if(!rev.length){
      $('blocks').innerHTML='<div class="loading">No blocks yet — start the node.</div>';
      return;
    }
    const rows = rev.map(b=>`<tr onclick="lookupBlock(${b.height})">
      <td class="mono">${b.height}</td>
      <td><span class="hash" title="${b.hash}">${short(b.hash)}</span></td>
      <td>${ago(b.timestamp)}</td>
      <td>${new Date(b.timestamp*1000).toLocaleTimeString()}</td>
      <td class="mono">${b.tx_count}</td>
      <td class="mono">${b.slot}</td>
      <td class="mono">${rpc(b.base_fee)}</td>
      <td class="mono">${rpc(b.gas_used)}</td>
    </tr>`).join('');
    $('blocks').innerHTML=`<table>
      <thead><tr><th>Height</th><th>Hash</th><th>Age</th><th>Time</th><th>Txs</th><th>Slot</th><th>Base Fee</th><th>Gas Used</th></tr></thead>
      <tbody>${rows}</tbody>
    </table>`;
    $('refreshed').textContent='Updated '+ago(Math.floor(Date.now()/1000));
  }catch(e){
    $('blocks').innerHTML='<div class="loading err">Error: '+e.message+'</div>';
  }
}

async function lookupBlock(height){
  $('sq').value = String(height);
  await doSearch();
}

async function doSearch(){
  const q = $('sq').value.trim();
  if(!q) return;
  $('result').style.display='block';
  $('rp').textContent='Searching…';
  try{
    const data = await json('/api/search?q='+encodeURIComponent(q));
    $('rp').textContent = JSON.stringify(data, null, 2);
  }catch(e){
    $('rp').textContent = 'Not found or error: '+e.message;
  }
}

$('sq').addEventListener('keydown', e=>{ if(e.key==='Enter') doSearch(); });

loadBlocks();
setInterval(loadBlocks, 8000);
</script>
</body>
</html>
"#;
