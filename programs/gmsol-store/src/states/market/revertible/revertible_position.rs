use std::cell::RefMut;

use anchor_lang::prelude::*;
use gmsol_model::action::decrease_position::DecreasePositionSwapType;

use crate::{
    constants,
    events::TradeData,
    states::{position::PositionState, HasMarketMeta, Position},
    CoreError,
};

use super::{market::RevertibleMarket, Revertible};

/// Revertible Position.
pub struct RevertiblePosition<'a> {
    market: RevertibleMarket<'a>,
    storage: RefMut<'a, Position>,
    state: PositionState,
    is_collateral_token_long: bool,
    is_long: bool,
}

impl<'a> RevertiblePosition<'a> {
    pub(crate) fn new<'info>(
        market: RevertibleMarket<'a>,
        loader: &'a AccountLoader<'info, Position>,
    ) -> Result<Self> {
        let storage = loader.load_mut()?;
        let meta = market.market_meta();

        require_eq!(
            storage.market_token,
            meta.market_token_mint,
            CoreError::MarketTokenMintMismatched
        );

        let is_long = storage.try_is_long()?;
        let is_collateral_token_long = meta.to_token_side(&storage.collateral_token)?;

        Ok(Self {
            is_long,
            is_collateral_token_long,
            state: storage.state,
            market,
            storage,
        })
    }

    pub(crate) fn collateral_token(&self) -> &Pubkey {
        &self.storage.collateral_token
    }

    pub(crate) fn write_to_event(&self, event: &mut TradeData) -> Result<()> {
        event.update_with_state(&self.state)
    }
}

impl<'a> Revertible for RevertiblePosition<'a> {
    fn commit(mut self) {
        self.market.commit();
        self.storage.state = self.state;
    }
}

impl<'a> gmsol_model::PositionState<{ constants::MARKET_DECIMALS }> for RevertiblePosition<'a> {
    type Num = u128;

    type Signed = i128;

    fn collateral_amount(&self) -> &Self::Num {
        &self.state.collateral_amount
    }

    fn size_in_usd(&self) -> &Self::Num {
        self.state.size_in_usd()
    }

    fn size_in_tokens(&self) -> &Self::Num {
        self.state.size_in_tokens()
    }

    fn borrowing_factor(&self) -> &Self::Num {
        self.state.borrowing_factor()
    }

    fn funding_fee_amount_per_size(&self) -> &Self::Num {
        self.state.funding_fee_amount_per_size()
    }

    fn claimable_funding_fee_amount_per_size(&self, is_long_collateral: bool) -> &Self::Num {
        self.state
            .claimable_funding_fee_amount_per_size(is_long_collateral)
    }
}

impl<'a> gmsol_model::Position<{ constants::MARKET_DECIMALS }> for RevertiblePosition<'a> {
    type Market = RevertibleMarket<'a>;

    fn market(&self) -> &Self::Market {
        &self.market
    }

    fn is_long(&self) -> bool {
        self.is_long
    }

    fn is_collateral_token_long(&self) -> bool {
        self.is_collateral_token_long
    }

    fn are_pnl_and_collateral_tokens_the_same(&self) -> bool {
        self.is_long == self.is_collateral_token_long || self.market.is_pure()
    }

    fn on_validate(&self) -> gmsol_model::Result<()> {
        self.storage.validate_for_market(&self.market.market)
    }
}

impl<'a> gmsol_model::PositionMut<{ constants::MARKET_DECIMALS }> for RevertiblePosition<'a> {
    fn market_mut(&mut self) -> &mut Self::Market {
        &mut self.market
    }

    fn on_increased(&mut self) -> gmsol_model::Result<()> {
        let clock = Clock::get().map_err(Error::from)?;
        self.state.updated_at_slot = clock.slot;
        self.state.increased_at = clock.unix_timestamp;
        self.state.trade_id = self.market.next_trade_id()?;
        Ok(())
    }

    fn on_decreased(&mut self) -> gmsol_model::Result<()> {
        let clock = Clock::get().map_err(Error::from)?;
        self.state.updated_at_slot = clock.slot;
        self.state.decreased_at = clock.unix_timestamp;
        self.state.trade_id = self.market.next_trade_id()?;
        Ok(())
    }

    fn on_swapped(
        &mut self,
        ty: DecreasePositionSwapType,
        report: &gmsol_model::action::swap::SwapReport<Self::Num>,
    ) -> gmsol_model::Result<()> {
        msg!(
            "[Decrease Position Swap] swapped: ty={}, report={:?}",
            ty,
            report
        );
        Ok(())
    }

    fn on_swap_error(
        &mut self,
        ty: DecreasePositionSwapType,
        error: gmsol_model::Error,
    ) -> gmsol_model::Result<()> {
        msg!("[Decrease Position Swap] error: ty={}, err={}", ty, error);
        Ok(())
    }
}

impl<'a> gmsol_model::PositionStateMut<{ constants::MARKET_DECIMALS }> for RevertiblePosition<'a> {
    fn collateral_amount_mut(&mut self) -> &mut Self::Num {
        self.state.collateral_amount_mut()
    }

    fn size_in_usd_mut(&mut self) -> &mut Self::Num {
        self.state.size_in_usd_mut()
    }

    fn size_in_tokens_mut(&mut self) -> &mut Self::Num {
        self.state.size_in_tokens_mut()
    }

    fn borrowing_factor_mut(&mut self) -> &mut Self::Num {
        self.state.borrowing_factor_mut()
    }

    fn funding_fee_amount_per_size_mut(&mut self) -> &mut Self::Num {
        self.state.funding_fee_amount_per_size_mut()
    }

    fn claimable_funding_fee_amount_per_size_mut(
        &mut self,
        is_long_collateral: bool,
    ) -> &mut Self::Num {
        self.state
            .claimable_funding_fee_amount_per_size_mut(is_long_collateral)
    }
}
