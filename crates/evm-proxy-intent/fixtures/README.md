# Cancel intent vectors

Cancel encodes a uint64 nonce. Domain salt binds the program, Store and genesis hash. The vectors preserve full-width chain IDs, uint64 boundaries, domain mutations and wrapping-counter boundaries.

From this crate's root, install the pinned tooling without changing a package manifest or lockfile:

`npm install --no-save --package-lock=false --ignore-scripts viem@2.57.7 @solana/web3.js@1.98.4`

Check the saved public vectors and signatures:

`node fixtures/scripts/generate-cancel-vectors.cjs --check`

`--write` generates a fresh signing key in memory and stores only public data and signatures. It never saves the key.
