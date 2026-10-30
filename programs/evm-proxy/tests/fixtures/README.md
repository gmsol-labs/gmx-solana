# Verification probe vectors

Public encoding fixtures generated with viem **2.57.7** `hashTypedData` and
`hashStruct`, following [EIP-712](https://eips.ethereum.org/EIPS/eip-712).
`VerificationProbe(bytes32 request)` is a test message, not a product intent.
No signing keys are used or stored. Rust signs the pinned digests with ephemeral
keys, or recovers public keys from synthetic boundary signatures.

With viem 2.57.7 available on Node's module search path, regenerate or verify:

```sh
node programs/evm-proxy/tests/fixtures/scripts/generate-verification-vectors.cjs --write
node programs/evm-proxy/tests/fixtures/scripts/generate-verification-vectors.cjs --check
```

The fixture records every domain input, the struct hash and the final digest.
The Rust test consumes these values without reimplementing EIP-712 hashing.

`cancel-pair-vectors.json` pins 22 production Cancel messages using the signed
`(generation, nonce)` pair. The generator uses viem **2.57.7** for the type,
struct and final EIP-712 hashes, and `@solana/web3.js` **1.98.4** for the
canonical proxy PDA. `--write` generates a signing key only in process memory;
the file holds its public address, signatures and hashes, never the key.
`--check` recomputes every hash and PDA and recovers the recorded address from
each signature. With those pinned packages on Node's module search path:

```sh
node programs/evm-proxy/tests/fixtures/scripts/generate-cancel-vectors.cjs --check
```
