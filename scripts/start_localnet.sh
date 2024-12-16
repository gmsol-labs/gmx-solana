#!/bin/bash

export ADDRESS=$(solana-keygen pubkey)

export PLUGIN_MESSENGER_CONFIG='{messenger_type="Redis",connection_config={redis_connection_str="redis://localhost:6379"}}'
RUST_LOG=trace \
  solana-test-validator -r \
  -l test-ledger \
  --url mainnet-beta \
  --geyser-plugin-config scripts/resources/geyser/plugin_config.json \
  --limit-ledger-size 1000000000 \
  --log-messages-bytes-limit 1000000000 \
  --compute-unit-limit 1000000000 \
  --clone 5gxPdahvSzcKySxXxPuRXZZ9s6h8hZ88XDVKavWpaQGn \
  --clone DaWUKXCyXsnzcvLUyeJRWou8KTn7XtadgTsdhJ6RHS7b \
  --upgradeable-program rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ external-programs/pyth-receiver.so $ADDRESS \
  --upgradeable-program pythWSnswVUd12oZpeFP8e9CVaEqJg25g1Vtc2biRsT external-programs/pyth-push-oracle.so $ADDRESS \
  --upgradeable-program HDwcJBJXjL9FpJ7UBsYBtaDjsBUhuLCUYoz3zr8SWWaQ external-programs/wormhole.so $ADDRESS \
  --upgradeable-program Gmso1YHcDFwzxBjXP4F6Hr35BZqWiQUzTwCN6Z2di3e target/verifiable/gmsol_store.so $ADDRESS \
  --upgradeable-program GTtRSYha5h8S26kPFHgYKUf8enEgabkTFwW7UToXAHoY target/verifiable/gmsol_treasury.so $ADDRESS \
  --upgradeable-program 4nMxSRfeW7W2zFbN8FJ4YDvuTzEzCo1e6GzJxJLnDUoZ target/verifiable/mock_chainlink_verifier.so $ADDRESS \
  $@
