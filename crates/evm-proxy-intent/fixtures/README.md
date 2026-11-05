# Cancel signing vectors

`cancel-pair-vectors.json` pins 22 production Cancel messages generated with
viem 2.57.7 and `@solana/web3.js` 1.98.4. The Rust tests compare the recorded
domain, struct and final EIP-712 hashes with the shared encoding crate. The
fixture holds public addresses and signatures, never a signing key.

With those packages on Node's module search path, verify the independent
vectors with `node crates/evm-proxy-intent/fixtures/scripts/generate-cancel-vectors.cjs --check`.
