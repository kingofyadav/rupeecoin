// TypeScript mirror of the Rust core types used in transaction construction.

/** 32-byte BLAKE3 hash, represented as lowercase hex string. */
export type HashHex = string;

/** 20-byte address, represented as Base58Check or lowercase hex string. */
export type AddressHex = string;

/** Reference to a previous unspent transaction output. */
export interface OutPoint {
  txid: HashHex;   // 64-char hex
  vout: number;    // u32
}

/** Spending of one UTXO. */
export interface TxInput {
  outpoint: OutPoint;
  /** Dilithium3 public key bytes as hex string. */
  public_key: string;
  /** Dilithium3 signature bytes as hex string (populated after signing). */
  signature: string;
}

/** Recipient and amount. */
export interface TxOutput {
  /** Amount in micro-RPC (1 RPC = 100_000_000). */
  amount: bigint;
  /** 20-byte recipient address as hex string. */
  recipient: AddressHex;  // 40-char hex
}

/**
 * Full transparent transaction (mirrors Rust Transaction struct exactly).
 *
 * Signing is handled externally (e.g. via `wallet sign-tx`) because
 * Dilithium3 requires WASM — planned for Phase 11.
 */
export interface Transaction {
  version: number;       // u32
  chain_id: number;      // u32  (1=mainnet, 2=testnet)
  nonce: bigint;         // u64
  expiry: bigint;        // u64  (0 = never)
  inputs: TxInput[];
  outputs: TxOutput[];
  /** Present only in coinbase / reward-claim / slash-report txs. */
  coinbase_data: Uint8Array | null;
}

/** A UTXO entry as returned by GET /rpc/utxos. */
export interface UtxoEntry {
  txid: HashHex;
  vout: number;
  amount_micro: bigint;
  amount_rpc: number;
}

/** Node status snapshot from GET /rpc/status. */
export interface NodeStatus {
  height: bigint;
  slot: bigint;
  epoch: bigint;
  last_justified_epoch: bigint;
  last_finalized_epoch: bigint;
  active_validators: number;
  total_stake: bigint;
  mempool_size: number;
  pending_privacy_txs: number;
  commitment_count: bigint;
  base_fee: bigint;
}

/** Validator info from GET /rpc/validator_info. */
export interface ValidatorInfo {
  claimable_rewards: bigint;
  stake: bigint;
  uptime_score: number;
  proposal_score: number;
  total_score: number;
  is_active: boolean;
  status: string;
}
