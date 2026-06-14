//! Phase 9.5 — RupeeCoin Testnet Faucet
//!
//! A lightweight web service that drips testnet RPC to users who submit their
//! Base58Check address.  Rate-limited to one request per IP per cooldown window.
//!
//! Usage:
//!   rupeecoin-faucet --node-rpc http://127.0.0.1:9944 --keyfile faucet-key.json

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, bail, Context, Result};
use axum::{
    extract::{ConnectInfo, State},
    http::StatusCode,
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use clap::Parser;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use rupeecoin_crypto::{decode_address, encode_address, Address, Hash, HdNode, Signature};
use rupeecoin_types::{
    OutPoint, Transaction, TxInput, TxOutput, MICRO_RPC_PER_COIN, TESTNET_CHAIN_ID,
};

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "rupeecoin-faucet", about = "RupeeCoin Testnet Faucet")]
struct Cli {
    /// HTTP port for the faucet web UI.
    #[arg(long, default_value = "8081")]
    port: u16,

    /// Node RPC base URL.
    #[arg(long, default_value = "http://127.0.0.1:9944")]
    node_rpc: String,

    /// Path to the faucet keyfile (JSON; created with a fresh seed on first run).
    #[arg(long, default_value = "faucet-key.json")]
    keyfile: PathBuf,

    /// Chain ID (1=mainnet, 2=testnet).
    #[arg(long, default_value_t = TESTNET_CHAIN_ID)]
    chain_id: u32,

    /// Network label shown in the UI.
    #[arg(long, default_value = "Devnet-1")]
    network: String,

    /// Drip amount in full RPC (default 100).
    #[arg(long, default_value_t = 100)]
    drip_rpc: u64,

    /// Cooldown between drips per source IP, in hours (default 24).
    #[arg(long, default_value_t = 24)]
    cooldown_hours: u64,
}

// ── Keyfile ───────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct Keyfile {
    seed_hex: String,
    nonce: u64,
}

fn load_or_create_keyfile(path: &Path) -> Result<([u8; 32], u64)> {
    if path.exists() {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading keyfile {}", path.display()))?;
        let kf: Keyfile =
            serde_json::from_str(&text).context("parsing keyfile JSON")?;
        let raw = hex::decode(&kf.seed_hex).context("invalid seed_hex in keyfile")?;
        let seed: [u8; 32] = raw
            .try_into()
            .map_err(|_| anyhow!("seed must be exactly 32 bytes"))?;
        info!(
            "Loaded keyfile {} (nonce={})",
            path.display(),
            kf.nonce
        );
        Ok((seed, kf.nonce))
    } else {
        let mut seed = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut seed);
        let kf = Keyfile {
            seed_hex: hex::encode(seed),
            nonce: 0,
        };
        std::fs::write(path, serde_json::to_string_pretty(&kf)?)
            .with_context(|| format!("writing keyfile {}", path.display()))?;
        info!(
            "Created new keyfile {} — back this file up to preserve the faucet key",
            path.display()
        );
        Ok((seed, 0))
    }
}

fn save_keyfile(path: &Path, seed: &[u8; 32], nonce: u64) -> Result<()> {
    let kf = Keyfile {
        seed_hex: hex::encode(seed),
        nonce,
    };
    std::fs::write(path, serde_json::to_string_pretty(&kf)?)
        .with_context(|| format!("saving keyfile {}", path.display()))?;
    Ok(())
}

// ── Application state ─────────────────────────────────────────────────────────

/// Immutable faucet configuration (no lock needed).
struct FaucetConfig {
    seed: [u8; 32],
    address: Address,
    address_b58: String,
    address_hex: String,
    node_rpc: String,
    chain_id: u32,
    drip_amount_micro: u64,
    fee_micro: u64,
    cooldown_secs: u64,
    network: String,
    keyfile_path: PathBuf,
}

/// Mutable state protected by a tokio Mutex (held across await points during drip).
struct FaucetMutable {
    nonce: u64,
    rate_limits: HashMap<IpAddr, u64>,
    total_drips: u64,
}

struct Faucet {
    config: FaucetConfig,
    mutable: tokio::sync::Mutex<FaucetMutable>,
}

type SharedFaucet = Arc<Faucet>;

// ── Node RPC helpers (called inside spawn_blocking) ───────────────────────────

#[derive(Deserialize)]
struct UtxoItem {
    txid: String,
    vout: u32,
    amount_micro: u64,
}

