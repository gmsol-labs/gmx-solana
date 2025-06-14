use std::collections::BTreeMap;

use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, TokenAccount};
use gmsol_model::{
    price::Prices, Bank, BorrowingFeeMarketMutExt, LiquidityMarketMutExt, MarketAction,
    PerpMarketMutExt, PositionImpactMarketMutExt,
};
use typed_builder::TypedBuilder;

use crate::{
    events::{DepositExecuted, EventEmitter, MarketFeesUpdated, WithdrawalExecuted},
    states::{
        common::swap::{SwapActionParams, SwapActionParamsExt},
        deposit::DepositActionParams,
        market::{
            revertible::{
                liquidity_market::RevertibleLiquidityMarket,
                market::SwapPricingKind,
                swap_market::{SwapDirection, SwapMarkets},
                Revertible, RevertibleMarket, RevertibleVirtualInventories, Revision,
            },
            utils::ValidateMarketBalances,
            virtual_inventory::VirtualInventory,
            HasMarketMeta,
        },
        withdrawal::WithdrawalActionParams,
        Deposit, Market, Oracle, ShiftActionParams, Store,
    },
    CoreError, ModelError,
};

pub(crate) type SwapMarketLoaders<'info> = [AccountLoader<'info, Market>];
pub(crate) type VirtualInventoryLoaders<'info> =
    BTreeMap<&'info Pubkey, AccountLoader<'info, VirtualInventory>>;

pub(crate) struct RemainingAccountsForMarket<'info> {
    swap_markets: Vec<AccountLoader<'info, Market>>,
    virtual_inventories: VirtualInventoryLoaders<'info>,
}

impl<'info> RemainingAccountsForMarket<'info> {
    pub(crate) fn new(
        remaining_accounts: &'info [AccountInfo<'info>],
        current_market_token: Pubkey,
        swap: Option<&SwapActionParams>,
    ) -> Result<Self> {
        let (swap_markets, remaining_accounts) = match swap {
            Some(swap) => {
                swap.unpack_markets_for_swap(&current_market_token, remaining_accounts)?
            }
            None => (Vec::default(), remaining_accounts),
        };
        // Note: currently all remaining accounts are assumed to be `VirtualInventory` accounts.
        let virtual_inventories = remaining_accounts
            .iter()
            .map(|info| Ok((info.key, AccountLoader::<VirtualInventory>::try_from(info)?)))
            .collect::<Result<_>>()?;
        Ok(Self {
            swap_markets,
            virtual_inventories,
        })
    }

    pub(crate) fn swap_market_loaders(&self) -> &SwapMarketLoaders<'info> {
        &self.swap_markets
    }

    pub(crate) fn load_virtual_inventories(&self) -> Result<RevertibleVirtualInventories<'info>> {
        RevertibleVirtualInventories::from_loaders(&self.virtual_inventories)
    }
}

/// Operation for transferring funds into market valut.
#[derive(TypedBuilder)]
pub(crate) struct MarketTransferInOperation<'a, 'info> {
    store: &'a AccountLoader<'info, Store>,
    market: &'a AccountLoader<'info, Market>,
    from: AccountInfo<'info>,
    from_authority: AccountInfo<'info>,
    vault: &'a Account<'info, TokenAccount>,
    amount: u64,
    token_program: AccountInfo<'info>,
    signer_seeds: &'a [&'a [u8]],
    #[builder(setter(into))]
    event_emitter: EventEmitter<'a, 'info>,
}

impl MarketTransferInOperation<'_, '_> {
    pub(crate) fn execute(self) -> Result<()> {
        use anchor_spl::token;

        self.market.load()?.validate(&self.store.key())?;

        let amount = self.amount;
        if amount != 0 {
            token::transfer(
                CpiContext::new(
                    self.token_program,
                    token::Transfer {
                        from: self.from,
                        to: self.vault.to_account_info(),
                        authority: self.from_authority,
                    },
                )
                .with_signer(&[self.signer_seeds]),
                amount,
            )?;
            let token = &self.vault.mint;
            let mut market = RevertibleMarket::new(
                self.market,
                // Virtual inventory feature is not required here.
                None,
                self.event_emitter,
            )?;
            market
                .record_transferred_in_by_token(token, &amount)
                .map_err(ModelError::from)?;
            market.commit();
        }

        Ok(())
    }
}

/// Operation for transferring funds out of market vault.
#[derive(TypedBuilder)]
pub(crate) struct MarketTransferOutOperation<'a, 'info> {
    store: &'a AccountLoader<'info, Store>,
    market: &'a AccountLoader<'info, Market>,
    amount: u64,
    decimals: u8,
    to: AccountInfo<'info>,
    token_mint: AccountInfo<'info>,
    vault: AccountInfo<'info>,
    token_program: AccountInfo<'info>,
    #[builder(setter(into))]
    event_emitter: EventEmitter<'a, 'info>,
}

