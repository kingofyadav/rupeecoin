use std::path::Path;
use std::sync::Arc;

use rocksdb::{ColumnFamily, ColumnFamilyDescriptor, Options, WriteBatch, DB};
use rupeecoin_consensus::WeakSubjectivityCheckpoint;
use rupeecoin_crypto::Hash;
use rupeecoin_privacy::{NoteCommitment, Nullifier, ShieldedOutput};
use rupeecoin_types::{Block, OutPoint, TxOutput};
use thiserror::Error;

const CF_BLOCKS: &str = "blocks";
const CF_HEIGHTS: &str = "heights"; // u64 BE → block hash
const CF_UTXOS: &str = "utxos";
const CF_META: &str = "meta";
const CF_NULLIFIERS: &str = "nullifiers"; // nullifier hash → block height (u64 BE)
const CF_NOTE_COMMITMENTS: &str = "note_cmts"; // leaf index (u64 BE) → NoteCommitment
const CF_SHIELDED_OUTPUTS: &str = "shielded_outs"; // leaf index (u64 BE) → ShieldedOutput (full)
const CF_CHECKPOINTS: &str = "checkpoints"; // u64 BE height → WeakSubjectivityCheckpoint
const CF_CONTRACT_STORAGE: &str = "contract_storage"; // addr_u64_le ++ slot_u64_le → val_u64_le
const CF_CONTRACT_CODE: &str = "contract_code"; // addr_u64_le(8B) → bytecode bytes

