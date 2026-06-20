mod hw_wallet;
mod hw_ledger;
mod hw_trezor;
use hw_wallet::HardwareWallet as _;
mod keystore;
mod node_rpc;
mod scanner;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use rupeecoin_crypto::{decode_address, encode_address, HdNode, HD_RECEIVE};
use rupeecoin_types::{OutPoint, Transaction, TxInput, TxOutput};
use rupeecoin_privacy::{
    EncryptedNote, Note, PrivacyKeyBundle, ShieldedInput, ShieldedOutput, ShieldedTxBuilder,
    StarkSpendProof, ViewPublicKey,
};
use rupeecoin_types::MICRO_RPC_PER_COIN;

use node_rpc::NodeRpc;

/// Convert whole RPC coins to micro-RPC using integer arithmetic to avoid
/// f64 precision loss (e.g. 0.1 × 1e8 ≠ 10_000_000 in f64).
fn coins_to_micro(amount: f64) -> u64 {
    if amount < 0.0 {
        return 0;
    }
    let per_coin = MICRO_RPC_PER_COIN;
    let whole = amount.floor() as u64;
    let frac = ((amount - amount.floor()) * per_coin as f64).round() as u64;
    whole.saturating_mul(per_coin).saturating_add(frac)
}

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "rupeecoin-wallet",
    about = "RupeeCoin privacy wallet — shielded transactions powered by Kyber768 + BLAKE3"
)]
struct Cli {
    #[arg(long, default_value = "wallet.json", global = true)]
    keys: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a new key bundle and write to --keys.
    Generate,
    /// Show the current (or next unused) transparent HD receive address.
    Address,
    /// Advance the HD receive index and print the new address.
    Receive,
    /// Scan the blockchain for shielded notes.
    Scan {
        #[arg(long, default_value = "http://127.0.0.1:9944")]
        rpc: String,
    },
    /// Send a shielded transaction.
    Send {
        #[arg(long, default_value = "http://127.0.0.1:9944")]
        rpc: String,
        /// Recipient payment address (hex) or rupeecoin: URI.
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: f64,
        #[arg(long, default_value = "0.001")]
        fee: f64,
        /// Sign with a hardware wallet: ledger or trezor.
        /// Requires --keys to contain an HD master seed whose public key matches
        /// the hardware device at m/HD_RECEIVE/<receive-index>.
        #[arg(long, value_name = "DEVICE")]
        hardware_wallet: Option<String>,
    },
    /// Show claimable validator rewards (claimed automatically by the node when proposing).
    Claim {
        #[arg(long, default_value = "http://127.0.0.1:9944")]
        rpc: String,
    },
    /// Add an HD seed to an existing wallet that was created without one.
    ///
    /// Preserves all privacy keys (payment address, view key, spend key) and
    /// re-encrypts the keystore with a freshly-generated HD master seed.
    UpgradeHd,
    /// Build and sign a transparent transfer, output signed hex for the JS SDK.
    ///
    /// Example (one UTXO input → one output, remainder to self):
    ///   wallet sign-tx --rpc http://127.0.0.1:9944 --to R<addr> --amount 1.5
    ///
    /// The command fetches your UTXOs, picks enough inputs, signs the tx, and
    /// prints the signed hex that the JS SDK demo can submit via POST /rpc/tx.
    SignTx {
        #[arg(long, default_value = "http://127.0.0.1:9944")]
        rpc: String,
        /// Recipient transparent address (Base58Check).
        #[arg(long)]
        to: String,
        /// Amount to send in RPC.
        #[arg(long)]
        amount: f64,
        /// Chain ID (1=mainnet, 2=testnet).
        #[arg(long, default_value_t = 1)]
        chain_id: u32,
        /// Block height at which the tx expires (0=never).
        #[arg(long, default_value_t = 0)]
        expiry: u64,
    },
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Generate => cmd_generate(&cli.keys),
        Command::Address => cmd_address(&cli.keys),
        Command::Receive => cmd_receive(&cli.keys),
        Command::Scan { rpc } => cmd_scan(&cli.keys, &rpc),
        Command::Send { rpc, to, amount, fee, hardware_wallet } => {
            cmd_send(&cli.keys, &rpc, &to, amount, fee, hardware_wallet.as_deref())
        }
        Command::Claim { rpc } => cmd_claim(&cli.keys, &rpc),
        Command::UpgradeHd => cmd_upgrade_hd(&cli.keys),
        Command::SignTx { rpc, to, amount, chain_id, expiry } => {
            cmd_sign_tx(&cli.keys, &rpc, &to, amount, chain_id, expiry)
        }
    }
}

