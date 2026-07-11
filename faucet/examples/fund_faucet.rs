//! One-shot tool: transfer coins from the validator address to the faucet.
//!
//! Used when the node is started with --validator-seed so we know the private
//! key and can build a spending transaction from the accumulated coinbase UTXOs.
//!
//! Usage:
//!   cargo run --example fund_faucet -p rupeecoin-faucet -- \
//!     --seed <64-char-hex>       \
//!     --to   <faucet_b58_addr>   \
//!     --amount 5000              \
//!     --node http://127.0.0.1:9944

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

use rupeecoin_crypto::{decode_address, encode_address, Hash, KeyPair, Signature};
use rupeecoin_types::{
    OutPoint, Transaction, TxInput, TxOutput, MICRO_RPC_PER_COIN, TESTNET_CHAIN_ID,
};

fn main() -> Result<()> {
    let mut seed_hex = String::new();
    let mut to_b58 = String::new();
    let mut amount_rpc: f64 = 5000.0;
    let mut node_url = "http://127.0.0.1:9944".to_string();
    let mut chain_id = TESTNET_CHAIN_ID;

    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--seed" => seed_hex = args.next().context("--seed value missing")?,
            "--to" => to_b58 = args.next().context("--to value missing")?,
            "--amount" => amount_rpc = args.next().context("--amount value missing")?.parse()?,
            "--node" => node_url = args.next().context("--node value missing")?,
            "--chain-id" => chain_id = args.next().context("--chain-id value missing")?.parse()?,
            other => bail!("unknown flag: {other}"),
        }
    }

    if seed_hex.is_empty() {
        bail!("--seed is required");
    }
    if to_b58.is_empty() {
        bail!("--to is required");
    }

    // Derive the sender keypair from the seed.
    let raw = hex::decode(&seed_hex).context("--seed must be valid hex")?;
    let seed_bytes: [u8; 32] = raw
        .try_into()
        .map_err(|_| anyhow!("--seed must be 32 bytes (64 hex chars)"))?;
    let kp = KeyPair::from_seed(&seed_bytes);
    let sender_addr = kp.public_key.to_address();
    let sender_hex = hex::encode(sender_addr.0);
    println!("Sender address : {}", encode_address(&sender_addr));

    // Decode recipient.
    let recipient = decode_address(&to_b58).map_err(|e| anyhow!("invalid --to address: {e}"))?;
    println!("Recipient      : {to_b58}");

    let amount_micro = (amount_rpc * MICRO_RPC_PER_COIN as f64) as u64;
    println!("Amount         : {amount_rpc} RPC  ({amount_micro} µRPC)");

    // Fetch sender UTXOs from node.
    println!("\nFetching UTXOs from {node_url}…");
    let utxos = fetch_utxos(&node_url, &sender_hex)?;
    if utxos.is_empty() {
        bail!("No UTXOs found for sender — has the node mined any blocks with --validator-seed?");
    }
    let balance: u64 = utxos.iter().map(|u| u.amount_micro).sum();
    println!(
        "Found {} UTXO(s)  total={} µRPC ({:.4} RPC)",
        utxos.len(),
        balance,
        balance as f64 / MICRO_RPC_PER_COIN as f64
    );

    if balance < amount_micro {
        bail!("Insufficient balance: have {balance} µRPC, need {amount_micro} µRPC");
    }

    // Greedy coin selection (largest first to minimise input count).
    let mut sorted = utxos;
    sorted.sort_by_key(|u| std::cmp::Reverse(u.amount_micro));
    let mut selected = Vec::new();
    let mut total_in: u64 = 0;
    for u in &sorted {
        if total_in >= amount_micro {
            break;
        }
        selected.push(u);
        total_in += u.amount_micro;
    }

    // Change output (fee = total_in - amount_micro, burned implicitly).
    // We keep change ≥ 0; fee is effectively 0 here (no output fee field).
    let change = total_in - amount_micro;

    let mut outputs = vec![TxOutput {
        amount: amount_micro,
        recipient,
    }];
    if change > 0 {
        outputs.push(TxOutput {
            amount: change,
            recipient: sender_addr,
        });
    }

    // Build inputs with placeholder sigs.
    let inputs: Vec<TxInput> = selected
        .iter()
        .map(|u| {
            let raw = hex::decode(&u.txid).expect("txid hex");
            let arr: [u8; 32] = raw.try_into().expect("txid 32 bytes");
            TxInput {
                outpoint: OutPoint {
                    txid: Hash::from_bytes(arr),
                    vout: u.vout,
                },
                public_key: kp.public_key.clone(),
                signature: Signature::empty(),
            }
        })
        .collect();

    let mut tx = Transaction {
        version: 1,
        chain_id,
        nonce: 0, // First spend from this address; node tracks nonce per address.
        expiry: 0,
        inputs,
        outputs,
        coinbase_data: None,
    };

    // Sign all inputs.
    let sig_hash = tx.sig_hash();
    let sig = kp.sign(sig_hash.as_bytes());
    for input in &mut tx.inputs {
        input.signature = sig.clone();
    }

    let txid = hex::encode(tx.txid().as_bytes());
    println!("\nTxid: {txid}");
    println!("Inputs: {}  Outputs: {}", tx.inputs.len(), tx.outputs.len());

    // Encode and submit.
    let tx_hex = hex::encode(bincode::serialize(&tx)?);
    println!("Submitting to {node_url}/rpc/tx…");

    let resp = ureq::post(&format!("{node_url}/rpc/tx"))
        .set("Content-Type", "application/octet-stream")
        .send_string(&tx_hex)
        .map_err(|e| anyhow!("POST /rpc/tx failed: {e}"))?
        .into_string()?;

    println!("Node response  : {resp}");
    println!("\nDone! Wait for the next block to confirm, then check the faucet balance.");
    Ok(())
}

#[derive(Deserialize)]
struct UtxoItem {
    txid: String,
    vout: u32,
    amount_micro: u64,
}

fn fetch_utxos(node_url: &str, addr_hex: &str) -> Result<Vec<UtxoItem>> {
    let url = format!("{node_url}/rpc/utxos?addr={addr_hex}");
    ureq::get(&url)
        .call()
        .map_err(|e| anyhow!("GET {url}: {e}"))?
        .into_json::<Vec<UtxoItem>>()
        .map_err(|e| anyhow!("decode UTXO response: {e}"))
}
