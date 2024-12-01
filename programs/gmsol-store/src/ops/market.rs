use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, TokenAccount};
use gmsol_model::{Bank, LiquidityMarketMutExt, PositionImpactMarketMutExt};
use typed_builder::TypedBuilder;

use crate::{
    states::{
        common::swap::SwapParams,
        deposit::DepositParams,
        market::{
            revertible::{
                liquidity_market::RevertibleLiquidityMarket,
                swap_market::{SwapDirection, SwapMarkets},
                Revertible,
            },
            utils::ValidateMarketBalances,
            HasMarketMeta,
        },
        withdrawal::WithdrawalParams,
        Market, Oracle, ShiftParams, Store,
    },
    CoreError, ModelError,
};

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
}

impl<'a, 'info> MarketTransferInOperation<'a, 'info> {
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
            self.market
                .load_mut()?
                .record_transferred_in_by_token(token, amount)?;
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
}

impl<'a, 'info> MarketTransferOutOperation<'a, 'info> {
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
            self.market
                .load_mut()?
                .record_transferred_out_by_token(token, amount)?;
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
    swap: Option<&'a SwapParams>,
    swap_markets: Vec<AccountLoader<'info, Market>>,
}

impl<'a, 'info> RevertibleLiquidityMarketOperation<'a, 'info> {
    pub(crate) fn new(
        store: &'a AccountLoader<'info, Store>,
        oracle: &'a Oracle,
        market: &'a AccountLoader<'info, Market>,
        market_token_mint: &'a mut Account<'info, Mint>,
        token_program: AccountInfo<'info>,
        swap: Option<&'a SwapParams>,
        remaining_accounts: &'info [AccountInfo<'info>],
    ) -> Result<Self> {
        let swap_markets = swap
            .map(|swap| swap.unpack_markets_for_swap(&market_token_mint.key(), remaining_accounts))
            .transpose()?
            .unwrap_or_default();

        Ok(Self {
            store,
            oracle,
            market,
            market_token_mint,
            token_program,
            swap,
            swap_markets,
        })
    }
}

impl<'a, 'info> RevertibleLiquidityMarketOperation<'a, 'info> {
    pub(crate) fn op<'ctx>(&'ctx mut self) -> Result<Execute<'ctx, 'info>> {
        let current_market_token = self.market_token_mint.key();
        let market = RevertibleLiquidityMarket::from_revertible_market(
            self.market.try_into()?,
            self.market_token_mint,
            &self.token_program,
            self.store,
        )?;
        let swap_markets = SwapMarkets::new(
            &self.store.key(),
            &self.swap_markets,
            Some(&current_market_token),
        )?;
        Ok(Execute {
            output: (),
            oracle: self.oracle,
            swap: self.swap,
            market,
            swap_markets,
        })
    }
}

#[must_use = "Revertible operation must be committed to take effect"]
pub(crate) struct Execute<'a, 'info, T = ()> {
    pub(crate) output: T,
    oracle: &'a Oracle,
    swap: Option<&'a SwapParams>,
    market: RevertibleLiquidityMarket<'a, 'info>,
    swap_markets: SwapMarkets<'a>,
}

impl<'a, 'info, T> Execute<'a, 'info, T> {
    pub(crate) fn with_output<U>(self, output: U) -> Execute<'a, 'info, U> {
        let Self {
            oracle,
            swap,
            market,
            swap_markets,
            ..
        } = self;

        Execute {
            output,
            oracle,
            swap,
            market,
            swap_markets,
        }
    }

    pub(crate) fn market(&self) -> &RevertibleLiquidityMarket<'a, 'info> {
        &self.market
    }

    pub(crate) fn market_mut(&mut self) -> &mut RevertibleLiquidityMarket<'a, 'info> {
        &mut self.market
    }

    pub(crate) fn swap_markets(&self) -> &SwapMarkets {
        &self.swap_markets
    }

    /// Swap and deposit into the current market.
    ///
    /// # CHECK
    ///
    /// # Errors
    #[inline(never)]
    pub(crate) fn unchecked_deposit(
        mut self,
        market_token_receiver: &'a AccountInfo<'info>,
        params: &DepositParams,
        initial_tokens: (Option<Pubkey>, Option<Pubkey>),
    ) -> Result<Execute<'a, 'info, u64>> {
        self.market = self.market.enable_mint(market_token_receiver);

        // Distribute position impact.
        {
            let report = self
                .market
                .distribute_position_impact()
                .map_err(ModelError::from)?
                .execute()
                .map_err(ModelError::from)?;
            msg!("[Deposit] pre-execute: {:?}", report);
        }

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
            let prices = self.oracle.market_prices(&self.market)?;
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

            msg!("[Deposit] executed: {:?}", report);

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
        params: &WithdrawalParams,
        final_tokens: (Pubkey, Pubkey),
    ) -> Result<Execute<'a, 'info, (u64, u64)>> {
        self.market = self.market.enable_burn(market_token_vault);

        // Distribute position impact.
        {
            let report = self
                .market
                .distribute_position_impact()
                .map_err(ModelError::from)?
                .execute()
                .map_err(ModelError::from)?;
            msg!("[Withdrawal] pre-execute: {:?}", report);
        }

        // Perform the withdrawal.
        let (long_amount, short_amount) = {
            let prices = self.oracle.market_prices(&self.market)?;
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
            msg!("[Withdrawal] executed: {:?}", report);
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
        } = self;

        (
            Execute {
                output: new_output,
                oracle,
                swap,
                market,
                swap_markets,
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
        params: &ShiftParams,
        from_market_token_vault: &'a AccountInfo<'info>,
        to_market_token_account: &'a AccountInfo<'info>,
    ) -> Result<(Self, Self, u64)> {
        let meta = self.market().market_meta();
        let (long_token, short_token) = (meta.long_token_mint, meta.short_token_mint);

        // Perform the shift-withdrawal.
        let (mut from_market, (long_amount, short_amount)) = {
            let (op, output) = self.take_output(());
            let mut withdrawal_params = WithdrawalParams::default();
            withdrawal_params.market_token_amount = params.from_market_token_amount;
            op.unchecked_withdraw(
                from_market_token_vault,
                &withdrawal_params,
                (long_token, short_token),
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
            let mut deposit_params = DepositParams::default();
            deposit_params.initial_long_token_amount = long_amount;
            deposit_params.initial_short_token_amount = short_amount;
            deposit_params.min_market_token_amount = params.min_to_market_token_amount;
            op.unchecked_deposit(to_market_token_account, &deposit_params, (None, None))?
                .take_output(output)
        };

        Ok((from_market, to_market, received))
    }
}

impl<'a, 'info, T> Revertible for Execute<'a, 'info, T> {
    fn commit(self) {
        self.market.commit();
        self.swap_markets.commit();
    }
}