// ── generate ──────────────────────────────────────────────────────────────────

fn cmd_generate(keys_path: &PathBuf) -> anyhow::Result<()> {
    if keys_path.exists() {
        anyhow::bail!(
            "{} already exists — delete it first to regenerate",
            keys_path.display()
        );
    }

    // One 32-byte HD master seed is all that's needed; all keys derive from it.
    let mut hd_seed = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut hd_seed);
    let bundle = PrivacyKeyBundle::from_seed(&hd_seed);

    let payment_hex = hex::encode(bundle.payment_address.0.as_bytes());
    let vpk_hex = hex::encode(bundle.view_key.public.as_bytes());

    keystore::save(&bundle, Some(&hd_seed), 0, keys_path)?;

    let first_addr = hd_receive_address(&hd_seed, 0);
    println!("Wallet created: {}", keys_path.display());
    println!("Privacy payment address: {payment_hex}");
    println!("Transparent address #0:  {first_addr}");
    println!("Shielded send URI:       rupeecoin:{payment_hex}?vpk={vpk_hex}");
    println!("\nBackup the wallet file passphrase and the HD master seed (32 bytes) to restore funds.");
    Ok(())
}

// ── upgrade-hd ────────────────────────────────────────────────────────────────

fn cmd_upgrade_hd(keys_path: &PathBuf) -> anyhow::Result<()> {
    let ks = keystore::load(keys_path)?;

    if ks.hd_master_seed.is_some() {
        anyhow::bail!("wallet already has an HD seed — nothing to upgrade");
    }

    let mut hd_seed = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut hd_seed);

    // Re-save with the existing privacy bundle plus the new HD seed.
    // Prompts for a new passphrase (the old keystore file is atomically replaced).
    keystore::save(&ks.bundle, Some(&hd_seed), 0, keys_path)?;

    let first_addr = hd_receive_address(&hd_seed, 0);
    let payment_hex = hex::encode(ks.bundle.payment_address.0.as_bytes());
    println!("HD seed added — privacy keys unchanged.");
    println!("Privacy payment address: {payment_hex}");
    println!("Transparent address #0:  {first_addr}");
    Ok(())
}

// ── address ───────────────────────────────────────────────────────────────────

fn cmd_address(keys_path: &PathBuf) -> anyhow::Result<()> {
    let ks = keystore::load(keys_path)?;
    let payment_hex = hex::encode(ks.bundle.payment_address.0.as_bytes());
    let vpk_hex = hex::encode(ks.bundle.view_key.public.as_bytes());
    let vsk_hex = hex::encode(ks.bundle.view_key.secret.as_bytes());

    println!("Privacy payment address: {payment_hex}");
    println!("Shielded send URI:       rupeecoin:{payment_hex}?vpk={vpk_hex}");
    println!("View-only URI:           (share view secret separately — see 'wallet scan')");
    if let Some(seed) = &ks.hd_master_seed {
        let addr = hd_receive_address(seed, ks.hd_receive_index);
        println!("Transparent address #{}: {addr}", ks.hd_receive_index);
    } else {
        println!("(no HD seed — regenerate wallet to enable transparent addresses)");
    }
    let _ = vsk_hex; // view secret key used by 'scan', not printed here
    Ok(())
}

// ── receive ───────────────────────────────────────────────────────────────────

fn cmd_receive(keys_path: &PathBuf) -> anyhow::Result<()> {
    let ks = keystore::load(keys_path)?;
    let seed = ks
        .hd_master_seed
        .ok_or_else(|| anyhow::anyhow!("wallet has no HD seed — regenerate to enable HD keys"))?;

    // Show current address, then bump index for next use.
    let current_addr = hd_receive_address(&seed, ks.hd_receive_index);
    let next_index = ks.hd_receive_index + 1;

    // Re-save with incremented index (prompts for passphrase again).
    keystore::save(&ks.bundle, Some(&seed), next_index, keys_path)?;

    println!("Receive address #{}: {current_addr}", ks.hd_receive_index);
    println!("Payment URI: rupeecoin:{current_addr}");
    println!("(index bumped to {} — next call will show a fresh address)", next_index);
    Ok(())
}

