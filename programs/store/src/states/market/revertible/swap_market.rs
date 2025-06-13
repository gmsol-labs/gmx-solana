use anchor_lang::prelude::*;
use gmsol_model::{Bank, BorrowingFeeMarketMutExt, MarketAction, SwapMarketMutExt};
use indexmap::{map::Entry, IndexMap};

use crate::{
    constants,
    events::{BorrowingFeesUpdated, EventEmitter, SwapExecuted},
    states::{
        common::swap::SwapActionParams, market::utils::ValidateMarketBalances, HasMarketMeta,
        Market, Oracle,
    },
    CoreError, ModelError,
};

use super::{market::RevertibleMarket, Revertible, RevertibleVirtualInventories, Revision};

/// A map of markets used for swaps where the key is the market token mint address.
pub struct SwapMarkets<'a, 'info> {
    markets: IndexMap<Pubkey, RevertibleMarket<'a, 'info>>,
    event_emitter: EventEmitter<'a, 'info>,
}

impl<'a, 'info> SwapMarkets<'a, 'info> {
    /// Create a new [`SwapMarkets`] from loders.
    pub(crate) fn new(
        store: &Pubkey,
        loaders: &'a [AccountLoader<'info, Market>],
        current_market_token: Option<&Pubkey>,
        virtual_inventories: &'a RevertibleVirtualInventories<'info>,
        event_emitter: EventEmitter<'a, 'info>,
    ) -> Result<Self> {
        let mut map = IndexMap::with_capacity(loaders.len());
        for loader in loaders {
            let key = loader.load()?.meta().market_token_mint;
            if let Some(market_token) = current_market_token {
                require!(key != *market_token, CoreError::InvalidSwapPath);
            }
            match map.entry(key) {
                // Cannot have duplicated markets.
                Entry::Occupied(_) => return err!(CoreError::InvalidSwapPath),
                Entry::Vacant(e) => {
                    loader.load()?.validate(store)?;
                    let market =
                        RevertibleMarket::new(loader, Some(virtual_inventories), event_emitter)?;
                    e.insert(market);
                }
            }
        }
        Ok(Self {
            markets: map,
            event_emitter,
        })
    }

    /// Get market mutably.
    pub fn get_mut(&mut self, token: &Pubkey) -> Option<&mut RevertibleMarket<'a, 'info>> {
        self.markets.get_mut(token)
    }

    /// Get market.
    pub fn get(&self, token: &Pubkey) -> Option<&RevertibleMarket<'a, 'info>> {
        self.markets.get(token)
    }

    /// Revertible Swap.
    pub(crate) fn revertible_swap<M>(
        &mut self,
        mut direction: SwapDirection<M>,
        oracle: &Oracle,
        params: &SwapActionParams,
        expected_token_outs: (Pubkey, Pubkey),
        token_ins: (Option<Pubkey>, Option<Pubkey>),
        token_in_amounts: (u64, u64),
    ) -> Result<(u64, u64)>
    where
        M: Key
            + Revision
            + HasMarketMeta
            + gmsol_model::Bank<Pubkey, Num = u64>
            + gmsol_model::SwapMarketMut<{ constants::MARKET_DECIMALS }, Num = u128>
            + gmsol_model::BorrowingFeeMarketMut<{ constants::MARKET_DECIMALS }>,
    {
        let long_path = params
            .validated_primary_swap_path()
            .map_err(CoreError::from)?;
        let long_output_amount = token_ins
            .0
            .and_then(|token| (token_in_amounts.0 != 0).then_some(token))
            .map(|token_in| {
                self.revertible_swap_for_one_side(
                    &mut direction,
                    oracle,
                    long_path,
                    expected_token_outs.0,
                    token_in,
                    token_in_amounts.0,
                )
            })
            .transpose()?
            .unwrap_or_default();
        let short_path = params
            .validated_secondary_swap_path()
            .map_err(CoreError::from)?;
        let short_output_amount = token_ins
            .1
            .and_then(|token| (token_in_amounts.1 != 0).then_some(token))
            .map(|token_in| {
                self.revertible_swap_for_one_side(
                    &mut direction,
                    oracle,
                    short_path,
                    expected_token_outs.1,
                    token_in,
                    token_in_amounts.1,
                )
            })
            .transpose()?
            .unwrap_or_default();

        // Validate token balances of output markets and current market.
        let current = direction.current();
        let mut long_output_market_token = long_path.last().unwrap_or(&current);
        let mut short_output_market_token = short_path.last().unwrap_or(&current);

        // When the swap direction is swapping into current market,
        // the balances should have been transferred into current market.
        if direction.is_into() {
            long_output_market_token = &current;
            short_output_market_token = &current;
        }

        let mut current_validated = false;
        if *long_output_market_token == *short_output_market_token {
            if *long_output_market_token != current {
                self.get(long_output_market_token)
                    .expect("must exist")
                    .validate_market_balances_excluding_the_given_token_amounts(
                        &expected_token_outs.0,
                        &expected_token_outs.1,
                        long_output_amount,
                        short_output_amount,
                    )?;
            } else {
                direction
                    .current_market()
                    .validate_market_balances_excluding_the_given_token_amounts(
                        &expected_token_outs.0,
                        &expected_token_outs.1,
                        long_output_amount,
                        short_output_amount,
                    )?;
                current_validated = true;
            }
        } else {
            for (market_token, amount, token) in [
                (
                    long_output_market_token,
                    long_output_amount,
                    &expected_token_outs.0,
                ),
                (
                    short_output_market_token,
                    short_output_amount,
                    &expected_token_outs.1,
                ),
            ] {
                if *market_token != current {
                    self.get(market_token)
                        .expect("must exist")
                        .validate_market_balances_excluding_the_given_token_amounts(
                            token, token, amount, 0,
                        )?;
                } else {
                    direction
                        .current_market()
                        .validate_market_balances_excluding_the_given_token_amounts(
                            token, token, amount, 0,
                        )?;
                    current_validated = true;
                }
            }
        }
        if !current_validated {
            direction.current_market().validate_market_balances(0, 0)?;
        }
        Ok((long_output_amount, short_output_amount))
    }

    /// Swap along the path.
    ///
    /// ## Assumptions
    /// - The input amount is already deposited in the first market.
    /// - The path consists of the mint addresses of unique market tokens.
    ///
    /// ## Notes
    /// - The output amount will also remain deposited in the last market.
    fn swap_along_the_path(
        &mut self,
        oracle: &Oracle,
        path: &[Pubkey],
        token_in: &mut Pubkey,
        token_in_amount: &mut u64,
    ) -> Result<()> {
        if path.is_empty() {
            return Ok(());
        }
        let last_idx = path.len().saturating_sub(1);
        for (idx, market_token) in path.iter().enumerate() {
            let market = self.markets.get_mut(market_token).ok_or_else(|| {
                msg!("Swap Error: missing market account for {}", market_token);
                error!(CoreError::MarketAccountIsNotProvided)
            })?;
            if idx != 0 {
                market
                    .record_transferred_in_by_token(token_in, token_in_amount)
                    .map_err(ModelError::from)?;
            }
            let side = market
                .market_meta()
                .to_token_side(token_in)
                .map_err(CoreError::from)?;
            let prices = oracle.market_prices(market)?;
            // Update borrowing state.
            {
                let report = market
                    .update_borrowing(&prices)
                    .map_err(ModelError::from)?
                    .execute()
                    .map_err(ModelError::from)?;
                market
                    .event_emitter()
                    .emit_cpi(&BorrowingFeesUpdated::from_report(
                        market.rev(),
                        *market_token,
                        report,
                    ))?;
            }
            let report = market
                .swap(side, (*token_in_amount).into(), prices)
                .map_err(ModelError::from)?
                .execute()
                .map_err(ModelError::from)?;
            *token_in = *market
                .market_meta()
                .opposite_token(token_in)
                .map_err(CoreError::from)?;
            *token_in_amount = (*report.token_out_amount())
                .try_into()
                .map_err(|_| error!(CoreError::TokenAmountOverflow))?;
            // Only validate the market without extra balances.
            if idx != last_idx {
                market
                    .record_transferred_out_by_token(token_in, token_in_amount)
                    .map_err(ModelError::from)?;
                market.validate_market_balances(0, 0)?;
            }
            market.event_emitter().emit_cpi(&SwapExecuted::new(
                market.rev(),
                *market_token,
                report,
                None,
            ))?;
        }
        msg!("[Swap] swapped along the path");
        Ok(())
    }

    /// Swap for one side.
    ///
    /// ## Assumption
    /// - The market tokens in the path must be unique.
    fn revertible_swap_for_one_side<M>(
        &mut self,
        direction: &mut SwapDirection<M>,
        oracle: &Oracle,
        mut path: &[Pubkey],
        expected_token_out: Pubkey,
        mut token_in: Pubkey,
        mut token_in_amount: u64,
    ) -> Result<u64>
    where
        M: Key
            + Revision
            + gmsol_model::Bank<Pubkey, Num = u64>
            + gmsol_model::SwapMarketMut<{ constants::MARKET_DECIMALS }, Num = u128>
            + HasMarketMeta,
    {
        require!(
            self.get_mut(&direction.current()).is_none(),
            CoreError::InvalidSwapPath
        );
        if !path.is_empty() {
            let current = direction.current();

            let first_market_token = path.first().unwrap();
            if let SwapDirection::From(from_market) = direction {
                if *first_market_token != current {
                    let first_market = self.get_mut(first_market_token).ok_or_else(|| {
                        msg!(
                            "Swap Error: missing market account for {}",
                            first_market_token
                        );
                        error!(CoreError::MarketAccountIsNotProvided)
                    })?;
                    // We are assuming that they are sharing the same vault of `token_in`.
                    from_market
                        .record_transferred_out_by_token(&token_in, &token_in_amount)
                        .map_err(ModelError::from)?;
                    from_market
                        .validate_market_balance_for_the_given_token(&token_in, 0)
                        .map_err(ModelError::from)?;
                    first_market
                        .record_transferred_in_by_token(&token_in, &token_in_amount)
                        .map_err(ModelError::from)?;
                }
            }

            if *first_market_token == current {
                // The validation of current market is delayed.
                direction.swap_with_current(
                    oracle,
                    &mut token_in,
                    &mut token_in_amount,
                    &self.event_emitter,
                )?;
                path = &path[1..];
                if let Some(first_market_token_to_swap_at) = path.first() {
                    debug_assert!(*first_market_token_to_swap_at != current);
                    let first_market =
                        self.get_mut(first_market_token_to_swap_at).ok_or_else(|| {
                            msg!(
                                "Swap Error: missing market account for {}",
                                first_market_token
                            );
                            error!(CoreError::MarketAccountIsNotProvided)
                        })?;
                    // We are assuming that they are sharing the same vault of `token_in`.
                    direction
                        .current_market_mut()
                        .record_transferred_out_by_token(&token_in, &token_in_amount)
                        .map_err(ModelError::from)?;
                    // The validation of current market is delayed.
                    first_market
                        .record_transferred_in_by_token(&token_in, &token_in_amount)
                        .map_err(ModelError::from)?;
                }
            }

            if !path.is_empty() {
                let mut should_swap_with_current = false;
                let last_market_token = path.last().unwrap();

                if *last_market_token == direction.current() {
                    should_swap_with_current = true;
                    path = path.split_last().unwrap().1;
                }

                self.swap_along_the_path(oracle, path, &mut token_in, &mut token_in_amount)?;

                if should_swap_with_current {
                    if let Some(last_swapped_market_token) = path.last() {
                        debug_assert!(*last_swapped_market_token != current);
                        let last_market =
                            self.get_mut(last_swapped_market_token).expect("must exist");
                        // We are assuming that they are sharing the same vault of `token_in`.
                        last_market
                            .record_transferred_out_by_token(&token_in, &token_in_amount)
                            .map_err(ModelError::from)?;
                        last_market.validate_market_balances(0, 0)?;
                        direction
                            .current_market_mut()
                            .record_transferred_in_by_token(&token_in, &token_in_amount)
                            .map_err(ModelError::from)?;
                    }
                    // The validation of current market is delayed.
                    direction.swap_with_current(
                        oracle,
                        &mut token_in,
                        &mut token_in_amount,
                        &self.event_emitter,
                    )?;
                }

                if let SwapDirection::Into(into_market) = direction {
                    if *last_market_token != current {
                        let last_market = self.get_mut(last_market_token).expect("must exist");
                        // We are assuming that they are sharing the same vault of `token_in`.
                        last_market
                            .record_transferred_out_by_token(&token_in, &token_in_amount)
                            .map_err(ModelError::from)?;
                        last_market.validate_market_balances(0, 0)?;
                        into_market
                            .record_transferred_in_by_token(&token_in, &token_in_amount)
                            .map_err(ModelError::from)?;
                    }
                }
            }
        }
        require_keys_eq!(token_in, expected_token_out, CoreError::InvalidSwapPath);
        Ok(token_in_amount)
    }
}

