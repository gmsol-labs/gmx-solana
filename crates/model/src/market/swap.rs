use num_traits::{CheckedAdd, CheckedDiv, CheckedMul, CheckedNeg, CheckedSub, One, Signed, Zero};

use crate::{
    action::swap::Swap,
    num::{Unsigned, UnsignedAbs},
    params::{FeeParams, PriceImpactParams},
    pool::delta::{PoolDelta, PriceImpact},
    price::{Price, Prices},
    Balance, BalanceExt, BaseMarket, Pool,
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

impl<M: SwapMarket<DECIMALS>, const DECIMALS: u8> SwapMarket<DECIMALS> for &mut M {
    fn swap_impact_params(&self) -> crate::Result<PriceImpactParams<Self::Num>> {
        (**self).swap_impact_params()
    }

    fn swap_fee_params(&self) -> crate::Result<FeeParams<Self::Num>> {
        (**self).swap_fee_params()
    }
}

impl<M: SwapMarketMut<DECIMALS>, const DECIMALS: u8> SwapMarketMut<DECIMALS> for &mut M {
    fn swap_impact_pool_mut(&mut self) -> crate::Result<&mut Self::Pool> {
        (**self).swap_impact_pool_mut()
    }
}

/// Extension trait for [`SwapMarket`].
pub trait SwapMarketExt<const DECIMALS: u8>: SwapMarket<DECIMALS> {
    /// Calculate swap price impact.
    fn swap_impact_value(
        &self,
        liquidity_pool_delta: &PoolDelta<Self::Num>,
        include_virtual_inventory_impact: bool,
    ) -> crate::Result<PriceImpact<Self::Signed>> {
        let params = self.swap_impact_params()?;

        let impact = liquidity_pool_delta.price_impact(&params)?;

        if !impact.value.is_negative() || !include_virtual_inventory_impact {
            return Ok(impact);
        }

        let Some(virtual_inventory) = self.virtual_inventory_for_swaps_pool()? else {
            return Ok(impact);
        };

        let delta = liquidity_pool_delta.delta();
        let long_token_price = liquidity_pool_delta.long_token_price();
        let short_token_price = liquidity_pool_delta.short_token_price();

        let virtual_inventory_impact = virtual_inventory
            .pool_delta_with_values(
                delta.long_value().clone(),
                delta.short_value().clone(),
                long_token_price,
                short_token_price,
            )?
            .price_impact(&params)?;

        if virtual_inventory_impact.value < impact.value {
            Ok(virtual_inventory_impact)
        } else {
            Ok(impact)
        }
    }

    /// Get the swap impact amount with cap.
    fn swap_impact_amount_with_cap(
        &self,
        is_long_token: bool,
        price: &Price<Self::Num>,
        usd_impact: &Self::Signed,
    ) -> crate::Result<(Self::Signed, Self::Num)> {
        if price.has_zero() {
            return Err(crate::Error::DividedByZero);
        }
        if usd_impact.is_positive() {
            let max_price = price.pick_price(true).to_signed()?;

            let mut amount = usd_impact
                .checked_div(&max_price)
                .ok_or(crate::Error::Computation("calculating swap impact amount"))?;

            let max_amount = if is_long_token {
                self.swap_impact_pool()?.long_amount()?
            } else {
                self.swap_impact_pool()?.short_amount()?
            }
            .to_signed()?;

            let capped_diff_value = if amount > max_amount {
                let capped_diff_value = amount
                    .checked_sub(&max_amount)
                    .map(|diff_amount| diff_amount.unsigned_abs())
                    .and_then(|diff_amount| diff_amount.checked_mul(price.pick_price(true)))
                    .ok_or(crate::Error::Computation("calculating capped diff value"))?;
                amount = max_amount;
                capped_diff_value
            } else {
                Zero::zero()
            };
            Ok((amount, capped_diff_value))
        } else if usd_impact.is_negative() {
            let price = price.pick_price(false).to_signed()?;
            let one = Self::Signed::one();
            // Round up div.
            let amount = usd_impact
                .checked_sub(&price)
                .and_then(|a| a.checked_add(&one)?.checked_div(&price))
                .ok_or(crate::Error::Computation(
                    "calculating round up swap impact amount",
                ))?;
            Ok((amount, Zero::zero()))
        } else {
            Ok((Zero::zero(), Zero::zero()))
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
        let (amount, _) = self.swap_impact_amount_with_cap(is_long_token, price, usd_impact)?;
        let delta = amount
            .checked_neg()
            .ok_or(crate::Error::Computation("negating swap impact delta"))?;
        if is_long_token {
            self.swap_impact_pool_mut()?
                .apply_delta_to_long_amount(&delta)?;
        } else {
            self.swap_impact_pool_mut()?
                .apply_delta_to_short_amount(&delta)?;
        }
        Ok(delta.unsigned_abs())
    }
}

impl<M: SwapMarketMut<DECIMALS>, const DECIMALS: u8> SwapMarketMutExt<DECIMALS> for M {}
