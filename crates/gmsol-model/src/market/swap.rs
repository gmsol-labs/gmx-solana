use num_traits::{CheckedSub, One, Signed, Zero};

use crate::{
    action::swap::Swap,
    num::UnsignedAbs,
    params::{FeeParams, PriceImpactParams},
    price::{Price, Prices},
    Balance, BaseMarket, Pool,
};

use super::BaseMarketMut;

/// A market for swapping tokens.
pub trait SwapMarket<const DECIMALS: u8>: BaseMarket<DECIMALS> {
    /// Get swap impact params.
    fn swap_impact_params(&self) -> crate::Result<PriceImpactParams<Self::Num>>;

    /// Get the swap fee params.
    fn swap_fee_params(&self) -> crate::Result<FeeParams<Self::Num>>;
}

/// A mutable market for swapping tokens.
pub trait SwapMarketMut<const DECIMALS: u8>:
    SwapMarket<DECIMALS> + BaseMarketMut<DECIMALS>
{
    /// Get the swap impact pool mutably.
    /// # Requirements
    /// - This method must return `Ok` if [`BaseMarket::swap_impact_pool`] does.
    fn swap_impact_pool_mut(&mut self) -> crate::Result<&mut Self::Pool>;
}

impl<'a, M: SwapMarket<DECIMALS>, const DECIMALS: u8> SwapMarket<DECIMALS> for &'a mut M {
    fn swap_impact_params(&self) -> crate::Result<PriceImpactParams<Self::Num>> {
        (**self).swap_impact_params()
    }

    fn swap_fee_params(&self) -> crate::Result<FeeParams<Self::Num>> {
        (**self).swap_fee_params()
    }
}

impl<'a, M: SwapMarketMut<DECIMALS>, const DECIMALS: u8> SwapMarketMut<DECIMALS> for &'a mut M {
    fn swap_impact_pool_mut(&mut self) -> crate::Result<&mut Self::Pool> {
        (**self).swap_impact_pool_mut()
    }
}

/// Extension trait for [`SwapMarket`].
pub trait SwapMarketExt<const DECIMALS: u8>: SwapMarket<DECIMALS> {
    /// Get the swap impact amount with cap.
    fn swap_impact_amount_with_cap(
        &self,
        is_long_token: bool,
        price: &Price<Self::Num>,
        usd_impact: &Self::Signed,
    ) -> crate::Result<Self::Signed> {
        if price.has_zero() {
            return Err(crate::Error::DividedByZero);
        }
        if usd_impact.is_positive() {
            let mut amount = usd_impact.clone()
                / price
                    .pick_price(true)
                    .clone()
                    .try_into()
                    .map_err(|_| crate::Error::Convert)?;
            let max_amount = if is_long_token {
                self.swap_impact_pool()?.long_amount()?
            } else {
                self.swap_impact_pool()?.short_amount()?
            };
            if amount.unsigned_abs() > max_amount {
                amount = max_amount.try_into().map_err(|_| crate::Error::Convert)?;
            }
            Ok(amount)
        } else if usd_impact.is_negative() {
            let price: Self::Signed = price
                .pick_price(false)
                .clone()
                .try_into()
                .map_err(|_| crate::Error::Convert)?;
            // Round up div.
            let amount = (usd_impact
                .checked_sub(&price)
                .ok_or(crate::Error::Computation(
                    "calculating round up swap impact amount",
                ))?
                + One::one())
                / price;
            Ok(amount)
        } else {
            Ok(Zero::zero())
        }
    }
}

impl<M: SwapMarket<DECIMALS> + ?Sized, const DECIMALS: u8> SwapMarketExt<DECIMALS> for M {}

/// Extension trait for [`SwapMarketMut`].
pub trait SwapMarketMutExt<const DECIMALS: u8>: SwapMarketMut<DECIMALS> {
    /// Create a [`Swap`].
    fn swap(
        &mut self,
        is_token_in_long: bool,
        token_in_amount: Self::Num,
        prices: Prices<Self::Num>,
    ) -> crate::Result<Swap<&mut Self, DECIMALS>>
    where
        Self: Sized,
    {
        Swap::try_new(self, is_token_in_long, token_in_amount, prices)
    }

    /// Apply a swap impact value to the price impact pool.
    ///
    /// - If it is a positive impact amount, cap the impact amount to the amount available in the price impact pool,
    ///   and the price impact pool will be decreased by this amount and return.
    /// - If it is a negative impact amount, the price impact pool will be increased by this amount and return.
    fn apply_swap_impact_value_with_cap(
        &mut self,
        is_long_token: bool,
        price: &Price<Self::Num>,
        usd_impact: &Self::Signed,
    ) -> crate::Result<Self::Num> {
        let delta = self.swap_impact_amount_with_cap(is_long_token, price, usd_impact)?;
        if is_long_token {
            self.swap_impact_pool_mut()?
                .apply_delta_to_long_amount(&-delta.clone())?;
        } else {
            self.swap_impact_pool_mut()?
                .apply_delta_to_short_amount(&-delta.clone())?;
        }
        Ok(delta.unsigned_abs())
    }
}

impl<M: SwapMarketMut<DECIMALS>, const DECIMALS: u8> SwapMarketMutExt<DECIMALS> for M {}
