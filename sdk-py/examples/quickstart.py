"""RupeeCoin Python SDK — quickstart (5-line core, extended for clarity)."""
import rupeecoin

# 1. Connect to a local node
rpc = rupeecoin.RpcClient("http://127.0.0.1:9944")
print("Chain height:", rpc.height())

# 2. Generate (or restore) a wallet
wallet = rupeecoin.Wallet.generate()
print("Receive address:", wallet.receive_address())

# ── Extended demo ──────────────────────────────────────────────────────────────

# Query full node status
# status = rpc.status()
# print(f"  slot={status['slot']}  epoch={status['epoch']}  base_fee={status['base_fee']}")

# Fetch UTXOs for this address
my_hex = wallet.receive_address_hex()
utxos = rpc.utxos(my_hex)        # empty for a fresh wallet
print(f"UTXOs: {len(utxos)}")

# Build a signed transaction (requires funded UTXOs)
# Once funded, replace the dummy values with real UTXO entries:
#
#   op   = rupeecoin.OutPoint(utxos[0]["txid"], utxos[0]["vout"])
#   out  = rupeecoin.TxOutput(recipient="<dest_address>",
#                             amount_micro=rupeecoin.coins_to_micro(1.0))
#   tx   = rupeecoin.build_tx(wallet, [op], [out], chain_id=rupeecoin.MAINNET_CHAIN_ID)
#   status = rpc.submit_tx(tx.hex)
#   print("Submitted:", status)

print("Done.")
