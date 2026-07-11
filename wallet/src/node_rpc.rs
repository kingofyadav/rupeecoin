/// HTTP client for the node's JSON-RPC endpoints.
use serde::Deserialize;
use thiserror::Error;

use rupeecoin_crypto::Hash;
use rupeecoin_privacy::{EncryptedNote, MerklePath, NoteCommitment, PrivacyTransaction};

#[derive(Debug, Error)]
pub enum RpcClientError {
    #[error("HTTP error: {0}")]
    Http(Box<ureq::Error>),
    #[error("JSON error: {0}")]
    Json(#[from] std::io::Error),
    #[error("hex decode: {0}")]
    Hex(#[from] hex::FromHexError),
    #[error("bincode decode: {0}")]
    Bincode(#[from] bincode::Error),
    #[error("invalid response: {0}")]
    Invalid(String),
}

impl From<ureq::Error> for RpcClientError {
    fn from(e: ureq::Error) -> Self {
        RpcClientError::Http(Box::new(e))
    }
}

pub struct NodeRpc {
    base_url: String,
}

impl NodeRpc {
    pub fn new(base_url: impl Into<String>) -> Self {
        NodeRpc {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    #[allow(dead_code)]
    pub fn get_height(&self) -> Result<u64, RpcClientError> {
        #[derive(Deserialize)]
        struct Resp {
            height: u64,
        }
        let resp: Resp = ureq::get(&self.url("/rpc/height")).call()?.into_json()?;
        Ok(resp.height)
    }

    pub fn get_recent_roots(&self) -> Result<(Vec<Hash>, u64), RpcClientError> {
        #[derive(Deserialize)]
        struct Resp {
            roots: Vec<String>,
            commitment_count: u64,
        }
        let resp: Resp = ureq::get(&self.url("/rpc/recent_roots"))
            .call()?
            .into_json()?;

        let mut roots = Vec::with_capacity(resp.roots.len());
        for s in &resp.roots {
            let raw = hex::decode(s)?;
            let arr: [u8; 32] = raw
                .try_into()
                .map_err(|_| RpcClientError::Invalid("root wrong length".into()))?;
            roots.push(Hash::from_bytes(arr));
        }
        Ok((roots, resp.commitment_count))
    }

    /// Fetch a window of shielded outputs for wallet scanning.
    pub fn get_shielded_outputs(
        &self,
        start: u64,
        count: u64,
    ) -> Result<Vec<ScannableOutput>, RpcClientError> {
        #[derive(Deserialize)]
        struct Item {
            index: u64,
            commitment_hex: String,
            encrypted_note_bytes: String,
        }

        let url = format!(
            "{}/rpc/shielded_outputs?start={}&count={}",
            self.base_url, start, count
        );
        let items: Vec<Item> = ureq::get(&url).call()?.into_json()?;

        let mut results = Vec::new();
        for item in items {
            let cm_bytes: [u8; 32] = hex::decode(&item.commitment_hex)?
                .try_into()
                .map_err(|_| hex::FromHexError::InvalidStringLength)?;
            let commitment = NoteCommitment(Hash::from_bytes(cm_bytes));

            let enc_bytes = hex::decode(&item.encrypted_note_bytes)?;
            let encrypted_note: EncryptedNote = bincode::deserialize(&enc_bytes)?;

            results.push(ScannableOutput {
                index: item.index,
                commitment,
                encrypted_note,
            });
        }
        Ok(results)
    }

    /// Fetch a Merkle authentication path for a leaf by index.
    pub fn get_merkle_path(&self, index: u64) -> Result<(MerklePath, Hash), RpcClientError> {
        #[derive(Deserialize)]
        struct Resp {
            path: Vec<(String, bool)>,
            anchor: String,
        }

        let url = format!("{}/rpc/merkle_path?index={}", self.base_url, index);
        let resp: Resp = ureq::get(&url).call()?.into_json()?;

        let mut path = Vec::with_capacity(resp.path.len());
        for (hex_str, is_right) in &resp.path {
            let raw = hex::decode(hex_str)?;
            let arr: [u8; 32] = raw
                .try_into()
                .map_err(|_| RpcClientError::Invalid("path sibling wrong length".into()))?;
            path.push((Hash::from_bytes(arr), *is_right));
        }

        let anchor_raw = hex::decode(&resp.anchor)?;
        let anchor_bytes: [u8; 32] = anchor_raw
            .try_into()
            .map_err(|_| RpcClientError::Invalid("anchor wrong length".into()))?;

        Ok((MerklePath { path }, Hash::from_bytes(anchor_bytes)))
    }

    /// Check whether a nullifier has been spent (guards against double-spend on wallet side).
    pub fn is_nullifier_spent(
        &self,
        nullifier: &rupeecoin_privacy::Nullifier,
    ) -> Result<bool, RpcClientError> {
        #[derive(Deserialize)]
        struct Resp {
            spent: bool,
        }
        let hex_nf = hex::encode(nullifier.0.as_bytes());
        let url = format!("{}/rpc/nullifier_status?hex={}", self.base_url, hex_nf);
        let resp: Resp = ureq::get(&url).call()?.into_json()?;
        Ok(resp.spent)
    }

    /// Query claimable rewards for a validator by payment-address hex.
    pub fn get_validator_rewards(&self, payment_addr_hex: &str) -> Result<u64, RpcClientError> {
        #[derive(Deserialize)]
        struct Resp {
            claimable_rewards: u64,
        }
        let url = format!(
            "{}/rpc/validator_info?addr={}",
            self.base_url, payment_addr_hex
        );
        let resp: Resp = ureq::get(&url).call()?.into_json()?;
        Ok(resp.claimable_rewards)
    }

    /// Fetch the next expected nonce for a transparent address.
    pub fn nonce(&self, addr_hex: &str) -> Result<u64, RpcClientError> {
        #[derive(Deserialize)]
        struct Resp {
            nonce: u64,
        }
        let url = format!("{}/rpc/nonce?addr={}", self.base_url, addr_hex);
        let resp: Resp = ureq::get(&url).call()?.into_json()?;
        Ok(resp.nonce)
    }

    /// Fetch unspent transparent UTXOs for a 20-byte address (hex).
    pub fn get_utxos(&self, addr_hex: &str) -> Result<Vec<UtxoEntry>, RpcClientError> {
        let url = format!("{}/rpc/utxos?addr={}", self.base_url, addr_hex);
        let entries: Vec<UtxoEntry> = ureq::get(&url).call()?.into_json()?;
        Ok(entries)
    }

    /// Submit a signed transparent transaction.  Returns the node's status string.
    #[allow(dead_code)]
    pub fn submit_transparent_tx(
        &self,
        tx: &rupeecoin_types::Transaction,
    ) -> Result<String, RpcClientError> {
        let bytes = bincode::serialize(tx)?;
        let hex_body = hex::encode(&bytes);
        #[derive(Deserialize)]
        struct Resp {
            status: String,
        }
        let resp: Resp = ureq::post(&self.url("/rpc/tx"))
            .set("Content-Type", "text/plain")
            .send_string(&hex_body)?
            .into_json()?;
        Ok(resp.status)
    }

    /// Submit a privacy transaction to the node.  Returns the status string.
    pub fn submit_privacy_tx(&self, tx: &PrivacyTransaction) -> Result<String, RpcClientError> {
        let bytes = bincode::serialize(tx)?;
        let hex_body = hex::encode(&bytes);

        #[derive(Deserialize)]
        struct Resp {
            status: String,
        }
        let resp: Resp = ureq::post(&self.url("/rpc/privacy_tx"))
            .set("Content-Type", "text/plain")
            .send_string(&hex_body)?
            .into_json()?;
        Ok(resp.status)
    }
}

/// A transparent UTXO entry as returned by GET /rpc/utxos.
#[derive(serde::Deserialize, Debug)]
pub struct UtxoEntry {
    pub txid: String,
    pub vout: u32,
    pub amount_micro: u64,
    #[allow(dead_code)]
    pub amount_rpc: f64,
}

/// A shielded output as received from the node RPC, ready for trial decryption.
pub struct ScannableOutput {
    pub index: u64,
    pub commitment: NoteCommitment,
    pub encrypted_note: EncryptedNote,
}
