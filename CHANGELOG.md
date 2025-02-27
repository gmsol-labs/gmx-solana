# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- programs: Added validation for the accounts length when loading instruction from an `InstructionBuffer`
- programs: Added validation for future oracle timestamps using the new `oracle_max_future_timestamp_excess` amount config
- programs: Added validation for `MarketDecrease` orders to ensure the oracle prices are updated after the position's last increase ts, similar to `LimitDecrease` orders
- programs: Added features to control the enablement of instructions for (GLV) deposit, (GLV) withdrawal, and (GLV) shift
- sdk: Added the `gmsol::cli` module
- sdk: Added `SwitchboardPullOracleFactory` structure
- cli: Added support for Switchboard to the `order` subcommand
- tests: Added an integration testing suite `integration_test` to the `gmsol` tests
- docs: Created a `CHANGELOG.md` file to document project updates

### Changed

- programs: Replaced the `no-mock` feature with `mock`, meaning the default is "no-mock"
- programs: The `verify` instruction of the `mock-chainlink-verifier` program now will panic if it is not built with the `mock` feature enabled
- programs: Restricted the creation of instruction buffers so that only the executor wallet can be signer
- programs: Allowed withdrawals from unauthorized treasury vaults
- programs: Changed the role authorized to invoke `sync_gt_bank` instruction to `TREASURY_WITHDRAWER`
- programs: Changed to use the `create_idempotent` instruction instead of `create` to prepare GM vaults when initializing GLV
- programs: Changed to use the maximized `to_market_token_value` to estimate the price impact after a GLV shift
- programs: Cancelled the ADL execution fee refund to ensure the fairness of ADL
- programs: Renamed the variants of `ActionDisabledFlag`:
  - `CreateOrder` -> `Create`
  - `UpdateOrder` -> `Update`
  - `ExecuteOrder` -> `Execute`
  - `CancelOrder` -> `Cancel`
- sdk: Changed the arguments of `SwitchboardPullOracle::from_parts` function
- sdk: Changed to use `Gateway::fetch_signatures_multi` to fetch price signatures for Switchboard pull oracle implementation
- tests: Renamed `anchor_tests` testing suite to `anchor_test` in the `gmsol` tests
- just: The `build-idls` recipe now builds IDLs that include docs
- Updated dependencies:
  - `switchboard-on-demand`: `v0.3.4`

### Fixed

- programs: Fixed the missing address validation when using Switchboard feeds
- programs: Fixed the incorrect owner of `SbFeed` when `devnet` feature is enabled
- programs: Fixed bug of allowing limit-swap orders to be updated to accept zero `min_output`
- programs: Fixed inconsistent market token balance validation for `GlvDepositOperation`
- programs: Fixed incorrect slot used as the publishing slot for a Switchboard feed price. Now the `SbFeed::result_land_slot()` is used instead

## [0.3.0] - 2025-02-18

### Added

- Initial release
- Implemented core programs:
  - `gmsol-store`: Provide the protocol's core instructions, including permission management, market management, and core support for swaps and perpetual trading.
  - `gmsol-treasury`: Provides instructions for treasury management and implementing GT buyback.
- Provided SDK (`gmsol`) and other utility crates
- Provided a command-line interface (`gmsol`)

[unreleased]: https://github.com/gmsol-labs/gmx-solana/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/gmsol-labs/gmx-solana/releases/tag/v0.3.0
