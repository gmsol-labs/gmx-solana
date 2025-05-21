# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Breaking Changes

- programs: Upgrade to `anchor v0.31.1` and `solana v2.1.21`.
- programs: Removed support for Chainlink data feeds.
- model: Added `paid_in_secondary_output_amount` and `is_collateral_token_long` parameters to the `on_insufficient_funding_fee_payment` function.
- model: The `PoolDelta::price_impact` function now returns a `PriceImpact` structure that includes a `BalanceChange`, instead of just the price impact value.
- model: Updated the fee factor logic to depend on `BalanceChange` rather than the sign of price impact; the `FeeParams::factor` function (along with other related functions) now takes `BalanceChange` instead of the `is_positive_impact` flag.
- sdk: Removed support for `spl-governance`.
- sdk(sdk): Updated the `ExchangeOps::update_order` function to include a `hint` parameter and return a future.

### Added

- programs: Defined the callback interface and added an example `gmsol-callback` program.
- programs(store): Added callback-enabled instructions for order.
  - Added the `create_order_v2` instruction.
  - Added the `update_order_v2` instruction.
  - Added the `close_order_v2` instrcution.
  - Added the `execute_increase_or_swap_order_v2` instruction.
  - Added the `execute_decrease_order_v2` instruction.
- programs(store): Added `InsufficientFundingFeePayment` CPI event to be emitted on insufficient funding fee payment.
- programs(store): Added `GtBuyback` CPI event to be emitted on GT exchange vault confirmation.
- programs(store): Added `confirm_gt_exchange_vault_v2` instruction, which requires the caller to provide buyback information.
- programs(store): Added `OrderUpdated` CPI event to be emitted on order creation or update.
- programs: Added the `gmsol-competition` program.
- programs(store): Added a new `AllowPriceAdjustment` flag to `TokenConfig` indicating whether the price adjustment is allowed.
- programs(store): Introduced price deviation checks with a `max_deviation_ratio` parameter. If price adjustment is allowed, the oracle price will be adjusted as needed to stay within the allowed range.
- programs(store): Added the `toggle_token_price_adjustment` and `set_feed_config_v2` instructions to support new token config items.
- programs(treasury): Added `receiver_vault_out` field to `GtBank` to track total amount withdrawn from receiver vault.
- programs(treasury): Added `GtBankFlags::Confirmed` to indicate whether the GT bank is confirmed.
- programs(treasury): Added `GtBankFlags::SycnedAfterConfirmation` to indicate whether the GT bank is synced after confirmation.
- programs(treasury): Added `sync_gt_bank_v2` instruction, which returns the synced amount.
- sdk(sdk): Added `callback` option to `ops::CreateOrderBuilder`.
- sdk(sdk): Added `callback` field to `ExecuteOrderHint` and `CloseOrderHint`.

### Changed

- programs(store): Deprecated `create_order`, `update_order`, `close_order`, `execute_increase_or_swap_order` and `execute_decrease_order` instructions.
- programs(store): Deprecated `confirm_gt_exchange_vault` instruction.
- programs(store): Deprecated `set_feed_config` instruction.
- programs(treasury): Deprecated `sync_gt_bank` instruction.

### Fixed

- programs(treasury): Corrected data error in GT buyback message.
- sdk(solana-utils): Fixed missing payer signer in transaction size calculation.

## [0.5.0] - 2025-05-16

### Breaking Changes

- programs: Renamed `mock_chainlink_verifier` to `gmsol_mock_chainlink_verifier`.
- programs: Replaced `data-streams-report` with the crates.io version of `chainlink-data-streams-report`.
- programs: Refactored some common definitions into the `gmsol-utils` crate:
  - Refactored pubkey utilities into `gmsol-utils`.
  - Refactored fixed string utilities into `gmsol-utils`.
  - Refactored market meta utilities into `gmsol-utils`.
  - Refactored `PriceProviderKind` into `gmsol-utils`.
  - Refactored `TokenConfig` and related definitions into `gmsol-utils`.
  - Refactored dynamic access utilities into `gmsol-utils`.
  - Refactored `ActionFlag` and `ActionState` into `gmsol-utils`.
  - Refactored `SwapActionParams` into `gmsol-utils`.
  - Refactored `OrderKind`, `OrderSide`, `PositionKind` and `PositionCutKind` into `gmsol-utils`.
  - Refactored `GlvMarketFlag` into `gmsol-utils`.
  - Refactored definitions related to `InstructionAccount` into `gmsol-utils`.
  - Refactored `TokenFlag` for treasury program into `gmsol-utils`.
