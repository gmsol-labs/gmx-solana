# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Breaking Changes

- programs: Replaced the `no-mock` feature with `mock`, meaning the default is "no-mock".
- programs: Replaced the `no-bug-fix` feature with the `migration` feature.
- programs: The `verify` instruction of the `mock-chainlink-verifier` program now will panic if it is not built with the `mock` feature enabled.
- programs: Changed the role authorized to invoke `sync_gt_bank` instruction to `TREASURY_WITHDRAWER`.
- programs: Renamed the variants of `ActionDisabledFlag`:
  - `CreateOrder` -> `Create`
  - `UpdateOrder` -> `Update`
  - `ExecuteOrder` -> `Execute`
  - `CancelOrder` -> `Cancel`
- programs: Changed the default byte order to little-endian.
- programs: Changed the index type for `PriceFeed` to `u16`.
- programs: Changed the index type for `TradeData` to `u16`.
- programs: Changed the index type for `Glv` to `u16`.
- programs: Changed the index type for `TreasuryVaultConfig` to `u16`.
- programs: Replaced `ReferralCode` with `ReferralCodeV2`.
- programs: Renamed the following structures to resolve IDL conflicts:

  - `SwapParams` -> `SwapActionParams`
  - `{Action}Params` -> `{Action}ActionParams` (e.g., `DepositParams` -> `DepositActionParams`)
  - `TokenAccounts` -> `{Action}TokenAccounts` (e.g., `DepositTokenAccounts`)
  - `GtState` (in `states::user`) -> `UserGtState`

- programs: Replaced `Pool`, `Clocks`, and `OtherState` with `EventPool`, `EventClocks`, and `EventOtherState` in the `MarketStateUpdated` event.
- programs: Redefined the `TradeEvent` structure to resolve `declare_program!` errors.

- model: Separated `BorrowingFeeMarketMut` trait from the `PerpMarketMut` trait.
- sdk: Changed the arguments of `SwitchboardPullOracle::from_parts` function.
- cli: Renamed the `--keep-previous-buffer` option of `other set-idl-buffer` to `--keep-buffer`.
- tests: Renamed `anchor_tests` testing suite to `anchor_test` in the `gmsol` tests.
- Renamed the `mock-chainlink-verifier` crate to `gmsol-mock-chainlink-verifier`.
- Renamed the `chainlink-datastreams` crate to `gmsol-chainlink-datastreams`.
- Updated dependencies:
  - `switchboard-on-demand`: `v0.3.4`

### Added

- programs: Added validation for the accounts length when loading instruction from an `InstructionBuffer`.
- programs: Added validation for future oracle timestamps using the new `oracle_max_future_timestamp_excess` amount config.
- programs: Added validation for `MarketDecrease` orders to ensure the oracle prices are updated after the position's last increase ts, similar to `LimitDecrease` orders.
- programs: Added features to control the enablement of instructions for (GLV) deposit, (GLV) withdrawal, and (GLV) shift.
- programs: Added a new config `adl_prices_max_staleness`, allowing the oracle prices to be stale relative to the ADL last update time by this amount.
- programs: Added `accept_referral_code` instruction to complete the referral code transfer.
- programs: Added `cancel_referral_code_transfer` instruction to cancel a referral code transfer.
- programs: Added `migrate_referral_code` instruction for `ReferralCode` account migration.
- programs: Added a new `BorrowingFeesUpdated` CPI event.
- programs: Added a new `GlvPricing` CPI event.
- sdk: Added the `gmsol::cli` module.
- sdk: Added `SwitchboardPullOracleFactory` structure.
- sdk: Added support for `accept_referral_code` and `cancel_referral_code_transfer` instructions.
- sdk: Added `IdlOps` trait and implemented it for `Client`.
- cli: Added support for Switchboard to the `order` subcommand.
- cli: Added support for new referral code management instructions.
- cli: Added the `other close-idl` command for closing IDL accounts.
- cli: Added the `other resize-idl` command for resizing IDL accounts.
- cli: Added the `other set-idl-authority` command.
- tests: Added an integration testing suite `integration_test` to the `gmsol` tests.
- docs: Created a `CHANGELOG.md` file to document project updates.

### Changed

- programs: Restricted the creation of instruction buffers so that only the executor wallet can be signer.
- programs: Allowed withdrawals from unauthorized treasury vaults.
- programs: Changed to use the `create_idempotent` instruction instead of `create` to prepare GM vaults when initializing GLV.
- programs: Changed to use the maximized `to_market_token_value` to estimate the price impact after a GLV shift.
- programs: Cancelled the ADL execution fee refund to ensure the fairness of ADL.
- programs: Set `GlvShift::MIN_EXECUTION_LAMPORTS` to `0`.
- programs: The `transfer_referral_code` instruction only update the `next_owner` field of the referral code.
- sdk: Changed to use `Gateway::fetch_signatures_multi` to fetch price signatures for Switchboard pull oracle implementation.
- just: The `build-idls` recipe now builds IDLs that include docs.

### Fixed

- programs: Fixed the missing address validation when using Switchboard feeds.
- programs: Fixed the incorrect owner of `SbFeed` when `devnet` feature is enabled.
- programs: Fixed bug of allowing limit-swap orders to be updated to accept zero `min_output`.
- programs: Fixed inconsistent market token balance validation for `GlvDepositOperation`.
- programs: Fixed incorrect slot used as the publishing slot for a Switchboard feed price. Now the `SbFeed::result_land_slot()` is used instead.
- programs: Fixed heartbeat validation for Switchboard to be based on `SbFeed::result_ts()`.
- programs: Fixed the issue of not updating the borrowing states of markets in the swap path.
- programs: Fixed the issue of max PnL not being validated when depositing GM tokens(market tokens) directly into GLV.

## [0.3.0] - 2025-02-18

### Added

- Initial release.
- Implemented core programs:
  - `gmsol-store`: Provide the protocol's core instructions, including permission management, market management, and core support for swaps and perpetual trading.
  - `gmsol-treasury`: Provides instructions for treasury management and implementing GT buyback.
- Provided SDK (`gmsol`) and other utility crates.
- Provided a command-line interface (`gmsol`).

[unreleased]: https://github.com/gmsol-labs/gmx-solana/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/gmsol-labs/gmx-solana/releases/tag/v0.3.0
