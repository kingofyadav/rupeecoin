/**
 * Base58Check encode/decode for RupeeCoin addresses.
 *
 * Address format (mirrors rupeecoin_crypto::encode_address):
 *   version byte: 0x12
 *   payload:      20-byte BLAKE3-derived address
 *   checksum:     first 4 bytes of SHA256(SHA256(version + payload))
 *
 * Note: We use SubtleCrypto (Web Crypto API) for SHA-256, so all
 * encode/decode functions are async.
 */

const BASE58_ALPHABET =
  '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';

const BASE58_MAP: Record<string, number> = {};
for (let i = 0; i < BASE58_ALPHABET.length; i++) {
  BASE58_MAP[BASE58_ALPHABET[i]] = i;
}

function base58Encode(buf: Uint8Array): string {
  let num = BigInt(0);
  for (const byte of buf) {
    num = num * 256n + BigInt(byte);
  }

  let result = '';
  while (num > 0n) {
    result = BASE58_ALPHABET[Number(num % 58n)] + result;
    num = num / 58n;
  }

  for (const byte of buf) {
    if (byte !== 0) break;
    result = '1' + result;
  }

  return result;
}

function base58Decode(s: string): Uint8Array {
  let num = BigInt(0);
  for (const ch of s) {
    const digit = BASE58_MAP[ch];
    if (digit === undefined) throw new Error(`Invalid Base58 char: '${ch}'`);
    num = num * 58n + BigInt(digit);
  }

  const bytes: number[] = [];
  while (num > 0n) {
    bytes.unshift(Number(num & 0xffn));
    num >>= 8n;
  }

  for (const ch of s) {
    if (ch !== '1') break;
    bytes.unshift(0);
  }

  return new Uint8Array(bytes);
}

async function sha256(data: Uint8Array): Promise<Uint8Array> {
  const copy = new Uint8Array(data).buffer as ArrayBuffer;
  const buf = await crypto.subtle.digest('SHA-256', copy);
  return new Uint8Array(buf);
}

async function checksum(payload: Uint8Array): Promise<Uint8Array> {
  const first = await sha256(payload);
  const second = await sha256(first);
  return second.slice(0, 4);
}

/** VERSION byte used for RupeeCoin transparent addresses. */
const ADDRESS_VERSION = 0x12;

/**
 * Encode a 20-byte address to Base58Check string.
 * Equivalent to `rupeecoin_crypto::encode_address`.
 */
export async function encodeAddress(addrBytes: Uint8Array): Promise<string> {
  if (addrBytes.length !== 20) throw new Error('Address must be 20 bytes');
  const payload = new Uint8Array(21);
  payload[0] = ADDRESS_VERSION;
  payload.set(addrBytes, 1);
  const cs = await checksum(payload);
  const full = new Uint8Array(25);
  full.set(payload);
  full.set(cs, 21);
  return base58Encode(full);
}

/**
 * Decode a Base58Check address string to 20-byte Uint8Array.
 * Throws if version or checksum is invalid.
 */
export async function decodeAddress(b58: string): Promise<Uint8Array> {
  const full = base58Decode(b58);
  if (full.length !== 25) throw new Error('Invalid address length');
  if (full[0] !== ADDRESS_VERSION) throw new Error('Wrong address version');
  const payload = full.slice(0, 21);
  const cs = await checksum(payload);
  for (let i = 0; i < 4; i++) {
    if (full[21 + i] !== cs[i]) throw new Error('Address checksum mismatch');
  }
  return full.slice(1, 21);
}

/**
 * Parse an address from either Base58Check or 40-char hex.
 * Returns 20 raw bytes.
 */
export async function parseAddress(s: string): Promise<Uint8Array> {
  if (/^[0-9a-fA-F]{40}$/.test(s)) {
    const bytes = new Uint8Array(20);
    for (let i = 0; i < 20; i++) {
      bytes[i] = parseInt(s.slice(i * 2, i * 2 + 2), 16);
    }
    return bytes;
  }
  return decodeAddress(s);
}

/** Convert 20 raw address bytes to lowercase hex. */
export function addressToHex(addrBytes: Uint8Array): string {
  return Array.from(addrBytes)
    .map(b => b.toString(16).padStart(2, '0'))
    .join('');
}
