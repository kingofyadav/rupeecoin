use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use rupeecoin_crypto::encode_address;
use rupeecoin_sdk::{NodeStatus, RpcClient, UtxoEntry, Wallet};
use rupeecoin_types::{OutPoint, Transaction, TxOutput, MAINNET_CHAIN_ID};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn to_py_err(e: impl std::fmt::Display) -> PyErr {
    PyRuntimeError::new_err(e.to_string())
}

// ── PyWallet ──────────────────────────────────────────────────────────────────

/// HD wallet: derives transparent addresses and holds shielded-pool keys.
///
///     w = Wallet.generate()
///     print(w.receive_address())   # Base58Check string
///     seed = w.master_seed()       # bytes — back this up!
///     w2 = Wallet.from_seed(seed)  # restores identically
#[pyclass(name = "Wallet")]
struct PyWallet {
    inner: Wallet,
}

#[pymethods]
impl PyWallet {
    /// Create a wallet from a random seed.
    #[staticmethod]
    fn generate() -> Self {
        PyWallet { inner: Wallet::generate() }
    }

    /// Restore a wallet from a 32-byte seed (bytes or bytearray).
    #[staticmethod]
    fn from_seed(seed: &[u8]) -> PyResult<Self> {
        let arr: [u8; 32] = seed.try_into()
            .map_err(|_| PyRuntimeError::new_err("seed must be exactly 32 bytes"))?;
        Ok(PyWallet { inner: Wallet::from_seed(arr) })
    }

    /// Return the current receive address as a Base58Check string.
    fn receive_address(&self) -> String {
        self.inner.receive_address_string()
    }

    /// Return the address at `index` in the HD receive chain.
    fn address_at(&self, index: u32) -> String {
        encode_address(&self.inner.address_at(index))
    }

    /// Return the current receive address as a 40-char lowercase hex string.
    fn receive_address_hex(&self) -> String {
        let addr = self.inner.receive_address();
        hex::encode(addr.0)
    }

    /// Return the 32-byte master seed.  Store securely.
    fn master_seed<'py>(&self, py: Python<'py>) -> Bound<'py, pyo3::types::PyBytes> {
        pyo3::types::PyBytes::new_bound(py, self.inner.master_seed())
    }

    fn __repr__(&self) -> String {
        format!("Wallet(address={})", self.inner.receive_address_string())
    }
}

// ── PyRpcClient ───────────────────────────────────────────────────────────────

/// HTTP client for rupeecoin-node JSON-RPC.
///
///     rpc = RpcClient("http://127.0.0.1:9944")
///     print(rpc.height())
///     print(rpc.status())   # dict with slot, epoch, base_fee, …
#[pyclass(name = "RpcClient")]
struct PyRpcClient {
    inner: RpcClient,
    base_url: String,
}

#[pymethods]
impl PyRpcClient {
    #[new]
    fn new(url: &str) -> Self {
        let base = url.trim_end_matches('/').to_owned();
        PyRpcClient {
            inner: RpcClient::new(&base),
            base_url: base,
        }
    }

    /// Return the current chain tip height.
    fn height(&self) -> PyResult<u64> {
        self.inner.height().map_err(to_py_err)
    }

