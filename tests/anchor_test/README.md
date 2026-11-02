# EVM proxy JS transaction check

The existing Programs CI job runs the standard Anchor validator suite and the
production SBF JS transaction check. The latter uses the tracked emitter in
`scripts/` to generate an in-memory EVM signer and a transaction from the JS SDK.
No signing key or transaction fixture is written to disk.

From the repository root, after building the normal EVM proxy SBF program:

```sh
npm ci --prefix tests/anchor_test/scripts
wasm-pack build crates/sdk --target nodejs --features js
SBF_OUT_DIR="$PWD/target/deploy" cargo test --locked -p gmsol-evm-proxy --features sbf-test,manual-js-sbf --test withdraw_js
```

`withdraw_js` invokes the emitter by a checkout-relative path. It fails if the
package installation, JS SDK build, or production ELF is missing. It checks
the generated transaction by executing it against the production SBF program.