- sdk: Added `compute_unit_min_priority_lamports` to `SendBundleOptions`.
- sdk: Boxed `ClientError` in the `Error` definition.
- sdk: Added a new feature flag to `gmsol-solana-utils` crate to consolidate all implementations that rely on the `solana_client` crate.

### Added

- programs: Implemented `Default` for `Glv` and made the `store` field public.
- programs: Introduced a separate `impl_fixed_map!` macro that implements fixed map functionality without defining the corresponding struct.
- programs: Introduced a separated `impl_flags!` macro that implements flag map functionality without defining the container.
- programs: Added the `gmsol-competition` program.
- model: Re-exported `num_traits`.
- sdk: Added more functions to `SquadsOps`:
  - `SquadsOps::squads_create_vault_transaction_and_return_data`: Creates a vault transaction and return the data.
  - `SquadsOps::squads_approve_proposal`: Approves a proposal.
  - `SquadsOps::squads_execute_vault_transaction`: Executes a vault transaction.
  - `SquadsOps::squads_from_bundle`: Creates a bundle of vault transactions for proposing transactions.
- sdk: Implemented the `MakeBundleBuidler` trait for `TransactionBuilder`.
- sdk: Introduced `OnceMakeBundleBuilder`, which implements `MakeBundleBuilder` and can be created directly from `BundleBuilder`.
- sdk: Added the `gmsol-programs` crate.
- sdk: Added the `gmsol-sdk` crate: unlike the `gmsol` crate, this crate is built on top of the `gmsol-programs` crate and includes WASM support.
- sdk: Added `get_token_accounts_by_owner_with_context` utility function.
- sdk: Added `Client::rpc` method to access the shared `RpcClient`.
- sdk: Added `WithSlot::slot_mut` and `WithSlot::value_mut` methods.
- sdk: Added `Client::glvs_with_config` and `Client::glvs` methods.
- sdk: Introduced the `AddressLookupTables` struct to manage multiple address lookup tables.
- sdk: Introduced the `AtomicGroup` struct to represent a set of instructions intended to be executed atomically in a single transaction.
- sdk: Introduced the `ParallelGroup` struct to represent a group of atomic instructions that are safe to execute in parallel.
- sdk: Introduced the `TransactionGroup` struct to build transactions from a sequence of `ParallelGroup`s.
- cli: Added the `treasury batch-withdraw` subcommand.
- cli: Added `--authority` option for the `inspect price-feed` subcommand.
- cli: Introduced a new ALT type `PriceFeed` for the `alt extend` subcommand.
- cli: Added a `--debug` option for the `gt status` subcommand.
- cli: Added the `inspect chainlink` subcommand for inspecting Chainlink Data Streams feeds.
- examples: Added `squads_trader` example.

### Changed

- model: make the `UpdateFundingState::next_funding_amount_per_size` function public.
- cli: Allowed the `migrate referral-code` subcommand to accept multiple addresses and allow the use of user account addresses or owner account addresses.
- cli: Ensured all commands respect the `--priority-lamports` option.
- cli: The `inspect glv` command now supports querying all existing valid GLV accounts.
- cli: Included GLV-related addresses when extending LUTs with market kind.

## [0.4.0] - 2025-03-08

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
- just: Added `build-idls-no-docs` recipe for building IDLs without documentation.

### Changed

- programs: Restricted the creation of instruction buffers so that only the executor wallet can be signer.
- programs: Allowed withdrawals from unauthorized treasury vaults.
- programs: Changed to use the `create_idempotent` instruction instead of `create` to prepare GM vaults when initializing GLV.
- programs: Changed to use the maximized `to_market_token_value` to estimate the price impact after a GLV shift.
- programs: Cancelled the ADL execution fee refund to ensure the fairness of ADL.
- programs: Set `GlvShift::MIN_EXECUTION_LAMPORTS` to `0`.
- programs: The `transfer_referral_code` instruction only update the `next_owner` field of the referral code.
- sdk: Changed to use `Gateway::fetch_signatures_multi` to fetch price signatures for Switchboard pull oracle implementation.
- cli: Implemented instruction buffering and serialization support for the `exchange` command.
- docs: Added the Audits section to the `README.md`.
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

[unreleased]: https://github.com/gmsol-labs/gmx-solana/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/gmsol-labs/gmx-solana/releases/tag/v0.5.0
[0.4.0]: https://github.com/gmsol-labs/gmx-solana/releases/tag/v0.4.0
[0.3.0]: https://github.com/gmsol-labs/gmx-solana/releases/tag/v0.3.0