    /// Return a dict with the full node status snapshot.
    fn status<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let s: NodeStatus = self.inner.status().map_err(to_py_err)?;
        let d = PyDict::new_bound(py);
        d.set_item("height",                s.height)?;
        d.set_item("slot",                  s.slot)?;
        d.set_item("epoch",                 s.epoch)?;
        d.set_item("last_justified_epoch",  s.last_justified_epoch)?;
        d.set_item("last_finalized_epoch",  s.last_finalized_epoch)?;
        d.set_item("active_validators",     s.active_validators)?;
        d.set_item("total_stake",           s.total_stake)?;
        d.set_item("mempool_size",          s.mempool_size)?;
        d.set_item("pending_privacy_txs",   s.pending_privacy_txs)?;
        d.set_item("commitment_count",      s.commitment_count)?;
        d.set_item("base_fee",              s.base_fee)?;
        Ok(d)
    }

    /// Return a list of dicts for unspent UTXOs owned by `addr`.
    ///
    /// `addr` can be a 40-char hex string or a Base58Check address.
    fn utxos<'py>(&self, py: Python<'py>, addr: &str) -> PyResult<Vec<Bound<'py, PyDict>>> {
        let hex_addr = parse_addr_to_hex(addr).map_err(to_py_err)?;
        let entries: Vec<UtxoEntry> = self.inner.utxos(&hex_addr).map_err(to_py_err)?;
        entries
            .into_iter()
            .map(|u| {
                let d = PyDict::new_bound(py);
                d.set_item("txid",         &u.txid)?;
                d.set_item("vout",         u.vout)?;
                d.set_item("amount_micro", u.amount_micro)?;
                d.set_item("amount_rpc",   u.amount_rpc)?;
                Ok(d)
            })
            .collect()
    }

    /// Submit a signed transparent transaction (bincode hex string).
    ///
    /// Returns the node's status string ("accepted" on success).
    fn submit_tx(&self, tx_hex: &str) -> PyResult<String> {
        let bytes = hex::decode(tx_hex).map_err(to_py_err)?;
        let tx: Transaction = bincode::deserialize(&bytes).map_err(to_py_err)?;
        self.inner.submit_tx(&tx).map_err(to_py_err)?;
        Ok("accepted".to_owned())
    }

    fn __repr__(&self) -> String {
        format!("RpcClient(url='{}')", self.base_url)
    }
}

// ── PyOutPoint ────────────────────────────────────────────────────────────────

/// Reference to a previous unspent output (txid:vout).
#[pyclass(name = "OutPoint")]
#[derive(Clone)]
struct PyOutPoint {
    #[pyo3(get, set)]
    txid: String,
    #[pyo3(get, set)]
    vout: u32,
}

#[pymethods]
impl PyOutPoint {
    #[new]
    fn new(txid: &str, vout: u32) -> Self {
        PyOutPoint { txid: txid.to_owned(), vout }
    }

    fn __repr__(&self) -> String {
        format!("OutPoint('{}', {})", &self.txid[..8.min(self.txid.len())], self.vout)
    }
}

// ── PyTxOutput ────────────────────────────────────────────────────────────────

/// One output of a transparent transaction.
#[pyclass(name = "TxOutput")]
#[derive(Clone)]
struct PyTxOutput {
    /// Amount in micro-RPC (1 RPC = 100_000_000 micro-RPC).
    #[pyo3(get, set)]
    amount_micro: u64,
    /// Recipient address (Base58Check or 40-char hex).
    #[pyo3(get, set)]
    recipient: String,
}

#[pymethods]
impl PyTxOutput {
    #[new]
    fn new(recipient: &str, amount_micro: u64) -> Self {
        PyTxOutput { amount_micro, recipient: recipient.to_owned() }
    }

    fn __repr__(&self) -> String {
        format!(
            "TxOutput(recipient='{}', amount_rpc={:.8})",
            self.recipient,
            self.amount_micro as f64 / 1e8
        )
    }
}

// ── PyTransaction ─────────────────────────────────────────────────────────────

/// A fully serialized transparent transaction, ready for submission.
#[pyclass(name = "Transaction")]
struct PyTransaction {
    #[pyo3(get)]
    hex: String,
}

#[pymethods]
impl PyTransaction {
    fn __repr__(&self) -> String {
        format!("Transaction(hex='{}'…)", &self.hex[..16.min(self.hex.len())])
    }
}

// ── build_tx ──────────────────────────────────────────────────────────────────

