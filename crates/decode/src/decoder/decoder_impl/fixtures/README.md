# V1 reference fixtures

`v1-reference.json` holds 20 synthetic cases, one per line, serialized and sanitized by the official Anza codec in an isolated project. Each case includes the original expected fields. Signatures and test RPC meta are synthetic; real provider replay is still required before deployment.

Sources (Apache-2.0): [SIMD-0385](https://github.com/solana-foundation/solana-improvement-documents/blob/main/proposals/0385-transaction-v1.md), [message 4.4.0 / aa9ce86](https://github.com/anza-xyz/solana-sdk/tree/aa9ce86aedecee08f1f61bc1bb0c1e2f90f55de7/message/src/versions/v1), [transaction 4.1.5 / 3a4e8ef](https://github.com/anza-xyz/solana-sdk/blob/3a4e8ef7dd15655296ecea4b0caa3d7bbc859335/transaction/src/versioned/mod.rs).

Cases cover all 16 config-presence combinations, multiple signers/instructions, zero versus absent values, no instructions, large transactions, 4096 bytes, and maximum counts. The tests also cover malformed input, legacy/v0 compatibility, ALT, visitors and typed GMX CPI events.

To regenerate, copy `generate_reference.rs` to `src/main.rs` in a project **outside this workspace**, using this manifest:

```toml
[package]
name = "gmsol-v1-reference"
version = "0.1.0"
edition = "2021"
[workspace]
[dependencies]
solana-transaction = { version = "=4.1.5", features = ["wincode"] }
solana-message = "=4.4.0"
solana-address = "=2.6.1"
solana-hash = "=4.4.0"
solana-short-vec = "=3.2.2"
wincode = "=0.5.5"
base64 = "=0.22.1"
serde_json = "1.0"
```

Run `cargo run --release > v1-reference.json` (reference generation used Rust 1.96), then review the result. The reference dependencies/toolchain are only for regeneration; normal repository builds and fixture tests retain Solana 2.1.21 and the existing Rust toolchain.
