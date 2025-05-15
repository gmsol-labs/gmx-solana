# GMX-Solana

[![Crates.io](https://img.shields.io/crates/v/gmsol-sdk.svg)](https://crates.io/crates/gmsol-sdk)
[![Docs.rs](https://docs.rs/gmsol-sdk/badge.svg)](https://docs.rs/gmsol-sdk)
[![npm](https://img.shields.io/npm/v/@gmsol-labs/gmsol-sdk.svg)](https://www.npmjs.com/package/@gmsol-labs/gmsol-sdk)

## Audits

| Program          | Last Audit Date | Version   |
| ---------------- | --------------- | --------- |
| [gmsol-store]    | [2025-03-07]    | [2a66761] |
| [gmsol-treasury] | [2025-03-07]    | [2a66761] |
| [gmsol-timelock] | [2025-03-07]    | [2a66761] |

[gmsol-store]: https://github.com/gmsol-labs/gmx-solana/tree/main/programs/gmsol-store
[gmsol-treasury]: https://github.com/gmsol-labs/gmx-solana/tree/main/programs/gmsol-treasury
[gmsol-timelock]: https://github.com/gmsol-labs/gmx-solana/tree/main/programs/gmsol-timelock
[2025-03-07]: https://github.com/gmsol-labs/gmx-solana-audits/blob/main/GMX_Solana_Audit_Report_Mar_7_2025_Zenith.pdf
[2a66761]: https://github.com/gmsol-labs/gmx-solana/commit/2a66761d6573a6db6160a19fc3057e2091aebbfe

## Integration

### Method 1: Using the Rust SDK

Add the following to your `Cargo.toml`:

```toml
[dependencies]
gmsol-sdk = { version = "0.5.0", features = ["client"] }
```

Create a `Client` and start using the core APIs:

```rust
use gmsol_sdk::{
    Client,
    ops::ExchangeOps,
    solana_utils::{
        cluster::Cluster,
        solana_sdk::{pubkey::Pubkey, signature::read_keypair_file},
    },
};

let keypair =
    read_keypair_file(std::env::var("KEYPAIR")?)?;
let market_token: Pubkey = std::env::var("MARKET_TOKEN")?.parse()?;

let client = Client::new(Cluster::Mainnet, &keypair)?;
let store = client.find_store_address("");

let (txn, order) = client
    .market_increase(
        &store,
        &market_token,
        true,
        5_000_000,
        true,
        500_000_000_000_000_000_000,
    )
    .build_with_address()
    .await?;

let signature = txn.send().await?;
```

### Method 2: Using `declare_program!`

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
