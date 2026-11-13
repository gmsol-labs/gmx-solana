# Cancel signing vectors

`cancel-pair-vectors.json` pins 22 single-nonce Cancel messages generated with
viem 2.57.7 and `@solana/web3.js` 1.98.4. The Rust tests compare the recorded
domain, struct and final EIP-712 hashes with the shared encoding crate. The
fixture holds public addresses and signatures, never a signing key.

The filename is retained for continuity. Zero, MAX and a nonce above 2^53 are
included. The first vector also records the old generation-bearing schema as
a negative control: its signature recovers the owner only for the old digest.
Rollover vectors describe boundary inputs, not a reachable full-cycle history
or proof of the absolute lifetime replay requirement.

With those packages on Node's module search path, verify the independent
vectors with `node crates/evm-proxy-intent/fixtures/scripts/generate-cancel-vectors.cjs --check`.
