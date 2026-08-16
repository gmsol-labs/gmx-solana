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
  --upgradeable-program rec2HHDDnjLfj4kE7VyEtFA1HPGQLK33259532cRyHp external-programs/pyth-receiver.so $ADDRESS \
  --upgradeable-program pyt2F414BA6dPttK6RddPZUdHfapoBN24GL5wbrPCou external-programs/pyth-push-oracle.so $ADDRESS \
  --upgradeable-program HDw2E7P8X1SkCyjvoGsfBGAVUutKcj874bXjHrpVYrVL external-programs/wormhole.so $ADDRESS \
  --upgradeable-program Gmso1uvJnLbawvw7yezdfCDcPydwW2s2iqG3w6MDucLo target/verifiable/gmsol_store.so $ADDRESS \
  --upgradeable-program GTuvYD5SxkTq4FLG6JV1FQ5dkczr1AfgDcBHaFsBdtBg target/verifiable/gmsol_treasury.so $ADDRESS \
  --upgradeable-program TimeBQ7gQyWyQMD3bTteAdy7hTVDNWSwELdSVZHfSXL target/verifiable/gmsol_timelock.so $ADDRESS \
  --upgradeable-program 4nMxSRfeW7W2zFbN8FJ4YDvuTzEzCo1e6GzJxJLnDUoZ target/verifiable/mock_chainlink_verifier.so $ADDRESS \
  $@
