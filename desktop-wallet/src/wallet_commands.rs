//! Tauri commands — all callable from the frontend via `invoke()`.

use serde::{Deserialize, Serialize};

use rupeecoin_crypto::{decode_address, encode_address, Hash, KeyPair};
use rupeecoin_sdk::Wallet;
use rupeecoin_types::{OutPoint, Transaction, TxInput, TxOutput, MICRO_RPC_PER_COIN};

use crate::keystore::{self, KeystoreInner};

// ── Response types ─────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct WalletInfo {
    /// Base58Check transparent address at current hd_receive_index.
    pub address: String,
    /// Address as 40-char hex (for RPC calls).
    pub address_hex: String,
    /// Current HD receive index.
    pub receive_index: u32,
    /// Path to the keystore file (echoed for display).
    pub keystore_path: String,
}

#[derive(Serialize)]
pub struct BalanceInfo {
    pub balance_micro: u64,
    pub balance_rpc: f64,
    pub utxo_count: usize,
}

#[derive(Deserialize)]
struct UtxoEntry {
    txid: String,
    vout: u32,
    amount_micro: u64,
}

#[derive(Deserialize)]
struct HeightResp {
    height: u64,
}

#[derive(Deserialize)]
struct NonceResp {
    nonce: u64,
}

// ── Helper: seed → HD wallet at index ─────────────────────────────────────

fn wallet_from_seed(seed_hex: &str, index: u32) -> Result<(Wallet, KeyPair), String> {
    let seed_bytes: [u8; 32] = hex::decode(seed_hex)
        .map_err(|e| format!("bad seed hex: {e}"))?
        .try_into()
        .map_err(|_| "seed must be 32 bytes")?;
    let mut wallet = Wallet::from_seed(seed_bytes);
    wallet.receive_index = index; // receive_index is pub; no need to loop
    let kp = wallet.receive_keypair();
    Ok((wallet, kp))
}

// ── Commands ───────────────────────────────────────────────────────────────

/// Generate a new HD wallet, encrypt it, and save to `keystore_path`.
/// Returns the wallet's first transparent receive address (Base58Check).
#[tauri::command]
pub fn generate_wallet(
    passphrase: String,
    keystore_path: String,
) -> Result<WalletInfo, String> {
    let wallet = Wallet::generate();
    let seed = wallet.master_seed();
    let inner = KeystoreInner {
        master_seed_hex: hex::encode(seed),
        receive_index: 0,
    };
    keystore::save(&keystore_path, &inner, &passphrase)?;

    let address_raw = wallet.receive_address();
    let address = encode_address(&address_raw);
    let address_hex = hex::encode(address_raw.0);

    Ok(WalletInfo { address, address_hex, receive_index: 0, keystore_path })
}

/// Decrypt an existing keystore and return wallet info (address, index).
#[tauri::command]
pub fn unlock_wallet(
    passphrase: String,
    keystore_path: String,
) -> Result<WalletInfo, String> {
    let inner = keystore::load(&keystore_path, &passphrase)?;
    let (wallet, _kp) = wallet_from_seed(&inner.master_seed_hex, inner.receive_index)?;

    let address_raw = wallet.receive_address();
    let address = encode_address(&address_raw);
    let address_hex = hex::encode(address_raw.0);

    Ok(WalletInfo {
        address,
        address_hex,
        receive_index: inner.receive_index,
        keystore_path,
    })
}

/// Advance the HD receive index by 1, save keystore, return new address.
#[tauri::command]
pub fn advance_address(
    passphrase: String,
    keystore_path: String,
) -> Result<WalletInfo, String> {
    let mut inner = keystore::load(&keystore_path, &passphrase)?;
    inner.receive_index += 1;

    let (wallet, _kp) = wallet_from_seed(&inner.master_seed_hex, inner.receive_index)?;
    keystore::save(&keystore_path, &inner, &passphrase)?;

    let address_raw = wallet.receive_address();
    let address = encode_address(&address_raw);
    let address_hex = hex::encode(address_raw.0);

    Ok(WalletInfo {
        address,
        address_hex,
        receive_index: inner.receive_index,
        keystore_path,
    })
}

/// Get the current block height from the node.
#[tauri::command]
pub fn get_height(rpc_url: String) -> Result<u64, String> {
    let url = format!("{rpc_url}/rpc/height");
    let resp: HeightResp = ureq::get(&url)
        .call()
        .map_err(|e| format!("GET {url}: {e}"))?
        .into_json()
        .map_err(|e| format!("parse height: {e}"))?;
    Ok(resp.height)
}

/// Get transparent UTXO balance for the wallet's current address.
#[tauri::command]
pub fn get_balance(addr_hex: String, rpc_url: String) -> Result<BalanceInfo, String> {
    let url = format!("{rpc_url}/rpc/utxos?addr={addr_hex}");
    let utxos: Vec<UtxoEntry> = ureq::get(&url)
        .call()
        .map_err(|e| format!("GET {url}: {e}"))?
        .into_json()
        .map_err(|e| format!("parse utxos: {e}"))?;

    let balance_micro: u64 = utxos.iter().map(|u| u.amount_micro).sum();
    Ok(BalanceInfo {
        balance_micro,
        balance_rpc: balance_micro as f64 / MICRO_RPC_PER_COIN as f64,
        utxo_count: utxos.len(),
    })
}