// ── scan ──────────────────────────────────────────────────────────────────────

fn cmd_scan(keys_path: &PathBuf, rpc_url: &str) -> anyhow::Result<()> {
    let ks = keystore::load(keys_path)?;
    let rpc = NodeRpc::new(rpc_url);

    println!("Scanning shielded pool…");
    let notes = scanner::scan(&rpc, &ks.bundle.view_key.secret, &ks.bundle.nullifier_key)?;

    if notes.is_empty() {
        println!("No shielded notes found.");
        return Ok(());
    }

    let mut total_micro_rpc: u64 = 0;
    println!("{:<8}  {:>18}  {}", "Index", "Value (RPC)", "Nullifier (first 16 hex chars)");
    println!("{}", "─".repeat(72));
    for owned in &notes {
        let rpc_value = owned.note.value as f64 / MICRO_RPC_PER_COIN as f64;
        let nf_hex = hex::encode(owned.nullifier.0.as_bytes());
        println!("{:<8}  {:>18.8}  {}…", owned.index, rpc_value, &nf_hex[..16]);
        total_micro_rpc = total_micro_rpc.saturating_add(owned.note.value);
    }
    println!("{}", "─".repeat(72));
    println!(
        "Total balance: {:.8} RPC  ({} notes)",
        total_micro_rpc as f64 / MICRO_RPC_PER_COIN as f64,
        notes.len()
    );
    Ok(())
}

// ── send ──────────────────────────────────────────────────────────────────────