const META_TIP_HASH: &[u8] = b"tip_hash";
const META_HEIGHT: &[u8] = b"height";

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("rocksdb: {0}")]
    Db(#[from] rocksdb::Error),
    #[error("serialization: {0}")]
    Codec(#[from] bincode::Error),
    #[error("column family missing: {0}")]
    MissingCf(&'static str),
    #[error("data corruption: {0}")]
    Corruption(String),
}

pub struct ChainStore {
    db: Arc<DB>,
}

impl ChainStore {
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        let cfs = [
            CF_BLOCKS,
            CF_HEIGHTS,
            CF_UTXOS,
            CF_META,
            CF_NULLIFIERS,
            CF_NOTE_COMMITMENTS,
            CF_SHIELDED_OUTPUTS,
            CF_CHECKPOINTS,
            CF_CONTRACT_STORAGE,
            CF_CONTRACT_CODE,
        ]
        .map(|name| ColumnFamilyDescriptor::new(name, Options::default()));

        let db = DB::open_cf_descriptors(&opts, path, cfs)?;
        Ok(ChainStore { db: Arc::new(db) })
    }

    /// Open the store in read-only mode — safe to use concurrently with a running node.
    /// Snapshot is current as of when the primary last flushed to SST files.
    pub fn open_read_only(path: &Path) -> Result<Self, StorageError> {
        let opts = Options::default();
        let cf_names = [
            CF_BLOCKS,
            CF_HEIGHTS,
            CF_UTXOS,
            CF_META,
            CF_NULLIFIERS,
            CF_NOTE_COMMITMENTS,
            CF_SHIELDED_OUTPUTS,
            CF_CHECKPOINTS,
            CF_CONTRACT_STORAGE,
            CF_CONTRACT_CODE,
        ];
        let db = DB::open_cf_for_read_only(&opts, path, cf_names, false)?;
        Ok(ChainStore { db: Arc::new(db) })
    }

    /// Open the store as a RocksDB secondary instance.  `secondary_path` is a
    /// scratch directory (may not exist yet) used for WAL replay.  Call
    /// `try_catch_up_with_primary()` periodically to pick up new data from the
    /// running node without restarting.
    pub fn open_secondary(primary_path: &Path, secondary_path: &Path) -> Result<Self, StorageError> {
        let opts = Options::default();
        let cf_names = [
            CF_BLOCKS,
            CF_HEIGHTS,
            CF_UTXOS,
            CF_META,
            CF_NULLIFIERS,
            CF_NOTE_COMMITMENTS,
            CF_SHIELDED_OUTPUTS,
            CF_CHECKPOINTS,
            CF_CONTRACT_STORAGE,
            CF_CONTRACT_CODE,
        ];
        std::fs::create_dir_all(secondary_path).ok();
        let db = DB::open_cf_as_secondary(&opts, primary_path, secondary_path, cf_names)?;
        Ok(ChainStore { db: Arc::new(db) })
    }

    /// Pull the latest data from the primary node (no-op when opened normally).
    pub fn try_catch_up_with_primary(&self) {
        let _ = self.db.try_catch_up_with_primary();
    }

    fn cf(&self, name: &'static str) -> Result<&ColumnFamily, StorageError> {
        self.db.cf_handle(name).ok_or(StorageError::MissingCf(name))
    }

    // ── Block storage ────────────────────────────────────────────────────────

    pub fn put_block(&self, block: &Block) -> Result<(), StorageError> {
        let hash = block.hash();
        let data = bincode::serialize(block)?;
        let height_key = block.header.height.to_be_bytes();

        let mut batch = WriteBatch::default();
        batch.put_cf(self.cf(CF_BLOCKS)?, hash.as_bytes(), &data);
        batch.put_cf(self.cf(CF_HEIGHTS)?, &height_key, hash.as_bytes());
        self.db.write(batch)?;
        Ok(())
    }

    pub fn get_block_by_hash(&self, hash: &Hash) -> Result<Option<Block>, StorageError> {
        match self.db.get_cf(self.cf(CF_BLOCKS)?, hash.as_bytes())? {
            Some(b) => Ok(Some(bincode::deserialize(&b)?)),
            None => Ok(None),
        }
    }

    pub fn get_block_by_height(&self, height: u64) -> Result<Option<Block>, StorageError> {
        let height_key = height.to_be_bytes();
        let hash_bytes = match self.db.get_cf(self.cf(CF_HEIGHTS)?, &height_key)? {
            Some(b) => b,
            None => return Ok(None),
        };
        let arr: [u8; 32] = hash_bytes
            .as_slice()
            .try_into()
            .map_err(|_| StorageError::Corruption("invalid block hash in heights CF".into()))?;
        self.get_block_by_hash(&Hash::from_bytes(arr))
    }

    // ── UTXO storage ─────────────────────────────────────────────────────────

    pub fn get_utxo(&self, outpoint: &OutPoint) -> Result<Option<TxOutput>, StorageError> {
        let key = bincode::serialize(outpoint)?;
        match self.db.get_cf(self.cf(CF_UTXOS)?, &key)? {
            Some(b) => Ok(Some(bincode::deserialize(&b)?)),
            None => Ok(None),
        }
    }

    // ── Chain tip ────────────────────────────────────────────────────────────

    pub fn get_tip(&self) -> Result<Option<(Hash, u64)>, StorageError> {
        let hash_raw = match self.db.get_cf(self.cf(CF_META)?, META_TIP_HASH)? {
            Some(b) => b,
            None => return Ok(None),
        };
        let height_raw = match self.db.get_cf(self.cf(CF_META)?, META_HEIGHT)? {
            Some(b) => b,
            None => return Ok(None),
        };

        let hash_arr: [u8; 32] = hash_raw
            .as_slice()
            .try_into()
            .map_err(|_| StorageError::Corruption("tip hash corrupt".into()))?;
        let height_arr: [u8; 8] = height_raw
            .as_slice()
            .try_into()
            .map_err(|_| StorageError::Corruption("tip height corrupt".into()))?;

        Ok(Some((
            Hash::from_bytes(hash_arr),
            u64::from_be_bytes(height_arr),
        )))
    }

    // ── Privacy: nullifiers ──────────────────────────────────────────────────

    pub fn is_nullifier_spent(&self, nullifier: &Nullifier) -> Result<bool, StorageError> {
        Ok(self
            .db
            .get_cf(self.cf(CF_NULLIFIERS)?, nullifier.0.as_bytes())?
            .is_some())
    }

    pub fn get_note_commitment_count(&self) -> Result<u64, StorageError> {
        match self.db.get_cf(self.cf(CF_META)?, b"note_count")? {
            Some(b) => {
                let arr: [u8; 8] = b
                    .as_slice()
                    .try_into()
                    .map_err(|_| StorageError::Corruption("note_count corrupt".into()))?;
                Ok(u64::from_be_bytes(arr))
            }
            None => Ok(0),
        }
    }

    /// Reconstruct the commitment tree from stored leaf data.
    pub fn load_commitment_tree(&self) -> Result<rupeecoin_privacy::CommitmentTree, StorageError> {
        let count = self.get_note_commitment_count()?;
        let mut tree = rupeecoin_privacy::CommitmentTree::new();
        for idx in 0..count {
            let key = idx.to_be_bytes();
            match self.db.get_cf(self.cf(CF_NOTE_COMMITMENTS)?, &key)? {
                Some(b) => {
                    let cm: NoteCommitment = bincode::deserialize(&b)?;
                    tree.append(cm);
                }
                None => {
                    return Err(StorageError::Corruption(format!(
                        "missing note commitment at index {idx}"
                    )));
                }
            }
        }
        Ok(tree)
    }

    /// Fetch a window of shielded outputs by leaf index for wallet scanning.
    pub fn get_shielded_outputs(
        &self,
        start: u64,
        count: u64,
    ) -> Result<Vec<(u64, ShieldedOutput)>, StorageError> {
        let total = self.get_note_commitment_count()?;
        let end = (start + count).min(total);
        let mut results = Vec::new();
        for idx in start..end {
            let key = idx.to_be_bytes();
            if let Some(b) = self.db.get_cf(self.cf(CF_SHIELDED_OUTPUTS)?, &key)? {
                let output: ShieldedOutput = bincode::deserialize(&b)?;
                results.push((idx, output));
            }
        }
        Ok(results)
    }

    // ── Atomic block application ─────────────────────────────────────────────

    /// Write the block, its UTXO delta, privacy pool changes, and the new tip
    /// in a single atomic RocksDB batch.
    pub fn apply_block(&self, block: &Block) -> Result<(), StorageError> {
        let hash = block.hash();
        let height = block.header.height;
        let block_data = bincode::serialize(block)?;
        let height_key = height.to_be_bytes();

        let mut batch = WriteBatch::default();

        batch.put_cf(self.cf(CF_BLOCKS)?, hash.as_bytes(), &block_data);
        batch.put_cf(self.cf(CF_HEIGHTS)?, &height_key, hash.as_bytes());

        // Transparent UTXO delta.
        for tx in &block.transactions {
            for input in &tx.inputs {
                let key = bincode::serialize(&input.outpoint)?;
                batch.delete_cf(self.cf(CF_UTXOS)?, &key);
            }
            let txid = tx.txid();
            for (vout, output) in tx.outputs.iter().enumerate() {
                let outpoint = OutPoint {
                    txid,
                    vout: vout as u32,
                };
                let key = bincode::serialize(&outpoint)?;
                let val = bincode::serialize(output)?;
                batch.put_cf(self.cf(CF_UTXOS)?, &key, &val);
            }
        }

        // Privacy pool: nullifiers + note commitments.
        let mut note_count = self.get_note_commitment_count()?;
        for privacy_tx in &block.privacy_txs {
            for input in &privacy_tx.shielded_inputs {
                batch.put_cf(
                    self.cf(CF_NULLIFIERS)?,
                    input.nullifier.0.as_bytes(),
                    &height_key,
                );
            }
            for output in &privacy_tx.shielded_outputs {
                let key = note_count.to_be_bytes();
                let cm_val = bincode::serialize(&output.commitment)?;
                batch.put_cf(self.cf(CF_NOTE_COMMITMENTS)?, &key, &cm_val);
                // Store full ShieldedOutput so wallets can scan encrypted notes.
                let out_val = bincode::serialize(output)?;
                batch.put_cf(self.cf(CF_SHIELDED_OUTPUTS)?, &key, &out_val);
                note_count += 1;
            }
        }
        batch.put_cf(self.cf(CF_META)?, b"note_count", &note_count.to_be_bytes());

        batch.put_cf(self.cf(CF_META)?, META_TIP_HASH, hash.as_bytes());
        batch.put_cf(self.cf(CF_META)?, META_HEIGHT, &height_key);

        self.db.write(batch)?;
        Ok(())
    }

    // ── Explorer helpers ─────────────────────────────────────────────────────

    /// Return the last `count` blocks (ascending height), or fewer if chain is shorter.
    pub fn get_recent_blocks(&self, count: u64) -> Result<Vec<Block>, StorageError> {
        let tip = match self.get_tip()? {
            Some((_, h)) => h,
            None => return Ok(vec![]),
        };
        let start = tip.saturating_sub(count.saturating_sub(1));
        let mut blocks = Vec::new();
        for h in start..=tip {
            if let Some(block) = self.get_block_by_height(h)? {
                blocks.push(block);
            }
        }
        Ok(blocks)
    }

    /// Scan all UTXOs and return those belonging to `addr`.
    pub fn scan_utxos_for_address(
        &self,
        addr: &rupeecoin_crypto::Address,
    ) -> Result<Vec<(OutPoint, TxOutput)>, StorageError> {
        let cf = self.cf(CF_UTXOS)?;
        let mut results = Vec::new();
        for item in self.db.iterator_cf(cf, rocksdb::IteratorMode::Start) {
            let (k, v) = item?;
            let output: TxOutput = bincode::deserialize(&v)?;
            if &output.recipient == addr {
                let outpoint: OutPoint = bincode::deserialize(&k)?;
                results.push((outpoint, output));
            }
        }
        Ok(results)
    }

    // ── RVM contract storage ─────────────────────────────────────────────────

    /// Compose the RocksDB key for a contract storage slot.
    fn contract_storage_key(contract_addr: u64, slot: u64) -> [u8; 16] {
        let mut key = [0u8; 16];
        key[..8].copy_from_slice(&contract_addr.to_le_bytes());
        key[8..].copy_from_slice(&slot.to_le_bytes());
        key
    }

    /// Load all storage slots for `contract_addr`, returning a `HashMap<slot, value>`.
    pub fn load_contract_storage(
        &self,
        contract_addr: u64,
    ) -> Result<std::collections::HashMap<u64, u64>, StorageError> {
        use rocksdb::IteratorMode;
        let cf = self.cf(CF_CONTRACT_STORAGE)?;
        let prefix = contract_addr.to_le_bytes();
        let mut map = std::collections::HashMap::new();
        // Iterate from the first key with this prefix.
        for item in self.db.iterator_cf(cf, IteratorMode::From(&prefix, rocksdb::Direction::Forward)) {
            let (k, v) = item?;
            if k.len() != 16 || k[..8] != prefix {
                break; // past this contract's keys
            }
            let slot = u64::from_le_bytes(k[8..16].try_into().unwrap());
            let val = u64::from_le_bytes(
                v.as_ref()
                    .try_into()
                    .map_err(|_| StorageError::Corruption("contract storage value corrupt".into()))?,
            );
            map.insert(slot, val);
        }
        Ok(map)
    }

    /// Persist all storage slots for `contract_addr` atomically.
    pub fn save_contract_storage(
        &self,
        contract_addr: u64,
        storage: &std::collections::HashMap<u64, u64>,
    ) -> Result<(), StorageError> {
        let mut batch = WriteBatch::default();
        let cf = self.cf(CF_CONTRACT_STORAGE)?;
        for (&slot, &val) in storage {
            let key = Self::contract_storage_key(contract_addr, slot);
            batch.put_cf(cf, &key, &val.to_le_bytes());
        }
        self.db.write(batch)?;
        Ok(())
    }

    // ── RVM contract bytecode (CF_CONTRACT_CODE) ─────────────────────────────

    /// Persist the bytecode of a deployed contract.  Key = addr as 8-byte LE.
    pub fn save_contract_code(&self, contract_addr: u64, bytecode: &[u8]) -> Result<(), StorageError> {
        let key = contract_addr.to_le_bytes();
        self.db.put_cf(self.cf(CF_CONTRACT_CODE)?, &key, bytecode)?;
        Ok(())
    }

    /// Load bytecode for `contract_addr`, returning `None` if not deployed.
    pub fn load_contract_code(&self, contract_addr: u64) -> Result<Option<Vec<u8>>, StorageError> {
        let key = contract_addr.to_le_bytes();
        Ok(self.db.get_cf(self.cf(CF_CONTRACT_CODE)?, &key)?)
    }

    // ── Raw meta key/value (for node-layer persistence) ──────────────────────

    /// Store an arbitrary byte blob under `key` in the meta column family.
    pub fn save_raw_meta(&self, key: &[u8], value: &[u8]) -> Result<(), StorageError> {
        self.db.put_cf(self.cf(CF_META)?, key, value)?;
        Ok(())
    }

    /// Load a previously saved byte blob from the meta column family.
    pub fn load_raw_meta(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(self.db.get_cf(self.cf(CF_META)?, key)?)
    }

    // ── Weak subjectivity checkpoints ────────────────────────────────────────

    /// Persist a weak subjectivity checkpoint keyed by block height.
    pub fn save_checkpoint(&self, cp: &WeakSubjectivityCheckpoint) -> Result<(), StorageError> {
        let key = cp.height.to_be_bytes();
        let val = bincode::serialize(cp)?;
        self.db.put_cf(self.cf(CF_CHECKPOINTS)?, &key, &val)?;
        Ok(())
    }

    /// Load all stored checkpoints, sorted by height ascending.
    pub fn load_checkpoints(&self) -> Result<Vec<WeakSubjectivityCheckpoint>, StorageError> {
        let cf = self.cf(CF_CHECKPOINTS)?;
        let mut results = Vec::new();
        for item in self.db.iterator_cf(cf, rocksdb::IteratorMode::Start) {
            let (_, val) = item?;
            let cp: WeakSubjectivityCheckpoint = bincode::deserialize(&val)?;
            results.push(cp);
        }
        // Already in ascending order since keys are big-endian u64.
        Ok(results)
    }
}
