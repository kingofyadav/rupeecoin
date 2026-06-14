# Security Policy

## Supported Versions

RupeeCoin is currently in **public testnet** (Phase 10 — Zeta). The mainnet has not launched yet.

| Version | Supported |
|---|---|
| `0.x.y` (pre-testnet) | No — superseded |
| `0.9.x` (public testnet) | **Yes — active** |
| `1.0.0` (mainnet) | Not yet launched |

---

## Responsible Disclosure

**Please do not open a public GitHub issue for security vulnerabilities.**

Report vulnerabilities privately:

- **Email:** security@rupeecoin.network  *(replace with real address before launch)*
- **PGP fingerprint:** `DEAD BEEF …`  *(replace with real key before launch)*

We will:

1. Acknowledge your report within **48 hours**.
2. Confirm the vulnerability and its scope within **5 business days**.
3. Provide a fix or mitigation timeline within **10 business days**.
4. Credit you in the release notes unless you prefer to remain anonymous.

Do **not** disclose publicly until we have shipped a fix or we have mutually agreed on a disclosure date.

---

## Bug Bounty Program

RupeeCoin runs a bug bounty program for the public testnet. Rewards are paid in RPC once the mainnet launches. Testnet-only findings may receive early community tokens at discretion.

### Scope

**In scope:**

| Component | Crate(s) |
|---|---|
| Core consensus | `core/consensus`, `core/staking` |
| Block validation | `core/blockchain`, `core/utxo` |
| Cryptography | `core/crypto` |
| Privacy layer | `core/privacy`, `core/zkproof` |
| Economics / fee market | `core/economics` |
| Node RPC | `node/src/rpc.rs` |
| P2P networking | `core/network` |
| Storage | `core/storage` |
| Wallet CLI | `wallet/` |
| Rust SDK | `rupeecoin-sdk/` |

**Out of scope:**

- Informational findings without demonstrated impact
- Denial-of-service attacks requiring large hardware resources
- Social engineering or phishing
- Findings in third-party dependencies (report upstream)
- UI cosmetic issues in the explorer, dashboard, or faucet

### Reward Tiers

| Severity | Reward |
|---|---|
| **Critical** — remote code execution, private key extraction, consensus break, double-spend | **$10,000 USD equivalent in RPC** |
| **High** — significant fund loss, forced fork, major privacy break | **$2,500 USD equivalent in RPC** |
| **Medium** — DoS vector with moderate effort, minor privacy leak, economic manipulation | **$500 USD equivalent in RPC** |
| **Low** — minor issues with limited impact | Credited in release notes |

### Severity Guidelines

**Critical:**
- Exploit that extracts a validator's private Dilithium3 key
- Double-spend attack that bypasses UTXO nonce or state root checks
- Consensus split that permanently forks the chain
- Ability to mint new RPC beyond the supply schedule
- Breaking the Casper FFG finality guarantee

**High:**
- Forced chain halt affecting all validators
- Breaking shielded pool nullifier uniqueness (double-spend on privacy side)
- Stealing another user's transparent UTXO without their key
- Bypassing the minimum stake requirement to participate as a validator

**Medium:**
- Linking a shielded spend to its deposit (deanonymization)
- Crashing a node via a malformed network message
- Causing incorrect fee calculation that benefits an attacker

### Rules

1. Do not test on mainnet (not yet live). Test on the Zeta testnet or a local node.
2. Do not leak user funds or private testnet validator keys.
3. Provide a minimal reproducible proof-of-concept (PoC).
4. Give us reasonable time to fix before any public disclosure.
5. Act in good faith — we will do the same.

---

## Security Architecture

For background on the security model, see:

- [README.md — Security Model](./README.md#security-model)
- [GUIDE.txt — Part 9, Cryptographic Primitives Reference](./GUIDE.txt)
- [Whitepaper — Section 12, Security Analysis](./whitepaper/sections/12-security.md)

### Known Limitations (not bugs)

| Limitation | Impact | Phase fix |
|---|---|---|
| STARK circuit proves Poseidon shadow tree (not BLAKE3 chain tree) | Spend-to-deposit linkability not fully closed | Phase 11 |
| LMD-GHOST computed but chain rollback not implemented | Fork resolution under partition unproven | Phase 10 |
| Testnet only — no mainnet contract guarantees | Testing environment | Phase 13 |

---

## Cryptographic Standards

RupeeCoin uses only post-quantum primitives:

| Algorithm | Standard | Use |
|---|---|---|
| CRYSTALS-Dilithium3 | NIST FIPS 204 | Signatures, VRF |
| CRYSTALS-Kyber768 | NIST FIPS 203 | Note encryption KEM |
| BLAKE3 | — | Hashing, HD derivation, state root |
| XChaCha20-Poly1305 | RFC 8439 variant | Symmetric encryption |
| Argon2id | RFC 9106 | Keystore KDF |
| Winterfell STARK | — | zk spend proofs |

No ECDSA, RSA, or other classical-only algorithms are used.

---

*From India. For the World. Owned by No One.*