fn node_fetch_utxos(node_rpc: &str, addr_hex: &str) -> Result<Vec<UtxoItem>> {
    let url = format!("{node_rpc}/rpc/utxos?addr={addr_hex}");
    let resp = ureq::get(&url)
        .call()
        .map_err(|e| anyhow!("GET {url}: {e}"))?
        .into_json::<Vec<UtxoItem>>()
        .map_err(|e| anyhow!("decode UTXO response: {e}"))?;
    Ok(resp)
}

fn node_submit_tx(node_rpc: &str, tx_hex: &str) -> Result<()> {
    let url = format!("{node_rpc}/rpc/tx");
    ureq::post(&url)
        .set("Content-Type", "application/octet-stream")
        .send_string(tx_hex)
        .map_err(|e| anyhow!("POST {url}: {e}"))?;
    Ok(())
}

// ── Transaction builder ───────────────────────────────────────────────────────

fn build_drip_tx(
    seed: &[u8; 32],
    utxos: &[UtxoItem],
    recipient: Address,
    faucet_addr: Address,
    chain_id: u32,
    nonce: u64,
    drip_amount: u64,
    fee: u64,
) -> Result<Transaction> {
    let needed = drip_amount
        .checked_add(fee)
        .ok_or_else(|| anyhow!("drip + fee overflow"))?;

    // Greedy coin selection — take UTXOs until we cover the drip + fee.
    let mut selected: Vec<&UtxoItem> = Vec::new();
    let mut total_in: u64 = 0;
    for utxo in utxos {
        if total_in >= needed {
            break;
        }
        selected.push(utxo);
        total_in = total_in.saturating_add(utxo.amount_micro);
    }

    if total_in < needed {
        bail!(
            "insufficient faucet balance: have {total_in} µRPC, need {needed} µRPC (drip + fee)"
        );
    }

    let change = total_in - drip_amount - fee;
    let keypair = HdNode::master(seed).to_keypair();

    // Build inputs with placeholder empty signatures (filled in after sig_hash).
    let inputs: Vec<TxInput> = selected
        .iter()
        .map(|u| {
            let raw = hex::decode(&u.txid).expect("txid from node is valid hex");
            let arr: [u8; 32] = raw.try_into().expect("txid is 32 bytes");
            TxInput {
                outpoint: OutPoint {
                    txid: Hash::from_bytes(arr),
                    vout: u.vout,
                },
                public_key: keypair.public_key.clone(),
                signature: Signature::empty(),
            }
        })
        .collect();

    let mut outputs = vec![TxOutput {
        amount: drip_amount,
        recipient,
    }];
    if change > 0 {
        outputs.push(TxOutput {
            amount: change,
            recipient: faucet_addr,
        });
    }

    let mut tx = Transaction {
        version: 1,
        chain_id,
        nonce,
        expiry: 0,
        inputs,
        outputs,
        coinbase_data: None,
    };

    // Sign every input with the faucet key (all inputs belong to the faucet address).
    let sig_hash = tx.sig_hash();
    let sig = keypair.sign(sig_hash.as_bytes());
    for input in &mut tx.inputs {
        input.signature = sig.clone();
    }

    Ok(tx)
}

// ── Handlers ──────────────────────────────────────────────────────────────────

async fn handler_index(State(f): State<SharedFaucet>) -> impl IntoResponse {
    Html(render_html(
        &f.config.network,
        &f.config.address_b58,
        f.config.drip_amount_micro / MICRO_RPC_PER_COIN,
        f.config.cooldown_secs / 3600,
    ))
}

#[derive(Deserialize)]
struct DripRequest {
    address: String,
}

#[derive(Serialize)]
struct DripResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    txid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    retry_after_secs: Option<u64>,
}

