# GMX-Solana

## Integration

### Method 1: Using `declare_program!`

#### 1. Initialize a new Rust project and add dependencies

Create a new Rust project and include `anchor_lang` and `bytemuck` as dependencies:

```toml
[dependencies]
anchor-lang = "0.30.1"
bytemuck = { version = "1.19.0", features = ["min_const_generics"] }
```

#### 2. Download and Store IDLs in `{PROJECT_ROOT}/idls/`

You can retrieve the IDLs using the `anchor` CLI or download them directly from the explorer ([`gmsol-store` Program][store-program-link] and [`gmsol-treasury` Program][treasury-program-link]).

Once downloaded, move them to the `{PROJECT_ROOT}/idls/` directory.

Your project structure should now look like this:

```bash
{PROJECT_ROOT}/
├── .gitignore
├── Cargo.lock
├── Cargo.toml
├── idls
│   ├── gmsol_store.json
│   └── gmsol_treasury.json
└── src
    └── lib.rs
```

#### 3. Declaring Programs in `lib.rs`

Use `declare_program!` to register the `gmsol-store` and `gmsol-treasury` programs:

```rust
use anchor_lang::declare_program;

declare_program!(gmsol_store);
declare_program!(gmsol_treasury);
```

#### 4. Build and Open the Documentation

Run the following command to generate and view the documentation:

```bash
cargo doc --open
```

If the build is successful, it will automatically open the documentation in your default web browser.

#### 5. Example Project

For a working implementation, check out the [gmx-solana-programs][gmx-solana-programs-link].

[store-program-link]: https://explorer.solana.com/address/Gmso1uvJnLbawvw7yezdfCDcPydwW2s2iqG3w6MDucLo/anchor-program
[treasury-program-link]: https://explorer.solana.com/address/GTuvYD5SxkTq4FLG6JV1FQ5dkczr1AfgDcBHaFsBdtBg/anchor-program
[gmx-solana-programs-link]: https://github.com/gmsol-labs/gmx-solana-programs

## Development

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install)
- [Solana v1.18.26](https://docs.anza.xyz/cli/install)
- [Anchor v0.30.1](https://www.anchor-lang.com/docs/installation)
- [Node](https://nodejs.org/en/download)
- [Just](https://github.com/casey/just?tab=readme-ov-file#installation)

### Commands

To run all tests:

```bash
just
```

To install the `gmsol` CLI:

```bash
cargo install-gmsol
```

Use the following command to verify the CLI is installed properly:

```bash
gmsol --version
```

### Troubleshooting

#### 1. Failed to start `test-validator` on MacOS

**Error Message:**

```bash
Error: failed to start validator: Failed to create ledger at test-ledger: io error: Error checking to unpack genesis archive: Archive error: extra entry found: "._genesis.bin" Regular
```

**Posssible Solution:**

Check [this comment](https://github.com/solana-labs/solana/issues/35629#issuecomment-2501133871).