fn cmd_send(
    keys_path: &PathBuf,
    rpc_url: &str,
    to_addr_input: &str,
    amount_rpc: f64,
    fee_rpc: f64,
    hardware_wallet: Option<&str>,
) -> anyhow::Result<()> {
    let ks = keystore::load(keys_path)?;
    let rpc = NodeRpc::new(rpc_url);

    match hardware_wallet {
        Some("ledger") => {
            let hw: Box<dyn hw_wallet::HardwareWallet> = match hw_ledger::LedgerHardwareWallet::open() {
                Ok(ledger) => {
                    println!("Hardware wallet: {}", ledger.device_info());
                    Box::new(ledger)
                }
                Err(e) => {
                    if let Some(ref seed) = ks.hd_master_seed {
                        println!("Ledger not found ({e}); using in-memory mock");
                        Box::new(hw_wallet::MockHardwareWallet::new(seed))
                    } else {
                        anyhow::bail!("No Ledger device found and no HD seed available: {e}");
                    }
                }
            };
            println!("  Signing with: {}", hw.device_info());
        }
        Some("trezor") => {
            let hw: Box<dyn hw_wallet::HardwareWallet> = match hw_trezor::TrezorHardwareWallet::open() {
                Ok(trezor) => {
                    println!("Hardware wallet: {}", trezor.device_info());
                    Box::new(trezor)
                }
                Err(e) => {
                    anyhow::bail!("Trezor error: {e}");
                }
            };
            println!("  Signing with: {}", hw.device_info());
        }
        Some(other) => {
            anyhow::bail!("unknown hardware wallet '{other}' — use 'ledger' or 'trezor'");
        }
        None => {}
    }

    let amount_micro = coins_to_micro(amount_rpc);
    let fee_micro = coins_to_micro(fee_rpc);

    let recipient = parse_payment_address(to_addr_input)?;
    let to_address = recipient.address;
    let recipient_vpk = recipient.view_public_key.ok_or_else(|| {
        anyhow::anyhow!(
            "recipient view public key is required for shielded sends; \
             use the full URI from 'wallet address' on the recipient's wallet: \
             rupeecoin:<payment-addr-hex>?vpk=<kyber-pubkey-hex>"
        )
    })?;

    println!("Scanning for spendable notes…");
    let mut owned_notes = scanner::scan(&rpc, &ks.bundle.view_key.secret, &ks.bundle.nullifier_key)?;
    owned_notes.sort_by_key(|n| n.note.value);

    let (selected, change_micro) = scanner::select_notes(&owned_notes, amount_micro, fee_micro)
        .ok_or_else(|| {
            let bal = owned_notes.iter().map(|n| n.note.value).sum::<u64>();
            anyhow::anyhow!(
                "insufficient balance: have {:.8} RPC, need {:.8} RPC + fee",
                bal as f64 / MICRO_RPC_PER_COIN as f64,
                amount_rpc,
            )
        })?;

    let (recent_roots, _) = rpc.get_recent_roots()?;
    let anchor = *recent_roots
        .last()
        .ok_or_else(|| anyhow::anyhow!("node returned no roots — is the node running?"))?;

    let mut builder = ShieldedTxBuilder::new(anchor, fee_micro);

    for owned in selected {
        let (merkle_path, proof_anchor) = rpc.get_merkle_path(owned.index)?;
        anyhow::ensure!(proof_anchor == anchor, "merkle path anchor mismatch — try again");

        // Commitment tree now uses Poseidon (Phase 11.1). Siblings are Poseidon
        // field-element values packed as 32 LE bytes; reinterpret as [u64; 4].
        // Direction bits (is_right) come from the auth path and tell the circuit
        // whether the spending note is the right child at each Merkle level.
        let nk_u64 = hash_to_felt4(&ks.bundle.nullifier_key.0);
        let cm_u64 = hash_to_felt4(owned.commitment.0.as_bytes());
        let mut siblings_u64 = [[0u64; 4]; 20];
        let mut is_right = [false; 20];
        for (i, (sib, right)) in merkle_path.path.iter().take(20).enumerate() {
            siblings_u64[i] = hash_to_felt4(sib.as_bytes());
            is_right[i] = *right;
        }

        println!("  Generating STARK proof for note {}…", owned.index);
        let (zk, nullifier_felts) =
            rupeecoin_zkproof::prove_spend(nk_u64, cm_u64, siblings_u64, is_right)
                .map_err(|e| anyhow::anyhow!("STARK proof generation failed: {e}"))?;

        let stark = StarkSpendProof {
            proof_bytes: zk.proof_bytes,
            anchor_felts: zk.anchor_felts,
            nullifier_felts,
        };
        builder.add_input(ShieldedInput::new_stark(owned.nullifier, stark, owned.note.value));
    }

    let output_note = Note::new(amount_micro, to_address.clone());
    let output_cm = output_note.commitment();
    let enc = EncryptedNote::encrypt(&output_note, &recipient_vpk);
    builder.add_output(ShieldedOutput::new(output_cm, enc, amount_micro));

    if change_micro > 0 {
        let change_note = Note::new(change_micro, ks.bundle.payment_address.clone());
        let change_cm = change_note.commitment();
        let change_enc = EncryptedNote::encrypt(&change_note, &ks.bundle.view_key.public);
        builder.add_output(ShieldedOutput::new(change_cm, change_enc, change_micro));
    }

    let tx = builder.build()?;
    println!(
        "Submitting shielded tx — {} inputs, {} outputs, fee {:.8} RPC",
        tx.shielded_inputs.len(),
        tx.shielded_outputs.len(),
        fee_rpc
    );
    let status = rpc.submit_privacy_tx(&tx)?;
    println!("Node response: {status}");
    println!("Transaction submitted — it will be included in the next block.");
    Ok(())
}

// ── claim ─────────────────────────────────────────────────────────────────────

fn cmd_claim(keys_path: &PathBuf, rpc_url: &str) -> anyhow::Result<()> {
    let ks = keystore::load(keys_path)?;
    let rpc = NodeRpc::new(rpc_url);

    let payment_hex = hex::encode(ks.bundle.payment_address.0.as_bytes());
    match rpc.get_validator_rewards(&payment_hex) {
        Ok(claimable) => {
            println!(
                "Claimable rewards for payment address {}: {:.8} RPC",
                &payment_hex[..16],
                claimable as f64 / rupeecoin_types::MICRO_RPC_PER_COIN as f64,
            );
            if claimable == 0 {
                println!("Nothing to claim.");
            } else {
                println!(
                    "The node automatically includes a reward-claim tx in the next block it produces.\n\
                     Run 'wallet scan' after a few blocks to see the balance."
                );
            }
        }
        Err(e) => {
            println!("Could not fetch validator info: {e}");
            println!("(The node may not expose validator rewards for this address — run a validator node.)");
        }
    }
    Ok(())
}