async fn handler_drip(
    State(f): State<SharedFaucet>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(req): Json<DripRequest>,
) -> (StatusCode, Json<DripResponse>) {
    let ip = peer.ip();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Validate recipient address before we touch the lock.
    let recipient = match decode_address(&req.address) {
        Ok(a) => a,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(DripResponse {
                    txid: None,
                    error: Some(format!("Invalid address: {e}")),
                    retry_after_secs: None,
                }),
            );
        }
    };

    // Acquire the exclusive lock for the full drip to prevent nonce races.
    let mut m = f.mutable.lock().await;

    // Rate-limit: one drip per IP per cooldown window.
    if let Some(&last) = m.rate_limits.get(&ip) {
        let elapsed = now.saturating_sub(last);
        if elapsed < f.config.cooldown_secs {
            let retry_in = f.config.cooldown_secs - elapsed;
            warn!("Rate limit for {ip}: retry in {retry_in}s");
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(DripResponse {
                    txid: None,
                    error: Some(format!(
                        "Rate limit: one drip per {} hours. Try again in {} minutes.",
                        f.config.cooldown_secs / 3600,
                        retry_in / 60 + 1,
                    )),
                    retry_after_secs: Some(retry_in),
                }),
            );
        }
    }

    // Fetch the faucet's current UTXOs from the node (blocking I/O in threadpool).
    let node_rpc = f.config.node_rpc.clone();
    let addr_hex = f.config.address_hex.clone();
    let utxos = match tokio::task::spawn_blocking(move || node_fetch_utxos(&node_rpc, &addr_hex))
        .await
    {
        Ok(Ok(u)) => u,
        Ok(Err(e)) => {
            error!("Fetch UTXOs: {e}");
            return (
                StatusCode::BAD_GATEWAY,
                Json(DripResponse {
                    txid: None,
                    error: Some(format!("Node unreachable: {e}")),
                    retry_after_secs: None,
                }),
            );
        }
        Err(e) => {
            error!("spawn_blocking panicked: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(DripResponse {
                    txid: None,
                    error: Some("Internal error".into()),
                    retry_after_secs: None,
                }),
            );
        }
    };

    // Build and sign the drip transaction.
    let tx = match build_drip_tx(
        &f.config.seed,
        &utxos,
        recipient,
        f.config.address,
        f.config.chain_id,
        m.nonce,
        f.config.drip_amount_micro,
        f.config.fee_micro,
    ) {
        Ok(t) => t,
        Err(e) => {
            error!("Build drip tx: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(DripResponse {
                    txid: None,
                    error: Some(e.to_string()),
                    retry_after_secs: None,
                }),
            );
        }
    };

    let txid = hex::encode(tx.txid().as_bytes());
    let tx_hex = match bincode::serialize(&tx) {
        Ok(b) => hex::encode(b),
        Err(e) => {
            error!("Serialize tx: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(DripResponse {
                    txid: None,
                    error: Some("Serialization error".into()),
                    retry_after_secs: None,
                }),
            );
        }
    };

    // Submit to the node.
    let node_rpc2 = f.config.node_rpc.clone();
    let tx_hex_clone = tx_hex.clone();
    if let Err(e) =
        tokio::task::spawn_blocking(move || node_submit_tx(&node_rpc2, &tx_hex_clone))
            .await
            .unwrap_or_else(|e| Err(anyhow!("spawn_blocking panic: {e}")))
    {
        error!("Submit tx: {e}");
        return (
            StatusCode::BAD_GATEWAY,
            Json(DripResponse {
                txid: None,
                error: Some(format!("Failed to submit transaction: {e}")),
                retry_after_secs: None,
            }),
        );
    }

    // Commit state: nonce, rate limit, total count, persisted keyfile.
    m.nonce += 1;
    m.total_drips += 1;
    m.rate_limits.insert(ip, now);

    if let Err(e) = save_keyfile(&f.config.keyfile_path, &f.config.seed, m.nonce) {
        // Non-fatal: log the error but don't fail the drip that already submitted.
        error!("Failed to persist keyfile (nonce may drift on restart): {e}");
    }

    info!(
        "Drip #{} → {} txid={}…  nonce_now={}",
        m.total_drips,
        req.address,
        &txid[..12],
        m.nonce
    );

    (
        StatusCode::OK,
        Json(DripResponse {
            txid: Some(txid),
            error: None,
            retry_after_secs: None,
        }),
    )
}

#[derive(Serialize)]
struct StatsResponse {
    faucet_address: String,
    balance_micro: u64,
    balance_rpc: f64,
    drip_amount_rpc: u64,
    total_drips: u64,
    network: String,
    cooldown_hours: u64,
}