impl Revertible for SwapMarkets<'_, '_> {
    /// Commit the swap.
    /// ## Panic
    /// Panic if one of the commitments panics.
    fn commit(self) {
        for market in self.markets.into_values() {
            market.commit();
        }
    }
}

pub(crate) enum SwapDirection<'a, M> {
    From(&'a mut M),
    Into(&'a mut M),
}

impl<M> SwapDirection<'_, M>
where
    M: Key,
{
    fn current(&self) -> Pubkey {
        match self {
            Self::From(p) | Self::Into(p) => p.key(),
        }
    }
}

impl<M> Revision for SwapDirection<'_, M>
where
    M: Revision,
{
    fn rev(&self) -> u64 {
        match self {
            Self::From(p) | Self::Into(p) => p.rev(),
        }
    }
}

impl<M> SwapDirection<'_, M> {
    fn current_market_mut(&mut self) -> &mut M {
        match self {
            Self::From(m) | Self::Into(m) => m,
        }
    }

    fn current_market(&self) -> &M {
        match self {
            Self::From(m) | Self::Into(m) => m,
        }
    }

    fn is_into(&self) -> bool {
        matches!(self, Self::Into(_))
    }
}

impl<M> SwapDirection<'_, M>
where
    M: Key
        + Revision
        + HasMarketMeta
        + gmsol_model::SwapMarketMut<{ constants::MARKET_DECIMALS }, Num = u128>,
{
    fn swap_with_current(
        &mut self,
        oracle: &Oracle,
        token_in: &mut Pubkey,
        token_in_amount: &mut u64,
        event_emitter: &EventEmitter,
    ) -> Result<()> {
        let current = match self {
            Self::From(m) | Self::Into(m) => m,
        };
        let side = current
            .market_meta()
            .to_token_side(token_in)
            .map_err(CoreError::from)?;
        let prices = oracle.market_prices(*current)?;
        let report = current
            .swap(side, (*token_in_amount).into(), prices)
            .map_err(ModelError::from)?
            .execute()
            .map_err(ModelError::from)?;
        *token_in_amount = (*report.token_out_amount())
            .try_into()
            .map_err(|_| error!(CoreError::TokenAmountOverflow))?;
        *token_in = *current
            .market_meta()
            .opposite_token(token_in)
            .map_err(CoreError::from)?;
        msg!("[Swap] swapped in current market");
        event_emitter.emit_cpi(&SwapExecuted::new(self.rev(), self.current(), report, None))?;
        Ok(())
    }
}
