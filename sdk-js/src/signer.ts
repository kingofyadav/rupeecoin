/**
 * Signer interface for pluggable signing backends.
 *
 * Phase 9.3: signing is done externally via `wallet sign-tx` (Rust CLI)
 *             because Dilithium3 WASM is not yet available.
 * Phase 11: a DilithiumSigner backed by WASM will implement this interface.
 */
export interface Signer {
  /** Return the Dilithium3 public key as hex. */
  publicKeyHex(): Promise<string>;

  /**
   * Sign the provided message bytes and return the Dilithium3 signature as hex.
   * The message is the BLAKE3 hash of the serialised transaction inputs + outputs.
   */
  sign(message: Uint8Array): Promise<string>;
}

/**
 * A signer that holds a pre-computed signature from an external Rust wallet.
 *
 * Usage:
 *   const hex = await runShell('rupeecoin-wallet sign-tx ...');
 *   // parse the hex tx, extract pubkey + sig, construct PreSignedSigner
 */
export class PreSignedSigner implements Signer {
  constructor(
    private readonly _pubKeyHex: string,
    private readonly _sigHex: string
  ) {}

  async publicKeyHex(): Promise<string> {
    return this._pubKeyHex;
  }

  async sign(_message: Uint8Array): Promise<string> {
    return this._sigHex;
  }
}
