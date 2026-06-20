//! Spend a single specific mature UTXO to the faucet address.
use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use rupeecoin_crypto::{decode_address, Hash, KeyPair, Signature};
use rupeecoin_types::{OutPoint, Transaction, TxInput, TxOutput, MICRO_RPC_PER_COIN};

#[derive(Deserialize)]
struct NonceResp { nonce: u64 }

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut txid_hex = String::new();
    let mut vout: u32 = 0;
    let mut to_b58 = String::new();
    let mut amount_rpc: f64 = 45.0;
    let mut seed_hex = String::new();
    let mut node_url = "http://127.0.0.1:9944".to_string();
    let mut chain_id: u32 = 1;

    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--txid"     => txid_hex   = args.next().context("--txid")?,
            "--vout"     => vout       = args.next().context("--vout")?.parse()?,
            "--to"       => to_b58     = args.next().context("--to")?,
            "--amount"   => amount_rpc = args.next().context("--amount")?.parse()?,
            "--seed"     => seed_hex   = args.next().context("--seed")?,
            "--node"     => node_url   = args.next().context("--node")?,
            "--chain-id" => chain_id   = args.next().context("--chain-id")?.parse()?,
            other => anyhow::bail!("unknown flag: {other}"),
        }
    }

    let seed_bytes = hex::decode(&seed_hex).context("seed hex")?;
    let seed: [u8; 32] = seed_bytes.try_into().map_err(|_| anyhow!("seed must be 32 bytes"))?;
    let kp = KeyPair::from_seed(&seed);
    let sender_hex = hex::encode(kp.public_key.to_address().0);

    let txid_bytes: [u8; 32] = hex::decode(&txid_hex)
        .context("txid hex")?.try_into().map_err(|_| anyhow!("txid 32 bytes"))?;
    let outpoint = OutPoint { txid: Hash::from_bytes(txid_bytes), vout };

    let recipient = decode_address(&to_b58).map_err(|e| anyhow!("to address: {e}"))?;
    let amount_micro = (amount_rpc * MICRO_RPC_PER_COIN as f64) as u64;

    let nonce_body = ureq::get(&format!("{node_url}/rpc/nonce?addr={sender_hex}"))
        .call().map_err(|e| anyhow!("{e}"))?.into_string()?;
    let nonce = serde_json::from_str::<NonceResp>(&nonce_body)?.nonce;

    println!("Spending txid={txid_hex} vout={vout}  →  {to_b58}  {amount_rpc} RPC");

    let mut tx = Transaction {
        version: 1, chain_id, nonce, expiry: 0,
        inputs: vec![TxInput { outpoint, public_key: kp.public_key.clone(), signature: Signature::empty() }],
        outputs: vec![TxOutput { amount: amount_micro, recipient }],
        coinbase_data: None,
    };
    let sig = kp.sign(tx.sig_hash().as_bytes());
    tx.inputs[0].signature = sig;

    let tx_hex = hex::encode(bincode::serialize(&tx)?);
    println!("Txid: {}", hex::encode(tx.sig_hash().as_bytes()));

    let resp = ureq::post(&format!("{node_url}/rpc/tx"))
        .set("Content-Type", "application/octet-stream")
        .send_string(&tx_hex)
        .map_err(|e| anyhow!("POST /rpc/tx: {e}"))?.into_string()?;
    println!("Node response: {resp}");
    Ok(())
}