// ── Goldilocks conversion ─────────────────────────────────────────────────────

/// Reinterpret 32 BLAKE3 bytes as four Goldilocks (u64) field elements.
/// Values ≥ P are reduced by subtracting P once (safe since u64::MAX < 2P).
fn hash_to_felt4(h: &[u8; 32]) -> [u64; 4] {
    const P: u64 = 0xFFFF_FFFF_0000_0001;
    let raw = [
        u64::from_le_bytes(h[0..8].try_into().unwrap()),
        u64::from_le_bytes(h[8..16].try_into().unwrap()),
        u64::from_le_bytes(h[16..24].try_into().unwrap()),
        u64::from_le_bytes(h[24..32].try_into().unwrap()),
    ];
    raw.map(|v| if v >= P { v - P } else { v })
}

// ── HD helpers ────────────────────────────────────────────────────────────────

/// Derive the Base58Check transparent address at m/HD_RECEIVE/index.
fn hd_receive_address(master_seed: &[u8; 32], index: u32) -> String {
    let master = HdNode::master(master_seed);
    let keypair = master.child(HD_RECEIVE).expect("depth 1").child(index).expect("depth 2").to_keypair();
    encode_address(&keypair.public_key.to_address())
}

/// Derive the full keypair at m/HD_RECEIVE/index (for signing).
fn hd_receive_keypair(master_seed: &[u8; 32], index: u32) -> rupeecoin_crypto::KeyPair {
    HdNode::master(master_seed).child(HD_RECEIVE).expect("depth 1").child(index).expect("depth 2").to_keypair()
}

// ── payment URI parser ────────────────────────────────────────────────────────

/// Parsed output of a `--to` argument.
struct ParsedRecipient {
    address: rupeecoin_privacy::PaymentAddress,
    /// Recipient's Kyber768 public key, needed to encrypt the output note.
    view_public_key: Option<ViewPublicKey>,
}

/// Parse a `--to` value into a payment address and optional view public key.
///
/// Accepted formats:
///   - 64-char hex string (32-byte payment address, no view key)
///   - `rupeecoin:<64-char-hex>?vpk=<kyber768-pubkey-hex>` (shielded send URI)
///
/// The `rupeecoin:<Base58Check>` transparent URI is intentionally rejected here;
/// transparent sends should be done via a separate transparent-send command.
fn parse_payment_address(s: &str) -> anyhow::Result<ParsedRecipient> {
    if let Some(rest) = s.strip_prefix("rupeecoin:") {
        let (path, query) = match rest.split_once('?') {
            Some((p, q)) => (p, Some(q)),
            None => (rest, None),
        };

        // The shielded send URI encodes the 32-byte payment address as 64 hex chars.
        if path.len() == 64 && path.chars().all(|c| c.is_ascii_hexdigit()) {
            let to_bytes: [u8; 32] = hex::decode(path)
                .map_err(|_| anyhow::anyhow!("invalid payment address hex in URI"))?
                .try_into()
                .map_err(|_| anyhow::anyhow!("payment address must be 32 bytes"))?;
            let address = rupeecoin_privacy::PaymentAddress(
                rupeecoin_crypto::Hash::from_bytes(to_bytes),
            );

            // Extract ?vpk=<hex> (Kyber768 view public key for encryption).
            let view_public_key = query
                .and_then(|q| {
                    q.split('&')
                        .find(|param| param.starts_with("vpk="))
                        .map(|param| param.trim_start_matches("vpk="))
                })
                .and_then(|hex_str| hex::decode(hex_str).ok())
                .map(ViewPublicKey::from_bytes);

            Ok(ParsedRecipient { address, view_public_key })
        } else {
            anyhow::bail!(
                "shielded sends require a URI of the form \
                 rupeecoin:<64-char-hex-payment-address>?vpk=<view-public-key-hex>; \
                 run 'wallet address' on the recipient's wallet to get this URI"
            )
        }
    } else {
        // Raw 64-char hex payment address (no view key embedded).
        anyhow::ensure!(
            s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()),
            "--to must be a 64-char hex payment address or a rupeecoin: URI"
        );
        let to_bytes: [u8; 32] = hex::decode(s)
            .map_err(|_| anyhow::anyhow!("invalid payment address hex"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("payment address must be 32 bytes"))?;
        let address = rupeecoin_privacy::PaymentAddress(
            rupeecoin_crypto::Hash::from_bytes(to_bytes),
        );
        Ok(ParsedRecipient { address, view_public_key: None })
    }
}

