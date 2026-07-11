# Running a Validator on the Zeta Public Testnet

> Roadmap deliverable §15.2. This guide takes you from a fresh Linux machine to a
> staked, block-producing validator on the Zeta testnet.
>
> Zeta is a **test network**: coins are worthless by design, and the chain may be
> reset with two weeks' notice (see the versioning policy in [`ROADMAP.md`](../ROADMAP.md)).

## Zeta network parameters

These values are published at each network (re)launch. Always take them from the
official announcement channel, never from a third party — the checkpoint hash is
what protects you from being synced onto a fake chain.

| Parameter | Value |
|---|---|
| Chain ID | announced at launch |
| Genesis config | `genesis-zeta.json`, published with the launch announcement |
| Bootnodes | published multiaddrs, e.g. `/ip4/<ip>/tcp/30333/p2p/<PeerId>` |
| Weak-subjectivity checkpoint | published `height:block_hash:state_root` triple |
| Minimum stake | 10,000 RPC |
| Slot duration | 4 s · 32 slots per epoch |

## 1. Hardware and network requirements

- Linux x86_64 (Ubuntu 22.04/24.04 tested), 4+ cores, 8 GB RAM, 100 GB SSD.
- A public, stable IP with TCP port **30333** reachable from the internet.
- Dilithium3 signatures are ~3.3 KB, so budget more bandwidth than an ECDSA
  chain: 10 Mbps sustained is a comfortable floor.
- Keep the RPC port (**9944**) firewalled to localhost unless you know why you
  are exposing it. The node binds it on 127.0.0.1 by default.

## 2. Build the node

```bash
sudo apt install build-essential clang libclang-dev
git clone https://github.com/rupeecoin/rupeecoin
cd rupeecoin
cargo build --release
cargo test --workspace        # optional but recommended: expect 0 failures
```

Binaries land in `target/release/`. You need `rupeecoin-node`,
`rupeecoin-wallet`, and optionally `rupeecoin-dashboard`.

## 3. Create your validator identity

A validator needs a keypair whose address holds the stake. Generate a wallet and
back up the master seed offline:

```bash
target/release/rupeecoin-wallet --keys ./validator-wallet.json generate
target/release/rupeecoin-wallet --keys ./validator-wallet.json address
```

For a **deterministic node identity** (same address across restarts and
machines), derive the node's validator key from a 32-byte seed you control:

```bash
VSEED=$(openssl rand -hex 32)      # store this somewhere safe — it IS the validator key
```

Anyone with this seed can sign blocks as you and get you slashed for
equivocation. Treat it like the wallet seed: offline backup, never in shell
history on shared machines (`export VSEED` from a mode-600 file instead).

## 4. Fund the stake

Minimum stake is **10,000 RPC**. On Zeta you get it for free:

1. Request test coins from the public faucet announced with the launch
   (the faucet drips a fixed amount per address per 24 h, so start early), or
2. Ask in the validator channel — existing validators can send you stake.

Confirm the balance via the explorer or:

```bash
curl -s "http://127.0.0.1:9944/rpc/utxos?addr=<your_address>"   # once your node is synced
```

## 5. Start the node

```bash
target/release/rupeecoin-node \
  --data-dir /var/lib/rupeecoin-zeta \
  --validator-seed "$VSEED" \
  --stake 10000 \
  --genesis-config genesis-zeta.json \
  --checkpoint <height>:<block_hash_hex>:<state_root_hex> \
  --peer <bootnode_multiaddr_1> \
  --peer <bootnode_multiaddr_2> \
  --listen /ip4/0.0.0.0/tcp/30333 \
  --rpc-port 9944
```

Flag notes (full reference: `rupeecoin-node --help` or HANDBOOK §B.4):

- `--checkpoint` is the weak-subjectivity boundary in
  `height:block_hash:state_root` form. **Never skip it on a public network** —
  without it a long-range attacker can feed your fresh node an alternative
  history.
- `--peer` is repeatable; list at least two bootnodes.
- `--stake` registers you as a validator once the node is synced and the stake
  transaction is finalized. Omit it to run a non-validating full node first and
  add it later.
- Nodes persist chain + validator registry in `--data-dir`; restarting on the
  same directory replays local blocks and continues. Safe to `systemctl restart`.

A systemd unit is the recommended way to keep the node running:

```ini
[Unit]
Description=RupeeCoin Zeta validator
After=network-online.target

[Service]
User=rupeecoin
EnvironmentFile=/etc/rupeecoin/zeta.env    # holds VSEED=...
ExecStart=/usr/local/bin/rupeecoin-node \
  --data-dir /var/lib/rupeecoin-zeta \
  --validator-seed ${VSEED} \
  --stake 10000 \
  --genesis-config /etc/rupeecoin/genesis-zeta.json \
  --checkpoint <height>:<hash>:<state_root> \
  --peer <bootnode_1> --peer <bootnode_2> \
  --listen /ip4/0.0.0.0/tcp/30333
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

## 6. Verify you are validating

```bash
curl -s http://127.0.0.1:9944/rpc/height
curl -s http://127.0.0.1:9944/rpc/status
curl -s "http://127.0.0.1:9944/rpc/validator_info?addr=<your_40hex_addr>"
```

`status` should show `active_validators` including you and advancing
`last_finalized_epoch`. `validator_info` reports your stake, uptime score,
proposal score, and claimable rewards.

For a live view, run the TUI dashboard (note: `--addr` takes the 40-hex or
Base58Check address, not the registry pubkey hash):

```bash
target/release/rupeecoin-dashboard --rpc http://127.0.0.1:9944 --addr <validator_addr>
```

## 7. Rewards, scoring, and slashing — what you signed up for

- **Rewards** accrue per proposed block (coinbase split) and per epoch from the
  PMFP fund, weighted by your Proof-of-Contribution score (uptime, attestation
  timeliness, proposal timeliness, propagation quality) — not raw stake alone.
  Coinbase outputs mature after **100 blocks**. Check accrued rewards with
  `rupeecoin-wallet claim --rpc http://127.0.0.1:9944`.
- **Inactivity leak**: if you go offline while finality stalls, your stake
  leaks. Keep the node up; 30-day continuous uptime is the Zeta milestone.
- **Slashing**: equivocation (double proposal / double vote) burns part of your
  stake and ejects you. Never run the same `--validator-seed` on two machines,
  including a "hot spare" — that is the classic self-slash.
- **Unstaking** takes 604,800 slots (~28 days) before funds are withdrawable.

## 8. Optional hardening

- **Tor**: route outbound P2P via `--tor-proxy 127.0.0.1:9050`.
- **Peer allowlist**: `--peer-allowlist <file>` (one PeerId per line, `#`
  comments) restricts gossip to known validators — mainly for permissioned test
  phases.
- Run the explorer against the same data dir (read-only secondary) to serve
  your own chain view: HANDBOOK §B.7.

## 9. Getting help

- Operational questions: the Zeta validator channel (announced at launch).
- Bugs: open a GitHub issue with `node.log` and your `rupeecoin-node` version.
- Security issues: **do not open an issue** — follow [`SECURITY.md`](../SECURITY.md).
