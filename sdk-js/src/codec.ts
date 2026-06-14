/**
 * Minimal bincode v1 writer.
 *
 * Encoding rules (little-endian, fixed-width integers):
 *   u8          → 1 byte
 *   u32         → 4 bytes LE
 *   u64         → 8 bytes LE  (BigInt input)
 *   [u8; N]     → N bytes direct (no length prefix)
 *   Vec<u8>     → u64 LE length + bytes
 *   Vec<T>      → u64 LE length + each element encoded
 *   Option<T>   → 0x00  or  0x01 + T
 *   bool        → 0x00 (false) or 0x01 (true)
 */
export class BincodeWriter {
  private chunks: Uint8Array[] = [];
  private _len = 0;

  writeU8(v: number): void {
    const b = new Uint8Array(1);
    b[0] = v & 0xff;
    this.push(b);
  }

  writeU32(v: number): void {
    const b = new Uint8Array(4);
    new DataView(b.buffer).setUint32(0, v >>> 0, true);
    this.push(b);
  }

  writeU64(v: bigint): void {
    const b = new Uint8Array(8);
    new DataView(b.buffer).setBigUint64(0, BigInt.asUintN(64, v), true);
    this.push(b);
  }

  /** Write raw bytes (no length prefix — used for fixed-size arrays). */
  writeBytes(b: Uint8Array): void {
    this.push(b);
  }

  /** Write a Vec<u8>: u64 length prefix + bytes. */
  writeByteVec(b: Uint8Array): void {
    this.writeU64(BigInt(b.length));
    this.push(b);
  }

  writeBool(v: boolean): void {
    this.writeU8(v ? 1 : 0);
  }

  /** Write Option<T>: call writer only when value is not null. */
  writeOption<T>(v: T | null, writer: (val: T) => void): void {
    if (v === null) {
      this.writeU8(0);
    } else {
      this.writeU8(1);
      writer(v);
    }
  }

  /** Write a Vec<T>: u64 length prefix + each element. */
  writeVec<T>(arr: T[], writer: (el: T) => void): void {
    this.writeU64(BigInt(arr.length));
    for (const el of arr) {
      writer(el);
    }
  }

  toBytes(): Uint8Array {
    const out = new Uint8Array(this._len);
    let offset = 0;
    for (const c of this.chunks) {
      out.set(c, offset);
      offset += c.length;
    }
    return out;
  }

  toHex(): string {
    return Array.from(this.toBytes())
      .map(b => b.toString(16).padStart(2, '0'))
      .join('');
  }

  private push(b: Uint8Array): void {
    this.chunks.push(b);
    this._len += b.length;
  }
}

/** Decode a hex string to Uint8Array. */
export function hexToBytes(hex: string): Uint8Array {
  if (hex.length % 2 !== 0) throw new Error('Odd-length hex string');
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

/** Encode Uint8Array to lowercase hex string. */
export function bytesToHex(b: Uint8Array): string {
  return Array.from(b)
    .map(x => x.toString(16).padStart(2, '0'))
    .join('');
}