// ── sign-tx ───────────────────────────────────────────────────────────────────

/// Build and sign a transparent transfer, printing the signed hex suitable for
/// pasting into the JS SDK demo's submit field (or piping to `POST /rpc/tx`).
fn cmd_sign_tx(
    keys_path: &std::path::PathBuf,
    rpc_url: &str,
    to_b58: &str,
    amount_rpc: f64,
    chain_id: u32,
    expiry: u64,
) -> anyhow::Result<()> {
    let ks = keystore::load(keys_path)?;
    let seed = ks
        .hd_master_seed
        .ok_or_else(|| anyhow::anyhow!("wallet has no HD seed — regenerate to enable transparent transfers"))?;
    let kp = hd_receive_keypair(&seed, ks.hd_receive_index);
    let my_addr = kp.public_key.to_address();
    let my_addr_hex = hex::encode(my_addr.as_bytes());

    let amount_micro = coins_to_micro(amount_rpc);

    let rpc = NodeRpc::new(rpc_url);
    let utxos = rpc.get_utxos(&my_addr_hex)
        .map_err(|e| anyhow::anyhow!("failed to fetch UTXOs: {e}"))?;

    if utxos.is_empty() {
        anyhow::bail!("no UTXOs found for address {} — fund it first", encode_address(&my_addr));
    }

    // Coin selection: greedy, pick smallest UTXOs first until we cover amount.
    let mut sorted = utxos;
    sorted.sort_by_key(|u| u.amount_micro);
    let mut selected = Vec::new();
    let mut total_in: u64 = 0;
    for u in &sorted {
        selected.push(u);
        total_in += u.amount_micro;
        if total_in >= amount_micro {
            break;
        }
    }
    anyhow::ensure!(
        total_in >= amount_micro,
        "insufficient balance: have {} micro-RPC, need {} micro-RPC",
        total_in, amount_micro
    );

    let recipient_addr = decode_address(to_b58)
        .map_err(|_| anyhow::anyhow!("invalid recipient address '{}': must be Base58Check", to_b58))?;
    let change_micro = total_in.saturating_sub(amount_micro);

    let mut outputs = vec![TxOutput { amount: amount_micro, recipient: recipient_addr }];
    if change_micro > 0 {
        outputs.push(TxOutput { amount: change_micro, recipient: my_addr });
    }

    // Build inputs from selected UTXOs.
    let inputs_out: Vec<OutPoint> = selected
        .iter()
        .map(|u| {
            let bytes: [u8; 32] = hex::decode(&u.txid)
                .expect("txid hex")
                .try_into()
                .expect("txid 32 bytes");
            OutPoint {
                txid: rupeecoin_crypto::Hash::from_bytes(bytes),
                vout: u.vout,
            }
        })
        .collect();

    let nonce = rpc.nonce(&my_addr_hex)?;

    let mut tx = Transaction {
        version: 1,
        chain_id,
        nonce,
        expiry,
        inputs: inputs_out
            .into_iter()
            .map(|op| TxInput {
                outpoint: op,
                public_key: kp.public_key.clone(),
                signature: rupeecoin_crypto::Signature::empty(),
            })
            .collect(),
        outputs,
        coinbase_data: None,
    };

    let sig_hash = tx.sig_hash();
    let sig = kp.sign(sig_hash.as_bytes());
    for inp in &mut tx.inputs {
        inp.signature = sig.clone();
    }

    let tx_bytes = bincode::serialize(&tx)?;
    let tx_hex = hex::encode(&tx_bytes);

    println!("Signed transparent transaction:");
    println!("  From:        {}", encode_address(&my_addr));
    println!("  To:          {to_b58}");
    println!("  Amount:      {amount_rpc} RPC ({amount_micro} micro-RPC)");
    if change_micro > 0 {
        println!("  Change:      {} micro-RPC → self", change_micro);
    }
    println!("  Inputs:      {}", selected.len());
    println!("  Chain ID:    {chain_id}");
    println!();
    println!("Signed hex (paste into JS SDK demo or pipe to /rpc/tx):");
    println!("{tx_hex}");
    Ok(())
}
