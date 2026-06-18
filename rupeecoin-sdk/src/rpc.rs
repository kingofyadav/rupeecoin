//! HTTP JSON-RPC client for rupeecoin-node.
//!
//! All methods are synchronous (blocking ureq calls).  For async contexts
//! wrap calls in `tokio::task::spawn_blocking`.

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

use rupeecoin_crypto::Hash;
use rupeecoin_privacy::PrivacyTransaction;
use rupeecoin_types::Transaction;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const READ_TIMEOUT: Duration = Duration::from_secs(15);

/// A thin HTTP client for the rupeecoin-node JSON-RPC API.
///
/// ```rust,no_run
/// use rupeecoin_sdk::RpcClient;
///
/// let rpc = RpcClient::new("http://127.0.0.1:9944");
/// let height = rpc.height().unwrap();
/// ```
pub struct RpcClient {
    base_url: String,
    agent: ureq::Agent,
}

// ── Response shapes ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct HeightResponse {
    height: u64,
}

/// Comprehensive node snapshot from `GET /rpc/status`.
#[derive(Debug, Deserialize, Clone)]
pub struct NodeStatus {
    pub height: u64,
    pub slot: u64,
    pub epoch: u64,
    pub last_justified_epoch: u64,
    pub last_finalized_epoch: u64,
    pub active_validators: usize,
    pub total_stake: u64,
    pub mempool_size: usize,
    pub pending_privacy_txs: usize,
    pub commitment_count: u64,
    pub base_fee: u64,
}

/// Transparent UTXO entry from `GET /rpc/utxos`.
#[derive(Debug, Deserialize, Clone)]
pub struct UtxoEntry {
    pub txid: String,
    pub vout: u32,
    pub amount_micro: u64,
    pub amount_rpc: f64,
}

#[derive(Debug, Deserialize)]
pub struct RecentRootsResponse {
    pub roots: Vec<String>,
    pub commitment_count: u64,
}

#[derive(Debug, Deserialize)]
pub struct ShieldedOutputEntry {
    pub index: u64,
    pub commitment_hex: String,
    pub encrypted_note_bytes: String,
}

#[derive(Debug, Deserialize)]
pub struct MerklePathResponse {
    pub path: Vec<(String, bool)>,
    pub anchor: String,
}

#[derive(Debug, Deserialize)]
pub struct NullifierStatusResponse {
    pub spent: bool,
}

#[derive(Debug, Deserialize)]
struct SubmitResponse {
    status: String,
}

// ── Client implementation ─────────────────────────────────────────────────────

