# ₹ The RupeeCoin Handbook

> **From India. For the World. Owned by No One.**
> A post-quantum, privacy-native Layer-1 blockchain written from scratch in Rust.

This is the single door into RupeeCoin. It is written for three very different
readers, and you only need to read your own track:

| If you are a… | Read | You will learn |
|---|---|---|
| 🧑‍💻 **User** | [Part A — Using RupeeCoin](#part-a--using-rupeecoin-for-users) | How to hold, receive and send RPC with the wallets, faucet and explorer — no code. |
| 🛠️ **Developer** | [Part B — Building on RupeeCoin](#part-b--building-on-rupeecoin-for-developers) | How to run a node, drive the CLI, call the RPC API, use the SDKs, and stand up a local network. |
| 📈 **Investor / Contributor** | [Part C — Why RupeeCoin](#part-c--why-rupeecoin-for-investors--contributors) | The thesis, what is actually built today, the moat, the risks, and how to do diligence. |

Everyone should skim the 60-second overview first.

---

## RupeeCoin in 60 seconds

RupeeCoin is a base-layer digital money network — like Bitcoin or Ethereum — but
engineered for the next 50 years instead of the last 15:

- **Quantum-safe from birth.** Every signature is CRYSTALS-Dilithium3 and every
  encrypted note is CRYSTALS-Kyber768 — both NIST post-quantum standards. There is
  no ECDSA or RSA anywhere to be broken by a future quantum computer.
- **Private by design.** Alongside ordinary transparent transactions, RupeeCoin has
  a *shielded pool* where amounts and recipients are hidden using zk-STARK proofs
  (no trusted setup, quantum-safe).
- **Proof-of-Stake, not Proof-of-Work.** Casper-FFG finality with VRF-chained
  proposer selection. No mining farms, no energy waste.
- **Programmable.** The RupeeCoin VM (RVM) runs smart contracts with RPC-20 / 721 /
  1155 token standards and an EVM→RVM transpiler.
- **Built for a billion.** Designed around the reality that India already runs ~10
  billion UPI payments a month. Scale is a first-class requirement, not an afterthought.

One coin is **1 RPC = 100,000,000 micro-RPC** (the smallest unit).

**Status honesty (read this):** RupeeCoin is *feature-complete in code* and covered by
**171 passing automated tests**, but it has **not** completed an external security audit
and is **not** running a live public mainnet. It is a serious, working engineering
artifact — not yet a production money network. Part C is explicit about this.

---

# Part A — Using RupeeCoin (for users)

You do not need to understand cryptography to use RupeeCoin, any more than you need to
understand TCP to send an email. This part uses plain language.

## A.1 The three ideas you actually need

1. **A wallet is a keychain.** It holds your secret key. Whoever has the key controls
   the money. RupeeCoin wallets encrypt that key with a passphrase you choose and store
   it in a file. **If you lose the passphrase and the master seed, the money is gone
   forever.** Nobody can reset it — that is the whole point of "owned by no one."

2. **You have two kinds of address.**
   - A **transparent address** (looks like `a2wH3gDYdrFtJStBV8EveZAfs3qcqk2DuL`) —
     normal, public, traceable, like a bank account number on a public ledger.
   - A **shielded / private payment address** (a long hex string) — used for private
     transactions where the amount and recipient are hidden.
   Use transparent for everyday visible payments; use shielded when you want privacy.

3. **A payment link is just an address in a URI.** Anyone can pay you by scanning or
   clicking `rupeecoin:<your-address>?amount=5&label=Coffee`.

## A.2 Getting coins on the test network (the faucet)

On the test network, a **faucet** hands out free test RPC so you can try everything
without buying anything.

1. Open the faucet in your browser (default **http://127.0.0.1:8081** when self-hosted,
   or the public testnet faucet URL when one is announced).
2. Paste your transparent address into the box.
3. Click **Request**. Within a few seconds a transaction is sent and your balance
   appears once the next block is produced.

Faucets are rate-limited (by default one drip per IP per 24 hours) so everyone gets a turn.

## A.3 The desktop wallet (recommended for most users)

The desktop wallet is a normal application window — create a wallet, see your balance,
send and receive, all with buttons.

**Install (Linux example):**
```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev
cd desktop-wallet
cargo build --release
./target/release/rupeecoin-desktop-wallet
```

**First run:**
1. **Create wallet** → choose a strong passphrase → **write down the master seed shown
   and store it offline.** This seed is the only way to recover your money on a new device.
2. **Unlock** with your passphrase whenever you reopen the app.
3. **Receive** tab shows your address and a copyable `rupeecoin:` link — share it to get paid.
4. **Send** tab: paste the recipient address, enter an amount, confirm. The wallet picks
   coins, signs with your key, and broadcasts. The balance refreshes automatically (~15s).
5. **Settings**: point the wallet at your node's RPC URL (default `http://127.0.0.1:9944`)
   and the correct chain ID.

Everything is stored **encrypted at rest** (Argon2id + XChaCha20-Poly1305). The app never
sends your key anywhere.

## A.4 The mobile wallet (Android)

A Flutter + Rust Android wallet. Today it supports **create / unlock / receive / portfolio /
settings** — sending is being finalized as the mobile zero-knowledge prover is optimized.

- **Portfolio**: balance and number of coins.
- **Receive**: a QR code and a `rupeecoin:<addr>` link — let someone scan to pay you.
- **Settings**: RPC URL, chain ID, and node status.
- It registers the `rupeecoin:` link scheme, so tapping a payment link opens the wallet.

Package: `network.rupeecoin.wallet`, Android 7.0+ (arm64 / x86_64).

## A.5 The block explorer (look up anything)

The explorer is a website that lets anyone inspect the chain — no wallet needed
(default **http://127.0.0.1:8080** when self-hosted).

- Search a **block height**, a **transaction id**, or an **address**.
- An address page shows its balance and unspent coins.
- Recent blocks stream on the front page.

Use it to confirm a payment arrived: search your address and watch the balance update.

## A.6 Staying safe — the five rules

1. **Back up your master seed offline** (paper, metal, or an encrypted USB). Test that
   you can restore before you hold anything meaningful.
2. **Never type your passphrase or seed into a website.** Real wallets ask locally.
3. **Verify the address** before sending — transactions are final and irreversible.
4. **Use a fresh receive address** for each payment when you want privacy (the wallet's
   "receive" action gives you a new one).
5. **On testnet, coins are worthless by design** — practice freely here first.

---

# Part B — Building on RupeeCoin (for developers)

This is the fast path from clone to a running local network you can transact against.

## B.1 Prerequisites

- **Rust** 1.75+ stable — `rustup update stable`
- **A C compiler** (`gcc` or `clang`) — required for the Dilithium3 seeded-keypair shim
  and for RocksDB. On Debian/Ubuntu: `sudo apt install build-essential clang libclang-dev`.
  > Troubleshooting: if a build fails with `could not exec the linker cc: Permission
  > denied (os error 13)`, your toolchain binaries lost their execute bit. Fix with
  > `sudo chmod 755 /usr/bin/x86_64-linux-gnu-{gcc-13,as,ld.bfd,g++-13}` (adjust versions).

## B.2 Build and test

```bash
git clone https://github.com/rupeecoin/rupeecoin
cd rupeecoin
cargo build --release          # binaries land in target/release/
cargo test  --workspace        # 171 tests, 0 failures expected
```

Key binaries produced:

| Binary | Role |
|---|---|
| `rupeecoin-node` | Full node: P2P, consensus, block production, JSON-RPC server |
| `rupeecoin-wallet` | CLI wallet: HD keys, shielded pool, transparent `sign-tx` |
| `rupeecoin-faucet` | Testnet faucet web service |
| `rupeecoin-explorer` | Block explorer REST API + SPA |
| `rupeecoin-dashboard` | Validator monitoring TUI |
| `evm2rvm` | EVM→RVM bytecode transpiler |

## B.3 A local devnet in five minutes

This produces a single-validator chain you can fund and transact on. (It mirrors the
verified `local-testnet/` setup in this repo.)

```bash
# 1. Start a validator node on a fresh data dir. A fixed --validator-seed gives you a
#    stable, spendable coinbase address. Genesis defaults to mainnet => chain_id 1.
VSEED=$(printf 'cc%.0s' {1..32})   # 64 hex chars = 32-byte seed
target/release/rupeecoin-node \
  --data-dir ./devnet --validator-seed "$VSEED" \
  --stake 10000 --block-interval 1 --rpc-port 9944 \
  --listen /ip4/127.0.0.1/tcp/30333 &

# 2. Create a wallet. rpassword reads the passphrase from the TTY, so type it when asked
#    (for scripted/non-interactive use, drive it through a PTY — see local-testnet/pw.py).
target/release/rupeecoin-wallet --keys ./devnet-wallet.json generate

# 3. Check the chain is producing blocks and read constants.
curl -s http://127.0.0.1:9944/rpc/height
curl -s http://127.0.0.1:9944/rpc/status
```

**Funding a wallet from the validator.** Coinbase rewards (47.5 RPC/block) mature after
`COINBASE_MATURITY = 100` blocks. Once height > 100, spend a known-mature early-block
coinbase to any address with the `fund_direct` example (it fetches the correct nonce):

```bash
# Find an early block's coinbase txid via the explorer, then:
target/release/examples/fund_direct \
  --txid <early_coinbase_txid> --vout 0 \
  --to <your_wallet_transparent_addr> --amount 45 \
  --seed "$VSEED" --node http://127.0.0.1:9944 --chain-id 1
```

> Note: the `fund_faucet` example hardcodes `nonce: 0`, so it only works for the
> validator's *first* spend and does not filter immature coinbases. Prefer `fund_direct`
> for repeatable funding.

**Restarting a node.** Nodes persist their chain and validator registry, so you can stop
and restart on the same `--data-dir`; the node replays local blocks and continues.

## B.4 Node CLI reference

```
rupeecoin-node [FLAGS]
  --data-dir <path>            Chain data dir                 [default: ./rupeecoin-data]
  --listen <multiaddr>         libp2p TCP listen              [default: /ip4/0.0.0.0/tcp/30333]
  --peer <multiaddr>           Dial a peer on startup (repeatable)
  --stake <RPC>                Whole-coin stake to validate   [min: 10000]
  --rpc-port <port>            JSON-RPC HTTP port             [default: 9944]
  --no-rpc                     Disable the RPC server
  --block-interval <secs>      Slot duration                  [default: 4]
  --validator-seed <64-hex>    Deterministic validator keypair (stable address for testnets)
  --checkpoint <h:hash:state>  Weak-subjectivity fast-sync boundary
  --tor-proxy <ip:port>        Route outbound P2P via a Tor SOCKS5 proxy
  --genesis-config <path>      Load genesis params from JSON (overrides built-in mainnet)
  --genesis-timestamp <secs>   Override genesis block timestamp
  --treasury-address <40-hex>  Override the treasury address
  -h | --help                  Print the flag reference
```

## B.5 Wallet CLI reference

```
rupeecoin-wallet [--keys <path>] <COMMAND>       # default keystore: wallet.json
```

| Command | What it does |
|---|---|
| `generate` | Create a new HD + privacy key bundle; encrypt with Argon2id + XChaCha20-Poly1305. |
| `address` | Print current transparent + shielded addresses (does not advance HD index). |
| `receive` | Advance the HD receive index and print a fresh address. |
| `scan --rpc <url>` | Trial-decrypt the shielded pool to find notes owned by this wallet. |
| `send --to <addr\|uri> --amount <RPC> [--fee] [--rpc]` | Build + broadcast a **shielded** tx (needs shielded notes). |
| `sign-tx --to <b58> --amount <RPC> [--chain-id] [--expiry] [--rpc]` | Build + sign a **transparent** tx; prints signed hex to POST to `/rpc/tx`. |
| `claim --rpc <url>` | Claim accrued validator rewards. |
| `upgrade-hd` | Add an HD seed to a legacy keystore, re-encrypting it. |
| `--hardware-wallet ledger` | Sign with a connected Ledger (Nano S/X/S+/Stax) via Linux hidraw. |

> The keystore passphrase is read from the controlling terminal via `rpassword`; it
> ignores piped stdin. To automate wallet commands, run them under a pseudo-terminal.

## B.6 HTTP JSON-RPC API

Served on `127.0.0.1:9944` (configurable). All `GET` except the two submit endpoints.

| Endpoint | Returns |
|---|---|
| `GET /rpc/height` | `{ "height": 42 }` |
| `GET /rpc/status` | `{ height, slot, epoch, last_justified_epoch, last_finalized_epoch, active_validators, total_stake, mempool_size, pending_privacy_txs, commitment_count, base_fee }` |
| `GET /rpc/utxos?addr=<hex\|b58>` | `[{ txid, vout, amount_micro, amount_rpc, amount_rpc_whole, amount_rpc_frac }]` |
| `GET /rpc/nonce?addr=<hex\|b58>` | `{ "nonce": 3 }` |
| `GET /rpc/validator_info?addr=<hex>` | `{ claimable_rewards, stake, uptime_score, proposal_score, total_score, is_active, status }` |
| `GET /rpc/recent_roots` | `{ roots: [...], commitment_count }` — anchors for shielded spends |
| `GET /rpc/shielded_outputs?start=&count=` | Paginated encrypted notes for wallet scanning |
| `GET /rpc/merkle_path?index=<n>` | Authentication path to build a `SpendProof` |
| `GET /rpc/nullifier_status?hex=<64-hex>` | `{ "spent": false }` |
| `POST /rpc/tx` | Submit a transparent tx (hex `bincode(Transaction)`) → `{ "status": "accepted" }` |
| `POST /rpc/privacy_tx` | Submit a shielded tx (hex `bincode(PrivacyTransaction)`) |

Example — build and broadcast a transparent transfer end to end:
```bash
# wallet signs and prints hex; capture the final hex line and submit it
target/release/rupeecoin-wallet --keys ./devnet-wallet.json \
  sign-tx --rpc http://127.0.0.1:9944 --to <recipient_b58> --amount 25 --chain-id 1
curl -s -X POST http://127.0.0.1:9944/rpc/tx \
  -H 'Content-Type: application/octet-stream' --data "<signed_hex>"
```

## B.7 Companion services

**Faucet** — `rupeecoin-faucet --port 8081 --node-rpc http://127.0.0.1:9944 --keyfile faucet-key.json --chain-id 1 --drip-rpc 100`
- `GET /` web UI, `POST /drip {"address":"<b58>"}`, `GET /api/stats`.
- Fund the faucet's own address first (from a validator coinbase) before it can dispense.

**Explorer** — `rupeecoin-explorer --data-dir ./devnet --secondary-path ./exp-sec --port 8080`
- Opens RocksDB as a *secondary* instance so it can read while the node holds the write lock.
- REST: `/api/height`, `/api/recent`, `/api/block/:height`, `/api/block/hash/:hash`,
  `/api/tx/:txid`, `/api/address/:addr`, `/api/nullifier/:hex`, `/api/search`.

**Dashboard** — `rupeecoin-dashboard --rpc http://127.0.0.1:9944 --addr <validator_b58_or_40hex>`
- A live TUI. `--addr` must be a 40-char hex or Base58Check address (not the 64-char
  registry pubkey hash), or it will show "Disconnected".

## B.8 SDKs

| SDK | Location | Use it for |
|---|---|---|
| **Rust** | `rupeecoin-sdk/` | `RpcClient`, `Wallet`, `build_transfer` — embed in Rust services. |
| **TypeScript / JS** | `sdk-js/` | bincode codec, RPC client, tx builder, signer, browser `demo.html`. |
| **Python** | `sdk-py/` | PyO3 + maturin wheel — scripting, data pipelines, bots. |

Each SDK speaks the same JSON-RPC described in B.6. The Rust and Python SDKs expose a
`UtxoEntry` with an `amount_rpc: f64` field that the node now returns directly.

## B.9 Contributing workflow

```bash
cargo build --workspace
cargo test  --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all
```

Commit messages are imperative (`Add X`, not `Added X`); use `feat/`, `fix/`, `chore/`
branch prefixes. See `GUIDE.txt` for the exhaustive CLI/RPC reference and `SECURITY.md`
for responsible disclosure.

---

# Part C — Why RupeeCoin (for investors & contributors)

This section is written to help you decide whether RupeeCoin deserves your **time,
energy, or capital of effort**. It is deliberately honest about both the upside and the
gaps. *Nothing here is financial advice, and there is no token sale.*

## C.1 The thesis in one paragraph

The two blockchains that matter most — Bitcoin and Ethereum — were designed before
quantum computing was a credible threat, before privacy was understood as a default
requirement, and before a single country demonstrated real-time payments at billion-user
scale. RupeeCoin is a bet that the *next* base-layer network wins on the axes the
incumbents cannot retrofit cheaply: **post-quantum security, default privacy, and
throughput designed for mass adoption** — shipped as one coherent system rather than
bolted-on patches.

## C.2 Why the timing is not arbitrary

- **Quantum risk is now a "when," not an "if."** NIST finalized post-quantum standards
  (including CRYSTALS-Dilithium and Kyber) precisely because "harvest now, decrypt later"
  is a live threat to long-lived value. A chain built on Dilithium3/Kyber768 from genesis
  has no migration cliff. Bitcoin and Ethereum face a hard, contentious fork to get there.
- **Privacy is becoming table stakes**, both for individuals and for institutions who
  cannot expose their entire transaction graph to competitors.
- **The scale proof exists.** India's UPI shows that a billion people *will* use
  real-time digital payments when the UX is right. RupeeCoin's three-layer design treats
  that as the target, not a distant dream.

## C.3 What is actually built today (verified)

This is not vaporware. As of this handbook, the following are implemented **and covered
by 171 passing automated tests**:

| Area | Status |
|---|---|
| Post-quantum crypto (Dilithium3 signing, Kyber768 notes, BLAKE3) | ✅ Working |
| UTXO settlement layer + per-block state root | ✅ Working |
| PoS consensus: Casper-FFG finality, VRF proposer selection, LMD-GHOST | ✅ Working |
| Slashing (equivocation / double-vote), inactivity leak, unstaking delay | ✅ Working |
| Shielded privacy pool: notes, nullifiers, commitment tree, Kyber-encrypted notes | ✅ Working |
| zk-STARK spend proofs (Winterfell), batched verification, Poseidon tree | ✅ Working |
| Economics: EIP-1559 base fee, PMFP public-goods fund, halving emission | ✅ Working |
| RupeeCoin VM: interpreter, gas metering, RPC-20/721/1155, EVM→RVM transpiler | ✅ Working |
| P2P: libp2p gossipsub, request-response sync, Dandelion++, Tor/SOCKS5 | ✅ Working |
| Full node persistence + clean restart (chain replay + registry) | ✅ Working |
| Tooling: CLI wallet, desktop wallet (Tauri), Android wallet, explorer, faucet, dashboard | ✅ Working |
| SDKs in Rust, TypeScript, and Python | ✅ Working |
| Ledger hardware-wallet signing (Linux hidraw) | ✅ Working |

**Engineering discipline signal:** the project has already run *multiple internal audit
passes* (labeled A–E) resolving dozens of Critical/High/Medium findings, and the codebase
is a single Rust workspace where a full `cargo test --workspace` is green. That is the
posture of a team that treats correctness as the product.

## C.4 How it compares

| Property | Bitcoin | Ethereum | RupeeCoin |
|---|---|---|---|
| Quantum-resistant signatures | ❌ | ❌ | ✅ Dilithium3 from genesis |
| Privacy by default | ❌ | ❌ | ✅ shielded pool + zk-STARKs |
| Smart contracts | ❌ | ✅ | ✅ RVM + token standards + EVM transpiler |
| Energy profile | ⛏️ PoW | ✅ PoS | ✅ PoS |
| Trusted setup for ZK | N/A | ⚠️ (some systems) | ✅ none (STARKs) |
| Perpetual validator incentive | ❌ (fee-cliff risk) | ⚠️ | ✅ PMFP public-goods fund |
| Founder control (end goal) | None | None | None (ownerless by design) |

## C.5 The moat

1. **Post-quantum is a from-scratch advantage, not a feature.** Retrofitting PQ signatures
   into a UTXO/account model with billions in value is a coordination nightmare for
   incumbents. RupeeCoin pays that cost once, at genesis.
2. **Privacy + programmability + PQ in one stack.** Zcash has privacy; Ethereum has
   contracts; neither is post-quantum. Holding all three simultaneously is rare.
3. **No trusted setup anywhere.** BLAKE3 and STARKs need no ceremony — a real trust and
   auditability advantage over SNARK-based privacy chains.
4. **Real, readable Rust.** Memory-safe, single-workspace, test-covered. The barrier to
   *auditing and contributing* is low, which compounds decentralization over time.

## C.6 Economics at a glance

- **Denomination:** 1 RPC = 100,000,000 micro-RPC.
- **Emission:** decreasing (halving-style) block rewards; **no pre-mine** — the treasury
  is funded from a fixed split of each coinbase, not a founder allocation.
- **Fee market:** EIP-1559 base fee (burned) + priority fee to validators; 30% of base
  fees route to the **PMFP** public-goods fund, distributed to validators by contribution
  score each epoch — a sustainable answer to Bitcoin's long-term fee-cliff problem.
- **Staking:** 10,000 RPC minimum; rewards weighted by a **Proof-of-Contribution** score
  (uptime, attestation timeliness, proposal timeliness, propagation quality), not raw stake alone.

## C.7 Roadmap and the milestones that matter

The protocol phases (whitepaper 0–10 and engineering phases 1–15 + audit passes A–E) are
tracked in `README.md` and `whitepaper/README.md`. The milestones an investor of time or
capital should watch, in order:

1. **External security audit(s) passed** — the single biggest de-risking event.
2. **Public testnet (Zeta): 100+ external validators finalizing across regions for 30 days.**
3. **Bug-bounty program live** with real payouts and no criticals outstanding.
4. **Mainnet genesis** with a credible, decentralized launch validator set.
5. **Ecosystem traction:** grants cohort shipped, real dApps on RVM, wallet installs.

## C.8 Honest risk assessment

Invest your time or energy with eyes open:

- **Pre-audit.** No independent security firm has signed off yet. Treat all guarantees as
  "tested, not yet audited." This is the dominant risk.
- **Pre-mainnet / pre-network-effect.** A base-layer chain's value is its validator set,
  liquidity, and developers — none of which exist at scale yet. Bootstrapping that is hard
  and most L1s fail at it.
- **Post-quantum signatures are large.** Dilithium3 signatures (~3.3 KB) mean bigger
  blocks and higher bandwidth than ECDSA chains — a real scalability tax the design must
  keep managing.
- **Privacy invites scrutiny.** Default-privacy money faces regulatory headwinds in many
  jurisdictions; selective-disclosure viewing keys are a mitigation, not a guarantee.
- **Cryptographic novelty.** Some constructions (seeded-Dilithium C shim, STARK circuits,
  the fee/PMFP model) are bespoke and need adversarial review before carrying value.
- **Execution risk.** "Owned by no one" is a strength and a fundraising/coordination
  challenge — there is no company balance sheet behind it.

If any of these are dealbreakers for you, that is the correct signal to wait for C.7's
milestones before committing more than curiosity.

## C.9 How to get involved

You do not need money to invest in RupeeCoin — the most valuable contributions right now
are **review and code**:

- **Auditors & cryptographers:** attack the consensus, the STARK circuits, and the
  seeded-key derivation. Responsible disclosure via `SECURITY.md`.
- **Rust / distributed-systems engineers:** the workspace is clean and idiomatic; pick up
  a roadmap carry-over.
- **App & wallet builders:** the SDKs (Rust/JS/Python) and RVM token standards are ready
  to build against today on a local devnet (Part B.3).
- **Validators:** run a node, learn the staking flow, and be ready for Zeta.
- **Writers & educators:** documentation and onboarding is how a billion-user network
  actually reaches a billion users.

Diligence checklist before you go deep: read the [whitepaper](whitepaper/README.md), run
`cargo test --workspace` yourself, stand up the Part B.3 devnet, read `SECURITY.md` and
the `audit/` folder, and watch for the C.7 milestones.

---

## Appendix — quick reference

**Denominations**

| Unit | Value |
|---|---|
| 1 RPC | 100,000,000 micro-RPC |
| 1 micro-RPC | smallest indivisible unit |

**Default ports**

| Service | Port |
|---|---|
| Node JSON-RPC | 9944 |
| Node P2P (libp2p) | 30333 |
| Faucet web | 8081 |
| Explorer web | 8080 |

**Selected protocol constants**

| Constant | Value |
|---|---|
| Slot duration | 4 s (override on devnets) |
| Slots per epoch | 32 |
| Minimum stake | 10,000 RPC |
| Coinbase maturity | 100 blocks |
| Anchor depth (shielded) | 100 roots |
| Unstaking delay | 604,800 slots (~28 days) |
| Reorg depth limit | 32 blocks |

**Glossary**

- **UTXO** — Unspent Transaction Output; the "coins" you own.
- **Shielded pool** — the private side of the ledger (hidden amounts/recipients).
- **Nullifier** — a one-time tag that lets the network prevent double-spends without
  revealing which note was spent.
- **Casper FFG** — the finality gadget: two consecutively justified epochs = final.
- **VRF** — Verifiable Random Function; picks the next block proposer unpredictably.
- **PMFP** — Public Monetary Fund Protocol; the on-chain public-goods fund fed by base fees.
- **RVM** — the RupeeCoin Virtual Machine for smart contracts.

---

> **RupeeCoin — From India, For the World, Owned by No One.**
> See also: [`README.md`](README.md) (technical overview), [`GUIDE.txt`](GUIDE.txt)
> (exhaustive CLI/RPC/SDK reference), [`whitepaper/`](whitepaper/README.md) (protocol
> specification), and [`SECURITY.md`](SECURITY.md) (disclosure policy).