async fn handler_stats(State(f): State<SharedFaucet>) -> impl IntoResponse {
    let node_rpc = f.config.node_rpc.clone();
    let addr_hex = f.config.address_hex.clone();
    let balance_micro =
        tokio::task::spawn_blocking(move || node_fetch_utxos(&node_rpc, &addr_hex))
            .await
            .ok()
            .and_then(|r| r.ok())
            .map(|u| u.iter().map(|x| x.amount_micro).sum::<u64>())
            .unwrap_or(0);

    let total_drips = f.mutable.lock().await.total_drips;

    Json(StatsResponse {
        faucet_address: f.config.address_b58.clone(),
        balance_micro,
        balance_rpc: balance_micro as f64 / MICRO_RPC_PER_COIN as f64,
        drip_amount_rpc: f.config.drip_amount_micro / MICRO_RPC_PER_COIN,
        total_drips,
        network: f.config.network.clone(),
        cooldown_hours: f.config.cooldown_secs / 3600,
    })
}

// ── Embedded HTML SPA ─────────────────────────────────────────────────────────

fn render_html(network: &str, faucet_addr: &str, drip_rpc: u64, cooldown_hours: u64) -> String {
    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>RupeeCoin Testnet Faucet</title>
<style>
* {{ box-sizing: border-box; margin: 0; padding: 0; }}
body {{
  background: #0d1117; color: #c9d1d9;
  font-family: 'Segoe UI', system-ui, sans-serif;
  min-height: 100vh;
  display: flex; flex-direction: column; align-items: center;
  padding: 2.5rem 1rem;
}}
header {{ text-align: center; margin-bottom: 2rem; }}
header h1 {{ font-size: 2rem; color: #f0883e; margin-bottom: 0.4rem; }}
.badge {{
  display: inline-block; background: #1a2744; color: #79c0ff;
  font-size: 0.72rem; font-weight: 700; letter-spacing: 0.1em;
  padding: 0.2rem 0.7rem; border-radius: 1rem; border: 1px solid #388bfd;
}}
.card {{
  background: #161b22; border: 1px solid #30363d; border-radius: 12px;
  padding: 1.75rem; width: 100%; max-width: 560px; margin-bottom: 1.25rem;
}}
.card h2 {{ font-size: 1rem; color: #e6edf3; margin-bottom: 1.1rem; }}
.info-row {{
  display: flex; justify-content: space-between; align-items: baseline;
  font-size: 0.83rem; color: #8b949e; margin-bottom: 0.5rem; gap: 1rem;
}}
.info-row .val {{
  color: #c9d1d9; font-family: 'Consolas', monospace;
  text-align: right; word-break: break-all;
}}
.balance-val {{ color: #f0883e; font-weight: 700; }}
label {{ display: block; font-size: 0.85rem; color: #8b949e; margin-bottom: 0.45rem; }}
input[type=text] {{
  width: 100%; background: #0d1117; border: 1px solid #30363d; border-radius: 8px;
  color: #c9d1d9; font-family: 'Consolas', monospace; font-size: 0.85rem;
  padding: 0.6rem 0.85rem; margin-bottom: 1rem; outline: none;
  transition: border-color 0.15s;
}}
input[type=text]:focus {{ border-color: #388bfd; }}
button {{
  width: 100%; background: #f0883e; border: none; border-radius: 8px;
  color: #0d1117; font-size: 0.95rem; font-weight: 700;
  padding: 0.75rem; cursor: pointer; transition: background 0.15s, opacity 0.15s;
}}
button:hover:not(:disabled) {{ background: #cf7332; }}
button:disabled {{ opacity: 0.5; cursor: not-allowed; }}
.result {{
  margin-top: 1rem; padding: 0.8rem 1rem; border-radius: 8px;
  font-size: 0.85rem; display: none; word-break: break-all; line-height: 1.5;
}}
.result.ok  {{ background: #0d2d0d; border: 1px solid #238636; color: #3fb950; }}
.result.err {{ background: #2d0d0d; border: 1px solid #da3633; color: #f85149; }}
.txid-label {{ color: #8b949e; font-size: 0.78rem; margin-top: 0.35rem; }}
.footer {{ font-size: 0.75rem; color: #484f58; text-align: center; margin-top: 0.5rem; }}
</style>
</head>
<body>
<header>
  <h1>&#x20B9; RupeeCoin Faucet</h1>
  <span class="badge">{network}</span>
</header>

<div class="card">
  <h2>Faucet Status</h2>
  <div class="info-row">Drip amount <span class="val">{drip_rpc} RPC per request</span></div>
  <div class="info-row">Cooldown <span class="val">{cooldown_hours}h per IP address</span></div>
  <div class="info-row">Faucet address <span class="val">{faucet_addr}</span></div>
  <div class="info-row">Balance <span class="val balance-val" id="balance">loading&hellip;</span></div>
  <div class="info-row">Total drips <span class="val" id="total-drips">&mdash;</span></div>
</div>

<div class="card">
  <h2>Request Testnet RPC</h2>
  <label for="addr-input">Your testnet address (Base58Check starting with R&hellip;)</label>
  <input id="addr-input" type="text"
    placeholder="Rpaste your address here"
    autocomplete="off" spellcheck="false">
  <button id="drip-btn" onclick="doDrip()">Request {drip_rpc} RPC</button>
  <div class="result" id="result-box"></div>
</div>

<div class="footer">
  Testnet funds only &mdash; not redeemable on mainnet.<br>
  Powered by <strong>RupeeCoin</strong> v0.3.0
</div>

<script>
async function loadStats() {{
  try {{
    const r = await fetch('/api/stats');
    if (!r.ok) return;
    const d = await r.json();
    document.getElementById('balance').textContent =
      d.balance_rpc.toFixed(4) + ' RPC';
    document.getElementById('total-drips').textContent =
      d.total_drips.toLocaleString();
  }} catch (_) {{}}
}}

async function doDrip() {{
  const addr = document.getElementById('addr-input').value.trim();
  const btn  = document.getElementById('drip-btn');
  if (!addr) {{ showResult(false, 'Please enter your testnet address.'); return; }}
  btn.disabled = true;
  btn.textContent = 'Sending…';
  try {{
    const resp = await fetch('/drip', {{
      method: 'POST',
      headers: {{ 'Content-Type': 'application/json' }},
      body: JSON.stringify({{ address: addr }}),
    }});
    const d = await resp.json();
    if (d.txid) {{
      showResult(true,
        '✓ Sent {drip_rpc} RPC to your address!<br>' +
        '<span class="txid-label">txid: ' + d.txid + '</span>');
      loadStats();
    }} else {{
      showResult(false, d.error || 'Unknown error');
    }}
  }} catch (e) {{
    showResult(false, 'Request failed: ' + e.message);
  }}
  btn.disabled = false;
  btn.textContent = 'Request {drip_rpc} RPC';
}}

function showResult(ok, msg) {{
  const el = document.getElementById('result-box');
  el.className = 'result ' + (ok ? 'ok' : 'err');
  el.innerHTML = msg;
  el.style.display = 'block';
}}

loadStats();
setInterval(loadStats, 15000);
</script>
</body>
</html>"##,
        network = network,
        faucet_addr = faucet_addr,
        drip_rpc = drip_rpc,
        cooldown_hours = cooldown_hours,
    )
}

// ── main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rupeecoin_faucet=info".into()),
        )
        .init();

    let cli = Cli::parse();

    let (seed, nonce) = load_or_create_keyfile(&cli.keyfile)?;

    let keypair = HdNode::master(&seed).to_keypair();
    let address = keypair.public_key.to_address();
    let address_b58 = encode_address(&address);
    let address_hex = hex::encode(address.0);

    info!("Faucet address : {address_b58}");
    info!("Faucet addr hex: {address_hex}");
    info!("Nonce (on disk): {nonce}");
    info!("Node RPC       : {}", cli.node_rpc);
    info!("Chain ID       : {}", cli.chain_id);
    info!("Drip amount    : {} RPC", cli.drip_rpc);
    info!("Cooldown       : {}h per IP", cli.cooldown_hours);

    let faucet = Arc::new(Faucet {
        config: FaucetConfig {
            seed,
            address,
            address_b58,
            address_hex,
            node_rpc: cli.node_rpc,
            chain_id: cli.chain_id,
            drip_amount_micro: cli.drip_rpc * MICRO_RPC_PER_COIN,
            // 0.01 RPC fee is burned (input_sum − output_sum difference)
            fee_micro: MICRO_RPC_PER_COIN / 100,
            cooldown_secs: cli.cooldown_hours * 3600,
            network: cli.network,
            keyfile_path: cli.keyfile,
        },
        mutable: tokio::sync::Mutex::new(FaucetMutable {
            nonce,
            rate_limits: HashMap::new(),
            total_drips: 0,
        }),
    });

    let app = Router::new()
        .route("/", get(handler_index))
        .route("/drip", post(handler_drip))
        .route("/api/stats", get(handler_stats))
        .with_state(faucet);

    let bind = SocketAddr::from(([127, 0, 0, 1], cli.port));
    info!("Faucet listening on http://{bind}");
    let listener = tokio::net::TcpListener::bind(bind).await?;

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}
