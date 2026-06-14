/**
 * Transaction builder and bincode serializer.
 *
 * Rust struct layout (bincode v1, little-endian):
 *
 *   Transaction {
 *     version:      u32,
 *     chain_id:     u32,
 *     nonce:        u64,
 *     expiry:       u64,
 *     inputs:       Vec<TxInput>,
 *     outputs:      Vec<TxOutput>,
 *     coinbase_data: Option<Vec<u8>>,
 *   }
 *
 *   TxInput {
 *     outpoint:   OutPoint { txid: [u8; 32], vout: u32 },
 *     public_key: Vec<u8>,
 *     signature:  Vec<u8>,
 *   }
 *
 *   TxOutput {
 *     amount:    u64,
 *     recipient: [u8; 20],
 *   }
 */

import { BincodeWriter, hexToBytes } from './codec.js';
import type { Transaction, TxInput, TxOutput } from './types.js';

function encodeOutPoint(w: BincodeWriter, txid: string, vout: number): void {
  w.writeBytes(hexToBytes(txid));   // [u8; 32]
  w.writeU32(vout);
}

function encodeInput(w: BincodeWriter, inp: TxInput): void {
  encodeOutPoint(w, inp.outpoint.txid, inp.outpoint.vout);
  w.writeByteVec(hexToBytes(inp.public_key));  // Vec<u8>
  w.writeByteVec(hexToBytes(inp.signature));   // Vec<u8>
}

function encodeOutput(w: BincodeWriter, out: TxOutput): void {
  w.writeU64(out.amount);                          // u64
  w.writeBytes(hexToBytes(out.recipient));         // [u8; 20]
}

/**
 * Serialize a Transaction to bincode v1 bytes.
 * The signature fields must already be populated.
 */
export function encodeTx(tx: Transaction): Uint8Array {
  const w = new BincodeWriter();
  w.writeU32(tx.version);
  w.writeU32(tx.chain_id);
  w.writeU64(tx.nonce);
  w.writeU64(tx.expiry);
  w.writeVec(tx.inputs, inp => encodeInput(w, inp));
  w.writeVec(tx.outputs, out => encodeOutput(w, out));
  w.writeOption(tx.coinbase_data, bytes => w.writeByteVec(bytes));
  return w.toBytes();
}

/** Serialize and hex-encode a Transaction — ready for POST /rpc/tx. */
export function encodeTxHex(tx: Transaction): string {
  const bytes = encodeTx(tx);
  return Array.from(bytes)
    .map(b => b.toString(16).padStart(2, '0'))
    .join('');
}

export interface BuildTxParams {
  version?: number;       // default 1
  chain_id?: number;      // default 1
  nonce?: bigint;
  expiry?: bigint;        // default 0n (never)
  inputs: TxInput[];
  outputs: TxOutput[];
}

/** Assemble an unsigned Transaction from parts. */
export function buildUnsignedTx(p: BuildTxParams): Transaction {
  return {
    version: p.version ?? 1,
    chain_id: p.chain_id ?? 1,
    nonce: p.nonce ?? 0n,
    expiry: p.expiry ?? 0n,
    inputs: p.inputs,
    outputs: p.outputs,
    coinbase_data: null,
  };
}
