# rupeecoin — Python SDK

PyO3 bindings for the RupeeCoin blockchain.  
Phase 9.4 complete · v0.3.0 · Rust + Python via maturin

## Install

```bash
pip install ./sdk-py          # from repo root
# or for development:
cd sdk-py && maturin develop --release
```

## 5-line quickstart

```python
import rupeecoin

rpc    = rupeecoin.RpcClient("http://127.0.0.1:9944")
print("height:", rpc.height())
wallet = rupeecoin.Wallet.generate()
print("address:", wallet.receive_address())
```

## Build a transaction

```python
import rupeecoin

rpc    = rupeecoin.RpcClient("http://127.0.0.1:9944")
wallet = rupeecoin.Wallet.generate()
utxos  = rpc.utxos(wallet.receive_address_hex())  # fund this address first

op  = rupeecoin.OutPoint(utxos[0]["txid"], utxos[0]["vout"])
out = rupeecoin.TxOutput("<recipient_address>", rupeecoin.coins_to_micro(1.0))
tx  = rupeecoin.build_tx(wallet, [op], [out])
print(rpc.submit_tx(tx.hex))
```

## API

| Symbol | Description |
|---|---|
| `Wallet.generate()` | New random HD wallet |
| `Wallet.from_seed(bytes)` | Restore from 32-byte seed |
| `wallet.receive_address()` | Current address (Base58Check) |
| `wallet.receive_address_hex()` | Current address (40-char hex) |
| `wallet.master_seed()` | 32-byte seed for backup |
| `RpcClient(url)` | Connect to a node |
| `rpc.height()` | Chain tip height |
| `rpc.status()` | Full node snapshot (dict) |
| `rpc.utxos(addr)` | Unspent UTXOs for an address |
| `rpc.submit_tx(hex)` | Broadcast a signed transaction |
| `build_tx(wallet, inputs, outputs, chain_id, nonce, expiry)` | Build + sign tx |
| `coins_to_micro(float)` | 1.5 → 150_000_000 |
| `micro_to_coins_str(int)` | 150_000_000 → "1.5 RPC" |
| `encode_addr(bytes)` | 20 bytes → Base58Check string |
| `decode_addr(str)` | Base58Check/hex → 20 bytes |
