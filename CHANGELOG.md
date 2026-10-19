# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Breaking Changes

- sdk(sdk): `OrderSimulationOutput::Increase` gained `gross_collateral_amount` and `builder_fee`, and `OrderSimulationOutput::Decrease` gained `receipts` and `builder_fee`. Code that destructures these variants without `..` must name the new fields. `OrderSimulationBuilderForSimulator` gained one builder-state element, which matters only to code that spells the alias out. For an order with a builder fee, `report.params().collateral_increment_amount()` of an increase is now the net collateral, as in the program's trade event.
- programs(store): GT exchange requests can also arrive through `request_gt_exchange_v2`, with owner or delegate authority and a separate rent payer. Consumers that identify requests only by `request_gt_exchange` must recognize the new instruction; the legacy accounts and behavior remain unchanged.
- sdk(sdk): `Client::complete_order` and `complete_order_with_config` now return `OrderCompletion` instead of `Option<TradeEvent>`: `Trade` (the order's trade), `NoTrade` (closed without one, e.g. a swap or a cancelled order) or `Inconclusive` (the order is gone but its history does not show the result). Replace `Some(trade)` with `OrderCompletion::Trade(trade)` and `None` with the other two variants.

### Changed

- sdk(sdk): `MarketGraph::simulate_order` and `MarketGraph.simulate_order` (JavaScript), deprecated since 0.8.0, do not model builder fees: their results are those of an order with no builder fee. Their documentation and deprecation notes now say so and point to `to_simulator`, and the JavaScript binding returns an error for a non-zero `builder_fee_factor` instead of ignoring it. Use `Simulator` for orders that carry a builder fee.
- sdk(sdk): `OrderSimulation`/`Simulator::simulate_order` restores the simulator to its state before the call when a simulation with a non-zero builder fee factor fails, as the failed execution would revert on chain. Simulations without a builder fee are unchanged, including the state they leave behind on failure.
- sdk(sdk): Added an optional `builder_fee_factor` field to `SettleBuilderFeeHint`, filled by `FromRpcClientWith` and defaulting to unknown (`None`, treated as chargeable). Callers that use the typed builder, or deserialize the hint (JavaScript), need no change; Rust code that constructs the hint with a struct literal must now set the field.

### Fixed

- sdk(sdk): Fixed `complete_order` returning no trade when an order was executed and closed in separate transactions (e.g. orders with a builder fee), when the trade landed before the wait started, or when polling found the order closed after the trade was already seen. The result is now rebuilt from the order's whole lifecycle, from its close back to its creation, and only from events for that order and action id, so another order's trade, or a trade from an earlier order at a reused address, is never returned.
- sdk(sdk): Fixed order simulations ignoring the builder fee checkpointed onto an order. For a positive fee, an increase used the gross swapped collateral where execution deducts the fee first, and a decrease omitted the fee-funded withdrawal (which can change a partial decrease into a full close, or make it fail), the fee charged on the executed size, and the fee reserved from the output. Estimated collateral, leverage, success and net receipt therefore differed from execution. `Simulator::simulate_order` now takes the order's `builder_fee_factor` and models each stage with the program's routing, caps and rounding; orders without a factor (the default) are unchanged.
- cli(gmsol): When an execute bundle fails before its close batch, the executor now closes the temporary Pyth accounts (encoded VAAs and price updates) that its setup transactions created, returning their rent to the payer. Accounts that were never created are skipped. If a transaction's outcome is unknown, it first waits until that transaction can no longer land. A failed cleanup is logged with the account addresses and does not change the returned error.
- sdk(sdk): Fixed `SettleBuilderFee`, which attached `final_output_token`, `escrow`, `builder_user` and `claim_vault` only when the order already recorded a non-zero builder fee amount. The fee is recorded by the execution, so a settlement built from a pending order, or placed after an execution in the same transaction, carried no accounts and reverted with `TokenMintNotProvided` once the amount turned non-zero, taking the execution with it. The accounts are now attached for any order with a builder whose checkpointed factor can charge, whatever amount is recorded when the instruction is built. Orders without a builder, and orders whose checkpoint was revoked (builder kept, factor `0`, claim vault not required), keep the account-less path. `SettleBuilderFeeHint` gains an optional `builder_fee_factor`, filled by `FromRpcClientWith` and defaulting to unknown, which is treated as chargeable.

### Added

- sdk(sdk): Added `builder_fee_factor` to `Simulator::simulate_order`. Increase simulations deduct the fee from the swapped collateral before the position grows and fail when it cannot be covered. Decrease simulations top up the collateral withdrawal by the fee estimate, charge the fee on the executed size in the final output token, clamp it to the outputs that fund its escrow, and reject `CollateralToPnlToken`. Swap orders reject a non-zero factor. Results report the gross amount, the fee (`BuilderFeeOutcome`: factor, token, computed and charged amounts), and the net amount: `net_collateral_amount()` for an increase and per-token `TokenReceipt`s for a decrease. A receipt counts everything the execution pays out in that token: the final output after the receive-token swap, the secondary pnl output and the funding rebates (`claimable_funding_amounts`), merged by token. The fee cap is computed from the trade outputs only, as in the program, so rebates do not widen it.
- sdk(js): Added `builder_fee_factor` to `Simulator.simulate_order`, and `gross_collateral_amount`, `net_collateral_amount`, `receipts` and `builder_fee` to the increase and decrease simulation outputs.
- sdk(sdk): Added `WithPullOracle::temporary_accounts` and `PriceUpdateInstructions::temporary_accounts`, which keep the addresses of the temporary oracle accounts a build creates, `PythPullOracle::close_temporary_accounts`, which builds a bundle closing the ones that still exist, and `PythPullOracle::open_temporary_accounts`, which lists the ones still open so a cleanup can be checked and retried. Calling either again after a partial cleanup is safe.
- sdk(solana-utils): Added `SendBundleOptions::recent_blockhash` to sign a bundle with a caller-chosen blockhash, so the caller knows its last valid block height. Code that builds `SendBundleOptions` with a struct literal and no `..Default::default()` must add the field.
- sdk(solana-utils): Added `client::wait_for_blockhash_expiry`, which waits until the block height at a given commitment passes a blockhash's last valid height, so a transaction using it can no longer land and is visible at that commitment. It tolerates short RPC outages and gives up after about 3 minutes. Also added `SetExecutionFee` for `&mut T`.
- programs(store): Added `close_gt_exchange_v2` so a GT controller can close a confirmed GT exchange and return its rent to the recorded receiver. The existing `close_gt_exchange` still serves owner receivers and now rejects an exchange configured for another receiver.
- programs(store): Added `request_gt_exchange_v2` for owner or delegate authorized GT burns with a separate rent payer, and records that payer as the receiver when an exchange is created.
- programs(store): Added the `GtAllowanceConsumed` CPI event for successful delegated GT exchange requests, recording the owner, User Account, delegate, spent amount, and remaining allowance.
- sdk(sdk): Added `GtOps::request_gt_exchange_v2_with_time_window_index` and current-window builder with distinct owner, authority, and rent-payer accounts.
- programs(store): Added owner-signed instructions to replace or clear a GT delegate and allowance, increase the remaining allowance, or decrease it to a floor of zero. Each emits the resulting delegation state.
- sdk(sdk): Added `UserOps` builders for GT delegate and allowance changes.
- sdk(decode): Added typed decoding for GT delegation updates.
- cli(gmsol): Added user commands for GT delegate and allowance changes, including Squads proposals through the existing `--squads` mode.
- sdk(solana-utils): Added `SendAndConfirm::send_and_confirm_transaction_detailed`, which returns `Error::SendUnresolved` (with the signature) whenever the transaction may have reached the cluster but its outcome is unknown: the send request timed out, the connection dropped, the response carried a malformed or mismatched signature, or confirmation failed. Only an explicit rejection by the RPC node (e.g. preflight failure) and on-chain failures are still `Error::Client`. It has a default implementation, and `send_and_confirm_transaction_with_config` is unchanged, so existing callers and implementors keep compiling.
- sdk(solana-utils): Added `Error::SendUnresolved { signature, source }`, returned when a transaction may have been submitted but its outcome is unknown. The signature is taken from the signed transaction, so it is available even when the send response is lost. Its outcome is unknown, so callers should look up `signature` before retrying. `Error::unresolved_signature` returns it.

### Changed

- sdk(solana-utils): `compress_send_results` no longer lets a later failure replace a pending `Error::SendUnresolved`, so its signature stays visible.

### Fixed

- sdk(solana-utils): With `continue_on_error = false`, `Bundle::send_all_with_opts_detailed` now waits for the rest of the failing parallel batch and reports each transaction's real outcome. Transactions already submitted alongside the failure were previously reported as `Error::SendAborted` (never sent), so retrying them could execute an action twice. Only later batches are padded with `SendAborted` now.


## [0.11.0] - 2026-10-21

### Breaking Changes

- programs(store): Creating an increase order now requires its final output token to be the position's collateral token, and executing one whose final output token was recorded at creation revalidates the same thing. Creating an order with a different final output token used to succeed and silently ignore the value; it now reverts with `TokenMintMismatched`. Existing orders with an uninitialized final output token are unaffected and keep executing.
- programs(store): An order with a recorded builder fee can no longer be closed by its owner or a keeper until `settle_builder_fee` clears the amount; `close_order_v2` returns `UnsettledBuilderFee` otherwise. Clients must settle before closing. The Rust SDK's `ExecuteOrderBuilder::close(true)` skips the bundled close when a nonzero fee factor is checkpointed, leaving the order open for settlement and a later close.
- programs(store): A nonzero builder fee can reduce position-order proceeds. Increase execution deducts the fee from collateral before increasing the position and can fail if the increment cannot cover it; decrease execution with `CollateralToPnlToken` cannot carry a nonzero fee. Checkpointing also advances an order's oracle freshness bounds, and market orders accept a checkpoint only in their creation slot.
- programs(store): `create_withdrawal` now rejects an execution fee below `Withdrawal::MIN_EXECUTION_LAMPORTS`, including zero; such withdrawals previously passed creation.
- programs(treasury): `complete_gt_exchange` now requires the owner signer account to be writable. Clients constructing account metas from an older IDL must mark it writable.
- programs(store): A newly initialized market uses the 50% reserve-factor default instead of the former 70% fee-receiver default until configured explicitly.
- programs(store): Insolvent liquidation and auto-deleveraging closes no longer credit GT rewards for order and borrowing fees that were not collected, so affected closes mint less GT than before.

### Added

- sdk(decode): Added `TransactionDecoder::decoded_transaction_any_version` for complete legacy/v0/v1 binary decoding with RPC meta, using Solana 2.1.21 types. Existing transaction visitors and CPI event APIs now accept v1; the original `decoded_transaction()` and its public result type remain legacy/v0-compatible and unchanged. SDK event subscriptions and history requests accept versions through v1. No v1 transaction construction or sending is added.
- programs(store): Added the permissionless `set_builder_fee_factor` instruction, with which a User Account owner advertises a builder fee factor on their own account, bounded by the store's `MaxBuilderFeeFactor` (which reads `0` until a config keeper raises it). Emits a `BuilderFeeFactorSet` event.
- sdk(sdk): Added `CreateOrderBuilder::prepare_final_output_token_escrow`, opting an increase order into providing its final output token escrow at creation. The escrow is what a builder fee would be paid out of, so an increase order created without it cannot be given one. Off by default, leaving the previous behavior unchanged.
- sdk(sdk): Added `UserOps::set_builder_fee_factor` for building the instruction.
- programs(store): Added the owner-signed `set_builder_fee` instruction, checkpointing a builder and its advertised fee factor onto one of the owner's own pending position orders. The factor must equal what the builder currently advertises and is re-checked against the store cap, since the cap can be lowered after a rate was legally advertised. The checkpoint must match the order's market and advances its oracle freshness bounds; market orders accept one only in their creation slot. A zero-factor checkpoint removes a builder, even when new attachments are disabled, without requiring a claim vault or associated token account. Clients should avoid reusing order nonces because a checkpoint can apply to a replacement order at the same address and market. Emits a `BuilderFeeSet` event carrying the replaced builder and factor.
- programs(utils): Added `OrderKind::is_user_initiated_position`, spelled as an explicit list of the five owner-created position kinds. `is_increase_position() || is_decrease_position()` is not equivalent: the latter also matches `Liquidation` and `AutoDeleveraging`, which must never carry a builder fee.
- sdk(sdk): Added `BuilderFeeOps::set_builder_fee` and the `SetBuilderFee` atomic-group builder for building the instruction.
- sdk(decode): Added `BuilderFeeFactorSet` and `BuilderFeeSet` to `GMSOLCPIEvent`, so the events decode into their typed form instead of `UnknownOwnedData`.
- sdk(decode): Added `BuilderFeeSettled` and `BuilderFeeClaimed` to `GMSOLCPIEvent`, so the events decode into their typed form instead of `UnknownOwnedData`.
- sdk(solana-utils): Added `Bundle::send_all_with_opts_detailed`, returning one `Result` per transaction with stable bundle indices.
- sdk(solana-utils): Added `Error::SendAborted` for unsent transactions after an early bundle abort.
- sdk(solana-utils): Made `compress_send_results` public so callers can map detailed results to the legacy signature list.
- programs(store): Added the permissionless `settle_builder_fee` instruction, which pays an order's recorded builder fee out of its final output token escrow into the builder's claim vault and zeroes the record. Idempotent: a recorded amount of zero, including orders that never had a builder, is an explicit no-op, so it may be called in any order state. The four transferring-path accounts (`final_output_token`, `escrow`, `builder_user`, `claim_vault`) are optional and required only when the recorded amount is non-zero. Emits a `BuilderFeeSettled` event on the transferring path; the no-op emits nothing.
- programs(store): Added the owner-signed `claim_builder_fees` instruction, with which a builder withdraws the balance of its claim vault to a destination token account. Idempotent: a zero-balance vault is an explicit no-op. Not gated by the `BuilderFee` feature flag. Emits a `BuilderFeeClaimed` event on the transferring path; the no-op emits nothing.
- programs(store): Added `Factors::max_builder_fee_factor`, the store-level cap that a User Account's advertised builder fee factor is checked against, which reads `0` until a config keeper raises it. Taken from reserved space (`[u128; 64]` to `[u128; 63]`), so the account layout and size are unchanged. The same field appears in the `gmsol_liquidity_provider`, `gmsol_timelock` and `gmsol_treasury` IDLs, which share the type.
- programs(store): Added `UserHeader::builder_fee_factor`, the factor a User Account advertises to builders. Also taken from reserved space (`[u8; 128]` to `[u8; 112]`), so the account layout and size are unchanged. Present in the `gmsol_liquidity_provider` IDL for the same reason.
- programs(store): Added the `BuilderFeeCharged` event, emitted on execution whenever the order carries a builder (a non-zero checkpointed factor), even when the amount charged against the order's collateral clamps to zero: its presence alone distinguishes "no builder attached" from "builder attached but nothing was collectible". Carries both the computed and the actually charged amounts.
- programs(store): Added builder fee charging to position order execution. Increase market-balance validation accounts for outgoing builder fees, decrease fee payment can use same-mint secondary output, and increase trade events include the fee output mint.
- programs(store): Added the `BuilderFeeSettled` event, carrying both the recorded amount and the amount actually transferred, so any shortfall between them is observable.
- programs(store): Added the `BuilderFeeClaimed` event, emitted when a builder withdraws from its claim vault.
- programs(store): Added nine builder fee error codes, `6129` through `6137`, appended after the existing codes so no existing code shifts: `BuilderFeeFactorExceedsMaxFactor` (6129), `UnsettledBuilderFee` (6130), `BuilderFeeExceedsCollateral` (6131), `BuilderFeeFinalOutputTokenMismatch` (6132), `BuilderFeeSwapTypeNotAllowed` (6133), `BuilderFeeFactorMismatched` (6134), `BuilderFeeOrderKindNotAllowed` (6135), `BuilderFeeFinalOutputTokenNotInitialized` (6136) and `BuilderFeeFinalOutputTokenEscrowNotInitialized` (6137).
- sdk(decode): Added `BuilderFeeCharged` to `GMSOLCPIEvent`, so the event decodes into its typed form instead of `UnknownOwnedData`.
- sdk(sdk): Added the `SettleBuilderFee`, `ClaimBuilderFees` and `SetBuilderFeeFactor` atomic-group builders, completing the set for the four builder fee instructions. Only `set_builder_fee` had one before, so the other three could not be composed into a single transaction with anything else, and the JS surface had nothing to wrap since it is built on the builder layer. `SettleBuilderFee` carries a hint resolvable from the order account; the other two need none, because every account they take is derived from the payer, the store and the mint.
- sdk(js): Exposed the builder fee instructions to JavaScript clients, including order creation with a final output token escrow and the builders for setting, settling and claiming fees.
- cli(gmsol): Added `user set-builder-fee`, `user set-builder-fee-factor` and `user claim-builder-fees`, wrapping the existing `BuilderFeeOps`/`UserOps` SDK operations. `claim-builder-fees` defaults `--destination` to the payer's associated token account and creates it if missing; an explicitly passed account is never created. `settle_builder_fee` has no CLI command yet.

### Changed

- programs(store): An increase order now records its final output token at creation when the escrow is provided, which is what makes it eligible for a builder fee later.
- sdk(solana-utils): Kept the two-argument `Bundle::send_all_with_opts` as a deprecated compatibility wrapper around the detailed API. It still returns the compressed success-signature list, and when multiple transactions fail it returns the **last** real send error (matching prior overwrite semantics; `SendAborted` placeholders are ignored).

### Fixed

- programs(store): Initialized a new market's reserve factor from its reserve default instead of the fee receiver default.
- programs(treasury): Made the GT exchange owner account writable during completion, including when the fee payer is a different account.
- programs(store): Prevented uncollected fees on insolvent liquidation and auto-deleveraging closes from contributing to GT rewards.
- sdk(sdk): Fixed `PositionStatus.liquidation_price`, which built its threshold from `min_collateral_factor` while a liquidation order is checked against `min_collateral_factor_for_liquidation`. Whenever a market sets the two to different values, the reported price was wrong for every position regardless of collateral token; it read higher than reality for a long, warning earlier than the contract acts. The market-closed variant is covered too, since `position_params()` already resolves it.
- sdk(sdk): Fixed `PositionStatus.liquidation_price` when the collateral token is the market index token, where it held `collateral_value` and the pending funding fee at spot while solving for the very price both depend on. The correction is in the denominator only, since every other term is a plain USD amount; positions collateralised in a different token are unaffected. Correlated but non-identical tokens are deliberately left uncorrected, as that error decays to zero on approach to liquidation.
- programs(store): Fixed `create_withdrawal`, which never checked the execution fee against `Withdrawal::MIN_EXECUTION_LAMPORTS`, unlike every sibling action (deposit, order, shift, GlvDeposit, GlvWithdrawal). A withdrawal could be created with an execution fee below the floor, including zero, and the keeper that later executed it was reimbursed nothing for a full execution.
- cli: Fixed the executor (`execute` commands), `inspect pyth` and treasury batch `claim-fees`, which built an unauthenticated Hermes client and ignored `PYTH_API_KEY`. They now share one Hermes configuration: a new `[pyth]` config section with `endpoint` and `api_key` (also `GMSOL_PYTH__ENDPOINT` / `GMSOL_PYTH__API_KEY`), falling back to `PYTH_API_KEY` and the public Hermes endpoint. A Pyth fetch without a configured key fails with a configuration error naming these settings; the executor only requires a key when the action has Pyth feeds.

## [0.10.0] - 2026-08-12

### Breaking Changes

- programs(utils): Changed `PriceFeedPrice::is_market_open` to take market status flags (signature change).
- sdk(chainlink-datastreams): Removed the `Error::UnknownMarketStatus` variant; an unknown market status no longer fails report conversion.
- sdk(chainlink-datastreams): Removed support for the v4 report schema.

### Added

- programs(utils): Added market status and feed-level market status flags for price feeds.
- programs(store): Added the `set_feed_config_market_status_flag` instruction.
- programs(store): Persisted market status and deferred market openness evaluation.
- sdk(chainlink-datastreams): Added v11 report decoding.
- sdk(sdk): Added an operation for setting feed market status flags.
- cli: Added the `set-market-status-flag` command.
- cli: Added options for market buffer comparison.
- cli: Added an option to enable idempotent price updates.
- just: Added the guardian-set check and rotate recipes (backed by a new xtask).

### Changed

- sdk(sdk): Changed Decimal-to-integer conversions to compensate for truncated scale.

### Fixed

- programs(store): Reallocated the token map account with `zero_init` enabled.
- programs(store): Forbade no-op swaps.
- model: Treated a decrease that zeroes `size_in_tokens` as a full close.
- sdk(chainlink-datastreams): Guarded `decode_full_report` against malformed ABI offsets.
- programs(utils): Fixed an out-of-bounds read in `fixed_map`'s `remove` that aborted the instruction when removing an entry from a map at full capacity.

### Deprecated

- sdk(chainlink-datastreams): Deprecated `Report::market_status` in favor of `extended_market_status`.

## [0.9.1] - 2026-03-18

### Fixed

- programs(store): Corrected the final output market for increase and swap orders.
- programs(store): Corrected market config initialization to set `swap_impact_negative_factor`.
- programs(store): Corrected the `to_market_token` constraint when creating shifts.

## [0.9.0] - 2026-02-04

### Added

- sdk(sdk): Added deposit simulation.
- sdk(sdk): Added withdrawal simulation.
- sdk(sdk): Added shift simulation.
- sdk(sdk): Added GLV deposit simulation.
- sdk(sdk): Added GLV withdrawal simulation.
- sdk(sdk): Added JsGlv and JsGlvModel.
- sdk(sdk): Added support for ChaosLabs Risk Oracle.
- sdk(solana-utils): Added option to send transactions using JITO.
- sdk(sdk): Added more support for virtual inventory.
- sdk(sdk): Added support for useful GLV calculations.
- sdk(sdk): Support changing chainlink pull oracle authority.
- sdk(sdk): Added `PriceUpdateInstructions::split` function.
- sdk(decode): Added `Glv` to `GMSOLAccountData`.
- sdk(sdk): Added support for order fee discount.
- cli: Added auto-creation support of multiple ALTs based on ALT limit for `alt extend` command.
- cli: Added support for creating market config buffer from ChaosLabs Risk Oracle's recommandations.
- cli: Added support for batch lp controller creation.
- cli: Added `exchange update-fees-state` command.
- programs(store): Added `update_price_feed_with_chainlink_idempotent` instruction.

### Changed

- programs(store): Refactored core GLV calculation logic to model crate.

### Fixed

- sdk(sdk): Corrected treasury program ID reference in `new_treasury_program` method.
- sdk(sdk): Corrected cycle detection in fallback swap path algorithm.
- sdk(solana-utils): Fixed bugs related to memo signers.
- sdk(solana-utils): Use current instruction options in transaction size estimation.
- cli: Fixed incorrect decimals used in glv config.
- programs(utils): Prevented silent truncation in `Decimal::try_from_price`.
- programs(liquidity-provider): Added missing pricing events.
- programs(store): Allowed transfer out for increase orders permitted when market is closed.
- programs(store): Ensured an error is always thrown when the required position or event buffer is missing for the `execute_increase_or_swap_order_v2` instruction.

## [0.8.0] - 2025-10-24

### Breaking Changes

- model: Updated the params and logic for `PositionExt::check_liquidatable`: use `min_collateral_factor_for_liquidation` as min factor when `for_liquidation=true`.
- programs(store): Replaced `u8::MAX` synthetic flag in oracle price map with `OraclePriceFlag` bitmap.
- programs(store): The following deprecated store program instructions have been removed:
  - `create_order` (replaced by `create_order_v2` since `0.6.0`)
  - `close_order` (replaced by `close_order_v2` since `0.6.0`)
  - `update_order` (replaced by `update_order_v2` since `0.6.0`)
  - `execute_increase_or_swap_order` (replaced by `execute_increase_or_swap_order_v2` since `0.6.0`)
  - `execute_decrease_order` (replaced by `execute_decrease_order_v2` since `0.6.0`)
  - `set_feed_config` (replaced by `set_feed_config_v2` since `0.6.0`)
  - `confirm_gt_exchange_vault` (replaced by `confirm_gt_exchange_vault_v2` since `0.6.0`)
- programs(treasury): The following deprecated treasury program instructions have been removed:
  - `sync_gt_bank` (replaced by `sync_gt_bank_v2` since `0.6.0`)
  - `create_swap` (replaced by `create_swap_v2` since `0.6.0`)
- sdk(sdk): Added `buyback_value` and `buyback_price` arguments to `GtOps::confirm_gt_exchange_vault`.
- cli: Added `buyback_value` and `buyback_price` arguments to `gt confirm-exchange-vault` subcommand.

### Added

- model: Added `min_collateral_factor_for_liquidation` to `PositionParams`.
- programs(store): Added `cumulative_inv_cost_factor` as a new global metric for GT, along with an instruction to update it.
- programs(store): Added `mint_gt_reward` instruction.
- programs(store): Added `get_market_token_value` instruction for market token pricing.
- programs(store): Added `created_at` field for `Position` account.
- programs(store): Added `min_position_age_for_manual_close` config.
- programs(store): Added `close_empty_position` instruction.
- programs(store): Added flag to indicate whether the token's market is open in oracle price map.
- programs(store): Added `update_closed_state` instruction.
- programs(store): Added new market config: `min_collateral_factor_for_liquidation`.
- programs(store): Introduced new config parameters for closed markets:
  - `market_closed_min_collateral_factor_for_liquidation`
  - `market_closed_skip_borrowing_fee_for_smaller_side`
  - `market_closed_borrowing_fee_base_factor`
  - `market_closed_borrowing_fee_above_optimal_usage_factor`
- programs(store): Introduced flag `use_market_closed_params` to control activation of closed-market configs.
- programs(store): Added a new `MARKET_CONFIG_KEEPER` role with permissions to update market configs.
- programs(store): Added `MarketConfigPermissions` store and `set_market_config_updatable` instruction for managing the permissions for updating market configs.
- programs(store): Added `OrderFlag` and `flags` field to `OrderActionParams`
- programs(store): Added logic to keep empty position account for orders with `OrderFlag::ShouldKeepPositionAccount` set.
- programs(store): Added `set_should_keep_position_account` instrcution.
- programs(store): Added `update_fees_state` instruction.
- sdk(sdk): Added `try_deserialize_zero_copy_from_base64_with_options` utility function.
- sdk(js): Added decode methods with `no_discriminator` option for JsMarket and JsPosition.
- sdk(js): Added `add_create_orders_builder` function.
- sdk(sdk): Added support for `get_market_token_value` instruction.
- sdk(sdk): Added support for `close_empty_position` instruction.
- sdk(solana-utils): Added generic RPC client behind `client-traits` feature.
- sdk(solana-utils): Added cross-platform `HttpRpcSender` implementation for `RpcSender`.
- sdk(solana-utils): Added `RpcClient` trait and `FromRpcClientWith` trait.
- sdk(solana-utils): Added `IntoAtomicGroup::into_atomic_group_with_rpc_client` method with hint fetch support.
- sdk(solana-utils): Added `Program` trait and `InstructionBuilder`.
- sdk(sdk): Added support for LP token staking instructions.
- sdk(decode): Added `UserHeader` to `GMSOLAccountData`.
- sdk(sdk): Added support for `update_closed_state` instruction.
- sdk(sdk): Added support for `set_market_config_updatable` instruction.
- sdk(sdk): Added support for `initialize` instruction for liquidity provider program.
- sdk(sdk): Added support for `set_should_keep_position_account` instruction.
- sdk(sdk): Added support for `update_fees_state` instruction.
- sdk(sdk): Added support for `prepare_position` instruction.
- sdk(sdk&js): Added options for position creation management.
- sdk(sdk): Added new `CreateDeposit` builder.
- sdk(sdk): Added new `CreateGlvDeposit` builder.
- sdk(sdk): Added new `CreateWithdrawal` builder.
- sdk(sdk): Added new `CreateGlvWithdrawal` builder.
- sdk(sdk): Added new `CreateShift` builder.
- sdk(programs): Added `MarketModel::into_empty_position_opts` function.
- sdk(programs): Added `PositionModel::update` function.
- sdk(sdk): Added `decode_anchor_event_with_options` utility function.
- sdk(js): Added support for `TradeEvent`.
- sdk(sdk): Added support for `mint_gt_reward` instruction.
- sdk(sdk): Added `base58-legacy` format for transaction serialization.
- cli: Added `lp init-lp` command.
- cli: Added `exchange close-empty-positions` command.
- cli: Added options to keep or close position.
- cli: Added `gt mint-reward` command.

### Changed

- programs(store): Refactored price flag in oracle price map into `OraclePriceFlag`.
- programs(store): Allow creating and executing order to increase collateral while market is closed.
- cli: Improved prompt when creating timelock instruction buffers.
- sdk(decode): Replaced `solana-transaction-status` with `solana-transaction-status-client-types` to simplify dependency.

### Fixed

- model & sdk(sdk): Aligned the funding value calculation with GMX V2.
- sdk(sdk): Fixed wrong event authority address in `CreateOrder`.
- sdk(sdk): Added `openssl-vendored` feature to avoid link errors.
- sdk(sdk): Fixed missing delegation to base impl of `SetExecutionFee::is_execution_fee_estimation_required`.

## [0.7.1] - 2025-08-15

### Breaking Changes

- program(chainlink-datastreams): Record last update diff in seconds instead of nanoseconds.

### Added

- sdk(sdk): Implemented off-chain order simulation.

### Fixed

- sdk(sdk): Ensured pay token account is prepared for wrap native.
- sdk(chainlink-datastreams): Used `ReportDataV7` to decode version 7 reports.

### Changes

- programs(utils): Supported `last_update_diff` in seconds in `PriceFeedPrice`.

### Deprecated

- programs(utils): Deprecated `PriceFeedPrice::last_update_diff_nanos` function.

## [0.7.0] - 2025-08-05

### Breaking Changes

- model: Changed trait definitions to support virtual inventories.
- model: Included borrowing fee in `paid_order_fee_value` and rename it to `paid_order_and_borrowing_fee_value`.
- sdk(solana-utils): Remiplemented the `BundleBuilder`A to support the new transaction group API.
- sdk(sdk): Updated `SquadsOps` methods to support ephemeral signers.
- sdk(decode): Added `Visitor::visit_transaction` to support transaction decoding.
- sdk(decode): Removed support for `gmsol-store` types.
- cli: Changed the `market push-to-buffer` command to accept only input in the `MarketConfigs` format.

### Added

- model: Added support for virtual inventories and virtual price impact.
- program(store): Introduced virtual inventory mechanism and related instructions.
- program(store): Added instructions to configure token metadata.
- program: Added `security.txt` to all public programs.
- program: Added validation for Chainlink Data Streams report expiry timestamp.
- program: Added new `PriceFlag::LastUpdateDiffEnabled`.
- model: Added `with_virtual_inventory_impact` function to configure whether virtual inventory impact is included.
- sdk(sdk): Added support for the instructions to configure token metadata.
- sdk(chainlink-datastreams): Added `market_status` field and corresponding method to the `Report` struct.
- sdk(chainlink-datastreams): Added support for the report schema v2 and the RWA report schemas (v4, v8) for Chainlink Data Streams.
- sdk(solana-utils): Added `luts` and `luts_mut` methods for `BundleBuilder`.
- sdk(sdk): Added `SerdeMarketConfigBuffer`.
- sdk(decode): Added `TransactionAccess` trait to abstract transaction decoding logic.
- sdk(sdk): Added `MarketClosed` error type.
- sdk(js): Added support for creating orders with callbacks.
- cli: Added `market buffer` command to display the content of given buffer account.
- cli: Added TOML as output format.
- cli: Added output of ALT address for `alt extend` command.
- cli: Added `market update-token-metadatas` command to create or update token metadatas.
- cli: Added commands for virtual inventory management.

### Fixed

- program(timelock): Fixed incorrect data length validation for accounts field.
- cli: Fixed bugs of `glv update-config` command.
- cli: Fixed the auto-wrap functionality for GLV deposits.

### Changed

- model: Updated the logic for capping positive position price impact so that it also applies when there is zero price impact.
- sdk: Enabled oracle price updates to be posted in parallel.
- cli: Automatically apply global ALTs to constructed transactions.
- cli: Allowed `toggle-gt-minting` command to accept multiple market tokens.

### Removed

- sdk: Removed the `gmsol` crate and `gmsol-legacy` cli.

## [0.6.0] - 2025-06-23

### Breaking Changes

- programs: Upgraded to `anchor v0.31.1` and `solana v2.1.21`.
- programs(store): Removed support for Chainlink data feeds.
- programs(store): Refactored `TradeFlag` into the `gmsol-utils` crate.
- programs(store): Refactored `MarketFlag` into the `gmsol-utils` crate.
- programs(store): Refactored GT related `Flag`s into the `gmsol-utils` crate.
- programs(store): Refactored `PriceFlag` into the `gmsol-utils` crate.
- programs(store): Refactored `OracleFlag` into the `gmsol-utils` crate.
- programs(store): Refactored `UserFlag` into the `gmsol-utils` crate.
- programs(store): Refactroed `PriceFeedPrice` into the `gmsol-utils` crate.
- programs(timelock): Refactored `InstructionFlag` into the `gmsol-utils` crate.
- model: Added `paid_in_secondary_output_amount` and `is_collateral_token_long` parameters to the `on_insufficient_funding_fee_payment` function.
- model: The `PoolDelta::price_impact` function now returns a `PriceImpact` structure that includes a `BalanceChange`, instead of just the price impact value.
- model: Updated the fee factor logic to depend on `BalanceChange` rather than the sign of price impact; the `FeeParams::factor` function (along with other related functions) now takes `BalanceChange` instead of the `is_positive_impact` flag.
- sdk: Removed support for `spl-governance`.
- sdk(sdk): Updated the `ExchangeOps::update_order` function to include a `hint` parameter and return a future.
- sdk(solana-utils): Added `append` argument to `TransactionBuilder::pre_instruction` and `TransactionBuilder::pre_instructions`.
- sdk(programs): Added the `utils` feature and made the `utils` module available only when it's enabled.
- sdk(sdk): Moved the `serde` module to the crate root.
- sdk(sdk): Added amount type definitions to `utils`.
- sdk(sdk): Modified all market fetching methods to return `Arc<Market>`.
- sdk(solana-utils): Removed incorrect options from `SendBundleOptions`.
- sdk(solana-utils): Introduced a `before_sign` callback in transaction construction methods.
- sdk(sdk): Removed the `Copy` implementation from `CloseOrderHint`.
- sdk(sdk): Added callback support for the new order instruction builders.
- sdk(solana-utils): Updated `Cluster` serialization to match CLI behavior.
- cli(gmsol): Renamed the legacy CLI binary from `gmsol` to `gmsol-legacy`.

### Added

- programs: Defined the callback interface and added an example `gmsol-callback` program.
- programs(store): Added callback-enabled instructions for order.
  - Added the `create_order_v2` instruction.
  - Added the `update_order_v2` instruction.
  - Added the `close_order_v2` instruction.
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
- sdk(sdk): Migrated the implementation of squads support to `gmsol-sdk`.
- sdk(programs): Added `gmsol-competition` support under `competition` feature.
- sdk(sdk): Added support for `gmsol-competition` via `CompetitionOps`, under `competition` feature.
- sdk(sdk): Added `StoreOps::initialize_callback_authority` function.
- sdk(sdk): Added `KeyedAccount`, implementing `gmsol_decode::AccountAccess`.
- sdk(sdk): Added ALT support for `CreateOrderBuilder`.
- sdk(sdk): Added list query methods for all remaining action types.
- sdk(sdk): Added `IdlOps` to support IDL account operations and implemented it for `Client`.
- sdk(sdk): Added `Borrow<Pubkey>` and `Display` implementations for `StringPubkey`.
- sdk(sdk): Implemented `HasMarketMeta` for `Market` type (from `gmsol-programs`).
- sdk(chainlink-datastreams): Added `FromChainlinkReport` trait.
- sdk(chainlink-datastreams): Added `FromChainlinkReport` implementation for `PriceFeedPrice` behind the `gmsol` feature.
- docs(examples): Added examples of using `gmsol-sdk`.
- just: Added the `cli` recipe for running the `gmsol` commands.

### Fixed

- programs(treasury): Corrected data error in GT buyback message.
- sdk(solana-utils): Fixed missing payer signer in transaction size calculation.

### Deprecated

- programs(store): Deprecated `create_order`, `update_order`, `close_order`, `execute_increase_or_swap_order` and `execute_decrease_order` instructions.
- programs(store): Deprecated `confirm_gt_exchange_vault` instruction.
- programs(store): Deprecated `set_feed_config` instruction.
- programs(treasury): Deprecated `sync_gt_bank` instruction.

### Removed

- just: Removed the `build-idls-no-docs` recipe.

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
- sdk: Implemented the `MakeBundleBuilder` trait for `TransactionBuilder`.
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

[unreleased]: https://github.com/gmsol-labs/gmx-solana/compare/v0.11.0...HEAD
[0.11.0]: https://github.com/gmsol-labs/gmx-solana/releases/tag/v0.11.0
[0.10.0]: https://github.com/gmsol-labs/gmx-solana/releases/tag/v0.10.0
[0.9.1]: https://github.com/gmsol-labs/gmx-solana/releases/tag/programs-v0.9.1
[0.9.0]: https://github.com/gmsol-labs/gmx-solana/releases/tag/v0.9.0
[0.8.0]: https://github.com/gmsol-labs/gmx-solana/releases/tag/v0.8.0
[0.7.1]: https://github.com/gmsol-labs/gmx-solana/releases/tag/v0.7.1
[0.7.0]: https://github.com/gmsol-labs/gmx-solana/releases/tag/v0.7.0
[0.6.0]: https://github.com/gmsol-labs/gmx-solana/releases/tag/v0.6.0
[0.5.0]: https://github.com/gmsol-labs/gmx-solana/releases/tag/v0.5.0
[0.4.0]: https://github.com/gmsol-labs/gmx-solana/releases/tag/v0.4.0
[0.3.0]: https://github.com/gmsol-labs/gmx-solana/releases/tag/v0.3.0