impl RpcClient {
    /// Create a client targeting `base_url` (e.g. `"http://127.0.0.1:9944"`).
    pub fn new(base_url: impl Into<String>) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(CONNECT_TIMEOUT)
            .timeout_read(READ_TIMEOUT)
            .build();
        RpcClient {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            agent,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    // ── Endpoints ─────────────────────────────────────────────────────────────

    /// Return the current chain height.
    pub fn height(&self) -> Result<u64> {
        let resp: HeightResponse = self.agent.get(&self.url("/rpc/height"))
            .call()
            .context("GET /rpc/height")?
            .into_json()
            .context("decode height response")?;
        Ok(resp.height)
    }

    /// Return the most recent shielded commitment-tree roots and the total
    /// number of commitments.
    pub fn recent_roots(&self) -> Result<RecentRootsResponse> {
        self.agent.get(&self.url("/rpc/recent_roots"))
            .call()
            .context("GET /rpc/recent_roots")?
            .into_json()
            .context("decode recent_roots response")
    }

    /// Return a page of shielded outputs starting at leaf `start`.
    /// `count` is capped server-side at 1000.
    pub fn shielded_outputs(&self, start: u64, count: u64) -> Result<Vec<ShieldedOutputEntry>> {
        let url = format!("{}/rpc/shielded_outputs?start={start}&count={count}", self.base_url);
        self.agent.get(&url)
            .call()
            .context("GET /rpc/shielded_outputs")?
            .into_json()
            .context("decode shielded_outputs response")
    }

    /// Return the Merkle authentication path for commitment-tree leaf `index`.
    pub fn merkle_path(&self, index: u64) -> Result<MerklePathResponse> {
        let url = format!("{}/rpc/merkle_path?index={index}", self.base_url);
        self.agent.get(&url)
            .call()
            .context("GET /rpc/merkle_path")?
            .into_json()
            .context("decode merkle_path response")
    }

    /// Check whether a nullifier has been spent on-chain.
    pub fn nullifier_status(&self, nullifier: &Hash) -> Result<bool> {
        let hex = hex::encode(nullifier.as_bytes());
        let url = format!("{}/rpc/nullifier_status?hex={hex}", self.base_url);
        let resp: NullifierStatusResponse = self.agent.get(&url)
            .call()
            .context("GET /rpc/nullifier_status")?
            .into_json()
            .context("decode nullifier_status response")?;
        Ok(resp.spent)
    }

    /// Return the comprehensive node status snapshot.
    pub fn status(&self) -> Result<NodeStatus> {
        self.agent.get(&self.url("/rpc/status"))
            .call()
            .context("GET /rpc/status")?
            .into_json()
            .context("decode status response")
    }

    /// Return unspent transparent UTXOs for `addr_hex` (40-char lowercase hex).
    pub fn utxos(&self, addr_hex: &str) -> Result<Vec<UtxoEntry>> {
        let url = format!("{}/rpc/utxos?addr={addr_hex}", self.base_url);
        self.agent.get(&url)
            .call()
            .context("GET /rpc/utxos")?
            .into_json()
            .context("decode utxos response")
    }

    /// Return the next expected nonce for a transparent address.
    pub fn nonce(&self, addr_hex: &str) -> Result<u64> {
        #[derive(Deserialize)]
        struct Resp { nonce: u64 }
        let url = format!("{}/rpc/nonce?addr={addr_hex}", self.base_url);
        let resp: Resp = self.agent.get(&url)
            .call()
            .context("GET /rpc/nonce")?
            .into_json()
            .context("decode nonce response")?;
        Ok(resp.nonce)
    }

    /// Submit a transparent `Transaction`.  Returns `Ok(())` on acceptance.
    pub fn submit_tx(&self, tx: &Transaction) -> Result<()> {
        let bytes = bincode::serialize(tx).context("serialize transaction")?;
        let hex_body = hex::encode(&bytes);
        let resp: SubmitResponse = self.agent.post(&self.url("/rpc/tx"))
            .set("Content-Type", "text/plain")
            .send_string(&hex_body)
            .context("POST /rpc/tx")?
            .into_json()
            .context("decode tx response")?;
        if resp.status == "accepted" {
            Ok(())
        } else {
            Err(anyhow!("node rejected tx: {}", resp.status))
        }
    }

    /// Submit a `PrivacyTransaction`.  Returns `Ok(())` on acceptance.
    pub fn submit_privacy_tx(&self, tx: &PrivacyTransaction) -> Result<()> {
        let bytes = bincode::serialize(tx).context("serialize privacy tx")?;
        let hex_body = hex::encode(&bytes);
        let resp: SubmitResponse = self.agent.post(&self.url("/rpc/privacy_tx"))
            .set("Content-Type", "text/plain")
            .send_string(&hex_body)
            .context("POST /rpc/privacy_tx")?
            .into_json()
            .context("decode privacy_tx response")?;
        if resp.status == "accepted" {
            Ok(())
        } else {
            Err(anyhow!("node rejected privacy tx: {}", resp.status))
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_url_construction() {
        let rpc = RpcClient::new("http://127.0.0.1:9944");
        assert_eq!(rpc.url("/rpc/height"), "http://127.0.0.1:9944/rpc/height");

        // Trailing slash in base_url should be stripped.
        let rpc2 = RpcClient::new("http://127.0.0.1:9944/");
        assert_eq!(rpc2.url("/rpc/height"), "http://127.0.0.1:9944/rpc/height");
    }
}