impl MarketTransferOutOperation<'_, '_> {
    pub(crate) fn execute(self) -> Result<()> {
        use crate::utils::internal::TransferUtils;

        {
            let market = self.market.load()?;
            let meta = market.validated_meta(&self.store.key())?;
            require!(
                meta.is_collateral_token(&self.token_mint.key()),
                CoreError::InvalidCollateralToken
            );
        }

        let amount = self.amount;
        if amount != 0 {
            let decimals = self.decimals;
            TransferUtils::new(
                self.token_program.to_account_info(),
                self.store,
                self.token_mint.to_account_info(),
            )
            .transfer_out(self.vault.to_account_info(), self.to, amount, decimals)?;
            let token = &self.token_mint.key();
            let mut market = RevertibleMarket::new(
                self.market,
                // Virtual inventory feature is not required here.
                None,
                self.event_emitter,
            )?;
            market
                .record_transferred_out_by_token(token, &amount)
                .map_err(ModelError::from)?;
            market.commit();
        }

        Ok(())
    }
}

/// Revertible Liquidity Market Operation.
pub struct RevertibleLiquidityMarketOperation<'a, 'info> {
    store: &'a AccountLoader<'info, Store>,
    oracle: &'a Oracle,
    market: &'a AccountLoader<'info, Market>,
    market_token_mint: &'a mut Account<'info, Mint>,
    token_program: AccountInfo<'info>,
    swap: Option<&'a SwapActionParams>,
    swap_market_loaders: &'a SwapMarketLoaders<'info>,
    virtual_inventories: &'a RevertibleVirtualInventories<'info>,
    event_emitter: EventEmitter<'a, 'info>,
}

impl<'a, 'info> RevertibleLiquidityMarketOperation<'a, 'info> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        store: &'a AccountLoader<'info, Store>,
        oracle: &'a Oracle,
        market: &'a AccountLoader<'info, Market>,
        market_token_mint: &'a mut Account<'info, Mint>,
        token_program: AccountInfo<'info>,
        swap: Option<&'a SwapActionParams>,
        swap_market_loaders: &'a SwapMarketLoaders<'info>,
        virtual_inventories: &'a RevertibleVirtualInventories<'info>,
        event_emitter: EventEmitter<'a, 'info>,
    ) -> Result<Self> {
        Ok(Self {
            store,
            oracle,
            market,
            market_token_mint,
            token_program,
            swap,
            swap_market_loaders,
            virtual_inventories,
            event_emitter,
        })
    }
}

impl<'info> RevertibleLiquidityMarketOperation<'_, 'info> {
    pub(crate) fn op<'ctx>(&'ctx mut self) -> Result<Execute<'ctx, 'info>> {
        let current_market_token = self.market_token_mint.key();

        let market = RevertibleLiquidityMarket::from_revertible_market(
            RevertibleMarket::new(
                self.market,
                Some(self.virtual_inventories),
                self.event_emitter,
            )?,
            self.market_token_mint,
            &self.token_program,
            self.store,
        )?;
        let swap_markets = SwapMarkets::new(
            &self.store.key(),
            self.swap_market_loaders,
            Some(&current_market_token),
            self.virtual_inventories,
            self.event_emitter,
        )?;
        Ok(Execute {
            output: (),
            oracle: self.oracle,
            swap: self.swap,
            market,
            swap_markets,
            event_emitter: self.event_emitter,
        })
    }
}

#[must_use = "Revertible operation must be committed to take effect"]
pub(crate) struct Execute<'a, 'info, T = ()> {
    pub(crate) output: T,
    oracle: &'a Oracle,
    swap: Option<&'a SwapActionParams>,
    market: RevertibleLiquidityMarket<'a, 'info>,
    swap_markets: SwapMarkets<'a, 'info>,
    event_emitter: EventEmitter<'a, 'info>,
}

impl<'a, 'info, T> Execute<'a, 'info, T> {
    pub(crate) fn with_output<U>(self, output: U) -> Execute<'a, 'info, U> {
        let Self {
            oracle,
            swap,
            market,
            swap_markets,
            event_emitter: event_authority,
            ..
        } = self;

        Execute {
            output,
            oracle,
            swap,
            market,
            swap_markets,
            event_emitter: event_authority,
        }
    }

    pub(crate) fn market(&self) -> &RevertibleLiquidityMarket<'a, 'info> {
        &self.market
    }

    pub(crate) fn market_mut(&mut self) -> &mut RevertibleLiquidityMarket<'a, 'info> {
        &mut self.market
    }

    pub(crate) fn swap_markets(&self) -> &SwapMarkets<'_, 'info> {
        &self.swap_markets
    }

