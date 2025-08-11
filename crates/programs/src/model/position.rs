use std::sync::Arc;

use gmsol_model::action::{decrease_position::DecreasePositionSwapType, swap::SwapReport};

use crate::{
    constants,
    gmsol_store::{accounts::Position, types::PositionState},
};

use super::MarketModel;

/// Position Model.
#[derive(Debug, Clone)]
pub struct PositionModel {
    market: MarketModel,
    position: Arc<Position>,
    is_long: bool,
    is_collateral_token_long: bool,
    swap_history: Vec<Arc<SwapReport<u128, i128>>>,
}

#[repr(u8)]
pub(super) enum PositionKind {
    #[allow(dead_code)]
    Uninitialized,
    Long,
    Short,
}

impl Position {
    /// Return whether the side of the position is long.
    pub fn try_is_long(&self) -> gmsol_model::Result<bool> {
        if self.kind == PositionKind::Long as u8 {
            Ok(true)
        } else if self.kind == PositionKind::Short as u8 {
            Ok(false)
        } else {
            Err(gmsol_model::Error::InvalidPosition(
                "uninitialized position",
            ))
        }
    }
}

impl PositionModel {
    /// Create from [`MarketModel`] and [`Position`].
    pub fn new(market: MarketModel, position: Arc<Position>) -> gmsol_model::Result<Self> {
        let is_long = position.try_is_long()?;
        let is_collateral_token_long = market.meta.token_side(&position.collateral_token)?;
        Ok(Self {
            market,
            position,
            is_long,
            is_collateral_token_long,
            swap_history: vec![],
        })
    }

    fn make_position_mut(&mut self) -> &mut Position {
        Arc::make_mut(&mut self.position)
    }

    /// Get position.
    pub fn position(&self) -> &Position {
        &self.position
    }

    /// Returns a reference to the position wrapped in an `Arc`.
    pub fn position_arc(&self) -> &Arc<Position> {
        &self.position
    }

    /// Returns the swap history.
    pub fn swap_history(&self) -> &[Arc<SwapReport<u128, i128>>] {
        &self.swap_history
    }

    /// Clear the swap history.
    pub fn clear(&mut self) {
        self.swap_history.clear();
    }
}

impl gmsol_model::PositionState<{ constants::MARKET_DECIMALS }> for PositionState {
    type Num = u128;

    type Signed = i128;

    fn collateral_amount(&self) -> &Self::Num {
        &self.collateral_amount
    }

    fn size_in_usd(&self) -> &Self::Num {
        &self.size_in_usd
    }

    fn size_in_tokens(&self) -> &Self::Num {
        &self.size_in_tokens
    }

    fn borrowing_factor(&self) -> &Self::Num {
        &self.borrowing_factor
    }

    fn funding_fee_amount_per_size(&self) -> &Self::Num {
        &self.funding_fee_amount_per_size
    }

    fn claimable_funding_fee_amount_per_size(&self, is_long_collateral: bool) -> &Self::Num {
        if is_long_collateral {
            &self.long_token_claimable_funding_amount_per_size
        } else {
            &self.short_token_claimable_funding_amount_per_size
        }
    }
}

impl gmsol_model::PositionState<{ constants::MARKET_DECIMALS }> for PositionModel {
    type Num = u128;

    type Signed = i128;

    fn collateral_amount(&self) -> &Self::Num {
        self.position.state.collateral_amount()
    }

    fn size_in_usd(&self) -> &Self::Num {
        self.position.state.size_in_usd()
    }

    fn size_in_tokens(&self) -> &Self::Num {
        self.position.state.size_in_tokens()
    }

    fn borrowing_factor(&self) -> &Self::Num {
        self.position.state.borrowing_factor()
    }

    fn funding_fee_amount_per_size(&self) -> &Self::Num {
        self.position.state.funding_fee_amount_per_size()
    }

    fn claimable_funding_fee_amount_per_size(&self, is_long_collateral: bool) -> &Self::Num {
        self.position
            .state
            .claimable_funding_fee_amount_per_size(is_long_collateral)
    }
}

impl gmsol_model::PositionStateMut<{ constants::MARKET_DECIMALS }> for PositionModel {
    fn collateral_amount_mut(&mut self) -> &mut Self::Num {
        &mut self.make_position_mut().state.collateral_amount
    }

    fn size_in_usd_mut(&mut self) -> &mut Self::Num {
        &mut self.make_position_mut().state.size_in_usd
    }

    fn size_in_tokens_mut(&mut self) -> &mut Self::Num {
        &mut self.make_position_mut().state.size_in_tokens
    }

    fn borrowing_factor_mut(&mut self) -> &mut Self::Num {
        &mut self.make_position_mut().state.borrowing_factor
    }

    fn funding_fee_amount_per_size_mut(&mut self) -> &mut Self::Num {
        &mut self.make_position_mut().state.funding_fee_amount_per_size
    }

    fn claimable_funding_fee_amount_per_size_mut(
        &mut self,
        is_long_collateral: bool,
    ) -> &mut Self::Num {
        if is_long_collateral {
            &mut self
                .make_position_mut()
                .state
                .long_token_claimable_funding_amount_per_size
        } else {
            &mut self
                .make_position_mut()
                .state
                .short_token_claimable_funding_amount_per_size
        }
    }
}

impl gmsol_model::Position<{ constants::MARKET_DECIMALS }> for PositionModel {
    type Market = MarketModel;

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
        Ok(())
    }
}

impl gmsol_model::PositionMut<{ constants::MARKET_DECIMALS }> for PositionModel {
    fn market_mut(&mut self) -> &mut Self::Market {
        &mut self.market
    }

    fn on_increased(&mut self) -> gmsol_model::Result<()> {
        Ok(())
    }

    fn on_decreased(&mut self) -> gmsol_model::Result<()> {
        Ok(())
    }

    fn on_swapped(
        &mut self,
        _ty: DecreasePositionSwapType,
        report: &SwapReport<Self::Num, <Self::Num as gmsol_model::num::Unsigned>::Signed>,
    ) -> gmsol_model::Result<()> {
        self.swap_history.push(Arc::new(report.clone()));
        Ok(())
    }

    fn on_swap_error(
        &mut self,
        _ty: DecreasePositionSwapType,
        _error: gmsol_model::Error,
    ) -> gmsol_model::Result<()> {
        Ok(())
    }
}