/// Build and sign a transparent transaction.
///
///     op   = OutPoint("abcd…", 0)
///     out  = TxOutput("RPCaddress…", 100_000_000)  # 1.0 RPC
///     tx   = build_tx(wallet, [op], [out], chain_id=1, nonce=0)
///     rpc.submit_tx(tx.hex)
#[pyfunction]
#[pyo3(signature = (wallet, inputs, outputs, chain_id=1, nonce=0, expiry=0))]
fn build_tx(
    wallet: &PyWallet,
    inputs: Vec<PyRef<PyOutPoint>>,
    outputs: Vec<PyRef<PyTxOutput>>,
    chain_id: u32,
    nonce: u64,
    expiry: u64,
) -> PyResult<PyTransaction> {
    let keypair = wallet.inner.receive_keypair();

    let rust_outputs: Vec<TxOutput> = outputs
        .iter()
        .map(|o| {
            let addr_bytes = parse_addr_to_bytes(&o.recipient)?;
            Ok(TxOutput {
                amount: o.amount_micro,
                recipient: rupeecoin_crypto::Address(addr_bytes),
            })
        })
        .collect::<Result<_, String>>()
        .map_err(to_py_err)?;

    let outpoints: Vec<OutPoint> = inputs
        .iter()
        .map(|i| {
            let raw = hex::decode(&i.txid).map_err(|e| e.to_string())?;
            let arr: [u8; 32] = raw.try_into().map_err(|_| "txid must be 32 bytes / 64 hex chars".to_owned())?;
            Ok(OutPoint {
                txid: rupeecoin_crypto::Hash::from_bytes(arr),
                vout: i.vout,
            })
        })
        .collect::<Result<_, String>>()
        .map_err(to_py_err)?;

    let tx = rupeecoin_sdk::tx::build_transfer(&keypair, outpoints, rust_outputs, chain_id, nonce, expiry)
        .map_err(to_py_err)?;
    let bytes = bincode::serialize(&tx).map_err(to_py_err)?;
    Ok(PyTransaction { hex: hex::encode(bytes) })
}

// ── coins_to_micro / micro_to_coins ──────────────────────────────────────────

/// Convert whole RPC coins to micro-RPC.  ``1.5 → 150_000_000``
#[pyfunction]
fn coins_to_micro(amount: f64) -> u64 {
    rupeecoin_sdk::tx::coins_to_micro(amount)
}

/// Convert micro-RPC to a human-readable string.  ``150_000_000 → "1.5 RPC"``
#[pyfunction]
fn micro_to_coins_str(micro: u64) -> String {
    rupeecoin_sdk::tx::micro_to_coins_str(micro)
}

// ── address helpers ───────────────────────────────────────────────────────────

/// Encode 20 raw address bytes to a Base58Check string.
#[pyfunction]
fn encode_addr(raw: &[u8]) -> PyResult<String> {
    let arr: [u8; 20] = raw
        .try_into()
        .map_err(|_| PyRuntimeError::new_err("address must be exactly 20 bytes"))?;
    Ok(encode_address(&rupeecoin_crypto::Address(arr)))
}

/// Decode a Base58Check or 40-char hex address to 20 raw bytes.
#[pyfunction]
fn decode_addr<'py>(py: Python<'py>, addr: &str) -> PyResult<Bound<'py, pyo3::types::PyBytes>> {
    let bytes = parse_addr_to_bytes(addr).map_err(to_py_err)?;
    Ok(pyo3::types::PyBytes::new_bound(py, &bytes))
}

// ── internal helpers ──────────────────────────────────────────────────────────

fn parse_addr_to_hex(addr: &str) -> Result<String, String> {
    if addr.len() == 40 && addr.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(addr.to_lowercase());
    }
    let bytes = rupeecoin_crypto::decode_address(addr)
        .map_err(|e| e.to_string())?;
    Ok(hex::encode(bytes.0))
}

fn parse_addr_to_bytes(addr: &str) -> Result<[u8; 20], String> {
    if addr.len() == 40 && addr.chars().all(|c| c.is_ascii_hexdigit()) {
        let v = hex::decode(addr).map_err(|e| e.to_string())?;
        return v.try_into().map_err(|_| "bad hex length".into());
    }
    let a = rupeecoin_crypto::decode_address(addr).map_err(|e| e.to_string())?;
    Ok(a.0)
}

// ── Module ────────────────────────────────────────────────────────────────────

#[pymodule]
fn rupeecoin(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyWallet>()?;
    m.add_class::<PyRpcClient>()?;
    m.add_class::<PyOutPoint>()?;
    m.add_class::<PyTxOutput>()?;
    m.add_class::<PyTransaction>()?;
    m.add_function(wrap_pyfunction!(build_tx, m)?)?;
    m.add_function(wrap_pyfunction!(coins_to_micro, m)?)?;
    m.add_function(wrap_pyfunction!(micro_to_coins_str, m)?)?;
    m.add_function(wrap_pyfunction!(encode_addr, m)?)?;
    m.add_function(wrap_pyfunction!(decode_addr, m)?)?;
    m.add("MAINNET_CHAIN_ID", MAINNET_CHAIN_ID)?;
    m.add("MICRO_RPC_PER_COIN", rupeecoin_types::MICRO_RPC_PER_COIN)?;
    Ok(())
}