    fn pre_execute(&mut self, prices: &Prices<u128>) -> Result<()> {
        // Distribute position impact.
        let distribute_position_impact = self
            .market
            .distribute_position_impact()
            .map_err(ModelError::from)?
            .execute()
            .map_err(ModelError::from)?;

        if *distribute_position_impact.distribution_amount() != 0 {
            msg!("[Pre-execute] position impact distributed");
        }

        // Update borrowing state.
        let borrowing = self
            .market
            .base_mut()
            .update_borrowing(prices)
            .and_then(|a| a.execute())
            .map_err(ModelError::from)?;
        msg!("[Pre-execute] borrowing state updated");

        // Update funding state.
        let funding = self
            .market
            .base_mut()
            .update_funding(prices)
            .and_then(|a| a.execute())
            .map_err(ModelError::from)?;
        msg!("[Pre-execute] funding state updated");

        self.event_emitter
            .emit_cpi(&MarketFeesUpdated::from_reports(
                self.market.rev(),
                self.market.market_meta().market_token_mint,
                distribute_position_impact,
                borrowing,
                funding,
            ))?;
        Ok(())
    }

    fn validate_first_deposit(
        &self,
        receiver: &Pubkey,
        params: &DepositActionParams,
    ) -> Result<()> {
        if self.market().market_token().supply == 0 {
            Deposit::validate_first_deposit(
                receiver,
                params.min_market_token_amount,
                self.market().base().as_ref(),
            )?;
        }

        Ok(())
    }

    /// Swap and deposit into the current market.
    ///
    /// # CHECK
    /// - `market_token_receiver` must be the correct escrow
    ///   account for market token.
    ///
    /// # Errors
    /// - Error if first deposit validation failed.
    #[inline(never)]
    pub(crate) fn unchecked_deposit(
        mut self,
        receiver: &Pubkey,
        market_token_receiver: &'a AccountInfo<'info>,
        params: &DepositActionParams,
        initial_tokens: (Option<Pubkey>, Option<Pubkey>),
        swap_pricing_kind: Option<SwapPricingKind>,
    ) -> Result<Execute<'a, 'info, u64>> {
        self.validate_first_deposit(receiver, params)?;

        self.market = self
            .market
            .enable_mint(market_token_receiver)
            .with_swap_pricing_kind(swap_pricing_kind.unwrap_or(SwapPricingKind::Deposit));

        let prices = self.oracle.market_prices(&self.market)?;

        self.pre_execute(&prices)?;

        // Swap tokens into the target market.
        let (long_token_amount, short_token_amount) = {
            let meta = self.market.market_meta();
            let expected_token_outs = (meta.long_token_mint, meta.short_token_mint);

            match self.swap {
                Some(swap) => self.swap_markets.revertible_swap(
                    SwapDirection::Into(&mut self.market),
                    self.oracle,
                    swap,
                    expected_token_outs,
                    initial_tokens,
                    (
                        params.initial_long_token_amount,
                        params.initial_short_token_amount,
                    ),
                )?,
                None => {
                    if params.initial_long_token_amount != 0 {
                        require!(initial_tokens.0.is_none(), CoreError::InvalidArgument);
                    }
                    if params.initial_short_token_amount != 0 {
                        require!(initial_tokens.1.is_none(), CoreError::InvalidArgument);
                    }
                    (
                        params.initial_long_token_amount,
                        params.initial_short_token_amount,
                    )
                }
            }
        };

        // Perform the deposit.
        let minted = {
            let report = self
                .market
                .deposit(long_token_amount.into(), short_token_amount.into(), prices)
                .and_then(|d| d.execute())
                .map_err(ModelError::from)?;
            self.market.validate_market_balances(0, 0)?;

            let minted: u64 = (*report.minted())
                .try_into()
                .map_err(|_| error!(CoreError::TokenAmountOverflow))?;

            params.validate_market_token_amount(minted)?;

            self.event_emitter.emit_cpi(&DepositExecuted::from_report(
                self.market.rev(),
                self.market.market_meta().market_token_mint,
                report,
            ))?;
            msg!("[Deposit] executed");

            minted
        };

        Ok(self.with_output(minted))
    }