/// Get the next expected nonce for the wallet's current address.
#[tauri::command]
pub fn get_nonce(addr_hex: String, rpc_url: String) -> Result<u64, String> {
    let url = format!("{rpc_url}/rpc/nonce?addr={addr_hex}");
    let resp: NonceResp = ureq::get(&url)
        .call()
        .map_err(|e| format!("GET {url}: {e}"))?
        .into_json()
        .map_err(|e| format!("parse nonce: {e}"))?;
    Ok(resp.nonce)
}

/// Build, sign, and broadcast a transparent transaction from the wallet.
/// Returns the transaction ID (hex).
#[tauri::command]
pub fn send_transaction(
    keystore_path: String,
    passphrase: String,
    to_address: String,
    amount_rpc: f64,
    rpc_url: String,
    chain_id: u32,
) -> Result<String, String> {
    let inner = keystore::load(&keystore_path, &passphrase)?;
    let (wallet, kp) = wallet_from_seed(&inner.master_seed_hex, inner.receive_index)?;

    let sender_addr = wallet.receive_address();
    let sender_hex = hex::encode(sender_addr.0);

    // Decode recipient.
    let recipient = decode_address(&to_address)
        .map_err(|e| format!("invalid recipient address: {e}"))?;

    let amount_micro = (amount_rpc * MICRO_RPC_PER_COIN as f64) as u64;
    if amount_micro == 0 {
        return Err("amount must be > 0".to_string());
    }

    // Fetch sender UTXOs.
    let utxos_url = format!("{rpc_url}/rpc/utxos?addr={sender_hex}");
    let utxos: Vec<UtxoEntry> = ureq::get(&utxos_url)
        .call()
        .map_err(|e| format!("fetch UTXOs: {e}"))?
        .into_json()
        .map_err(|e| format!("parse UTXOs: {e}"))?;

    let balance: u64 = utxos.iter().map(|u| u.amount_micro).sum();
    if balance < amount_micro {
        return Err(format!(
            "Insufficient balance: have {:.4} RPC, need {:.4} RPC",
            balance as f64 / MICRO_RPC_PER_COIN as f64,
            amount_rpc
        ));
    }

    // Fetch nonce.
    let nonce_url = format!("{rpc_url}/rpc/nonce?addr={sender_hex}");
    let nonce_resp: NonceResp = ureq::get(&nonce_url)
        .call()
        .map_err(|e| format!("fetch nonce: {e}"))?
        .into_json()
        .map_err(|e| format!("parse nonce: {e}"))?;
    let nonce = nonce_resp.nonce;

    // Greedy coin selection (largest first).
    let mut sorted = utxos;
    sorted.sort_by(|a, b| b.amount_micro.cmp(&a.amount_micro));
    let mut selected: Vec<&UtxoEntry> = Vec::new();
    let mut total_in: u64 = 0;
    for u in &sorted {
        if total_in >= amount_micro {
            break;
        }
        selected.push(u);
        total_in += u.amount_micro;
    }

    let change = total_in - amount_micro; // fee is implicit (burned)
    let mut outputs = vec![TxOutput { amount: amount_micro, recipient }];
    if change > 0 {
        outputs.push(TxOutput { amount: change, recipient: sender_addr });
    }

    // Build inputs with placeholder signatures.
    use rupeecoin_crypto::Signature;
    let inputs: Vec<TxInput> = selected
        .iter()
        .map(|u| {
            let raw = hex::decode(&u.txid).expect("txid hex");
            let arr: [u8; 32] = raw.try_into().expect("txid 32 bytes");
            TxInput {
                outpoint: OutPoint { txid: Hash::from_bytes(arr), vout: u.vout },
                public_key: kp.public_key.clone(),
                signature: Signature::empty(),
            }
        })
        .collect();

    let mut tx = Transaction {
        version: 1,
        chain_id,
        nonce,
        expiry: 0,
        inputs,
        outputs,
        coinbase_data: None,
    };

    // Sign all inputs with one signature (same key for all inputs from this address).
    let sig_hash = tx.sig_hash();
    let sig = kp.sign(sig_hash.as_bytes());
    for input in &mut tx.inputs {
        input.signature = sig.clone();
    }

    let txid = hex::encode(tx.txid().as_bytes());

    // Submit.
    let tx_hex = hex::encode(
        bincode::serialize(&tx).map_err(|e| format!("serialize tx: {e}"))?,
    );
    let submit_url = format!("{rpc_url}/rpc/tx");
    let resp_body = ureq::post(&submit_url)
        .set("Content-Type", "application/octet-stream")
        .send_string(&tx_hex)
        .map_err(|e| format!("submit tx: {e}"))?
        .into_string()
        .map_err(|e| format!("read submit response: {e}"))?;

    // Parse to check for error.
    let resp_json: serde_json::Value =
        serde_json::from_str(&resp_body).unwrap_or(serde_json::Value::Null);
    if let Some(err) = resp_json.get("error") {
        return Err(format!("node rejected tx: {err}"));
    }

    Ok(txid)
}