    /// Withdraw from the current market and swap.
    ///
    /// # CHECK
    ///
    /// # Errors
    ///
    pub(crate) fn unchecked_withdraw(
        mut self,
        market_token_vault: &'a AccountInfo<'info>,
        params: &WithdrawalActionParams,
        final_tokens: (Pubkey, Pubkey),
        swap_pricing_kind: Option<SwapPricingKind>,
    ) -> Result<Execute<'a, 'info, (u64, u64)>> {
        self.market = self
            .market
            .enable_burn(market_token_vault)
            .with_swap_pricing_kind(swap_pricing_kind.unwrap_or(SwapPricingKind::Withdrawal));

        let prices = self.oracle.market_prices(&self.market)?;

        self.pre_execute(&prices)?;

        // Perform the withdrawal.
        let (long_amount, short_amount) = {
            let report = self
                .market
                .withdraw(params.market_token_amount.into(), prices)
                .and_then(|w| w.execute())
                .map_err(ModelError::from)?;
            let (long_amount, short_amount) = (
                (*report.long_token_output())
                    .try_into()
                    .map_err(|_| CoreError::TokenAmountOverflow)?,
                (*report.short_token_output())
                    .try_into()
                    .map_err(|_| CoreError::TokenAmountOverflow)?,
            );
            // Validate current market.
            self.market
                .validate_market_balances(long_amount, short_amount)?;

            self.event_emitter
                .emit_cpi(&WithdrawalExecuted::from_report(
                    self.market.rev(),
                    self.market.market_meta().market_token_mint,
                    report,
                ))?;
            msg!("[Withdrawal] executed");

            (long_amount, short_amount)
        };

        // Perform the swap.
        let (final_long_amount, final_short_amount) = {
            let meta = *self.market.market_meta();
            match self.swap {
                Some(swap) => self.swap_markets.revertible_swap(
                    SwapDirection::From(&mut self.market),
                    self.oracle,
                    swap,
                    final_tokens,
                    (Some(meta.long_token_mint), Some(meta.short_token_mint)),
                    (long_amount, short_amount),
                )?,
                None => {
                    require!(
                        final_tokens == (meta.long_token_mint, meta.short_token_mint),
                        CoreError::InvalidSwapPath
                    );
                    (long_amount, short_amount)
                }
            }
        };

        params.validate_output_amounts(final_long_amount, final_short_amount)?;

        Ok(self.with_output((final_long_amount, final_short_amount)))
    }

    fn take_output<U>(self, new_output: U) -> (Execute<'a, 'info, U>, T) {
        let Self {
            output,
            oracle,
            swap,
            market,
            swap_markets,
            event_emitter: event_authority,
        } = self;

        (
            Execute {
                output: new_output,
                oracle,
                swap,
                market,
                swap_markets,
                event_emitter: event_authority,
            },
            output,
        )
    }

    /// Shift market tokens.
    /// # CHECK
    ///
    /// # Errors
    ///
    pub(crate) fn unchecked_shift(
        self,
        mut to_market: Self,
        receiver: &Pubkey,
        params: &ShiftActionParams,
        from_market_token_vault: &'a AccountInfo<'info>,
        to_market_token_account: &'a AccountInfo<'info>,
    ) -> Result<(Self, Self, u64)> {
        let meta = self.market().market_meta();
        let (long_token, short_token) = (meta.long_token_mint, meta.short_token_mint);

        // Perform the shift-withdrawal.
        let (mut from_market, (long_amount, short_amount)) = {
            let (op, output) = self.take_output(());
            let mut withdrawal_params = WithdrawalActionParams::default();
            withdrawal_params.market_token_amount = params.from_market_token_amount;
            op.unchecked_withdraw(
                from_market_token_vault,
                &withdrawal_params,
                (long_token, short_token),
                Some(SwapPricingKind::Shift),
            )?
            .take_output(output)
        };

        // Transfer tokens from the `from_market` to `to_market`.
        // The vaults are assumed to be shared.
        {
            from_market
                .market_mut()
                .record_transferred_out_by_token(&long_token, &long_amount)
                .map_err(ModelError::from)?;
            to_market
                .market_mut()
                .record_transferred_in_by_token(&long_token, &long_amount)
                .map_err(ModelError::from)?;

            from_market
                .market_mut()
                .record_transferred_out_by_token(&short_token, &short_amount)
                .map_err(ModelError::from)?;
            to_market
                .market_mut()
                .record_transferred_in_by_token(&short_token, &short_amount)
                .map_err(ModelError::from)?;
        }

        // Perform the shift-deposit.
        let (to_market, received) = {
            let (op, output) = to_market.take_output(());
            let mut deposit_params = DepositActionParams::default();
            deposit_params.initial_long_token_amount = long_amount;
            deposit_params.initial_short_token_amount = short_amount;
            deposit_params.min_market_token_amount = params.min_to_market_token_amount;
            op.unchecked_deposit(
                receiver,
                to_market_token_account,
                &deposit_params,
                (None, None),
                Some(SwapPricingKind::Shift),
            )?
            .take_output(output)
        };

        Ok((from_market, to_market, received))
    }
}

impl<T> Revertible for Execute<'_, '_, T> {
    fn commit(self) {
        self.market.commit();
        self.swap_markets.commit();
    }
}
