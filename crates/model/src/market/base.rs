use std::ops::{Deref, DerefMut};

use crate::{
    fixed::FixedPointOps,
    num::{MulDiv, Num, Unsigned, UnsignedAbs},
    pool::{balance::Merged, Balance, BalanceExt, Pool},
    price::{Price, Prices},
    Delta, PoolExt,
};
use num_traits::{CheckedAdd, CheckedSub, Signed, Zero};

use super::get_msg_by_side;

/// Base Market trait.
pub trait BaseMarket<const DECIMALS: u8> {
    /// Unsigned number type used in the market.
    type Num: MulDiv<Signed = Self::Signed> + FixedPointOps<DECIMALS>;

    /// Signed number type used in the market.
    type Signed: UnsignedAbs<Unsigned = Self::Num> + TryFrom<Self::Num> + Num;

    /// Pool type.
    type Pool: Pool<Num = Self::Num, Signed = Self::Signed>;

    /// Get the liquidity pool.
    fn liquidity_pool(&self) -> crate::Result<&Self::Pool>;

    /// Get the claimable fee pool.
    fn claimable_fee_pool(&self) -> crate::Result<&Self::Pool>;

    /// Get the swap impact pool.
    fn swap_impact_pool(&self) -> crate::Result<&Self::Pool>;

    /// Get the open interest pool.
    fn open_interest_pool(&self, is_long: bool) -> crate::Result<&Self::Pool>;

    /// Get the open interest in (index) tokens pool.
    fn open_interest_in_tokens_pool(&self, is_long: bool) -> crate::Result<&Self::Pool>;

    /// Get collateral sum pool.
    fn collateral_sum_pool(&self, is_long: bool) -> crate::Result<&Self::Pool>;

    /// Get virtual inventory for swaps.
    fn virtual_inventory_for_swaps_pool(
        &self,
    ) -> crate::Result<Option<impl Deref<Target = Self::Pool>>>;

    /// Get virtual inventory for positions.
    fn virtual_inventory_for_positions_pool(
        &self,
    ) -> crate::Result<Option<impl Deref<Target = Self::Pool>>>;

    /// USD value to market token amount divisor.
    ///
    /// One should make sure it is non-zero.
    fn usd_to_amount_divisor(&self) -> Self::Num;

    /// Get max pool amount.
    fn max_pool_amount(&self, is_long_token: bool) -> crate::Result<Self::Num>;

    /// Get pnl factor config.
    fn pnl_factor_config(&self, kind: PnlFactorKind, is_long: bool) -> crate::Result<Self::Num>;

    /// Get reserve factor.
    fn reserve_factor(&self) -> crate::Result<Self::Num>;

    /// Get open interest reserve factor.
    fn open_interest_reserve_factor(&self) -> crate::Result<Self::Num>;

    /// Get max open interest.
    fn max_open_interest(&self, is_long: bool) -> crate::Result<Self::Num>;

    /// Returns whether ignore open interest for usage factor.
    fn ignore_open_interest_for_usage_factor(&self) -> crate::Result<bool>;
}

/// Base Market trait for mutable access.
pub trait BaseMarketMut<const DECIMALS: u8>: BaseMarket<DECIMALS> {
    /// Get the liquidity pool mutably.
    /// # Requirements
    /// - This method must return `Ok` if [`BaseMarket::liquidity_pool`] does.
    /// # Notes
    /// - Avoid using this function directly unless necessary.
    ///   Use [`BaseMarketMut::apply_delta`] instead.
    fn liquidity_pool_mut(&mut self) -> crate::Result<&mut Self::Pool>;

    /// Get the mutable reference of the claimable fee pool.
    /// # Requirements
    /// - This method must return `Ok` if [`BaseMarket::claimable_fee_pool`] does.
    fn claimable_fee_pool_mut(&mut self) -> crate::Result<&mut Self::Pool>;

    /// Get virtual inventory for swaps mutably.
    /// # Requirements
    /// - This method must return `Ok(Some(_))` if [`BaseMarket::virtual_inventory_for_swaps_pool`] does.
    fn virtual_inventory_for_swaps_pool_mut(
        &mut self,
    ) -> crate::Result<Option<impl DerefMut<Target = Self::Pool>>>;
}

impl<M: BaseMarket<DECIMALS>, const DECIMALS: u8> BaseMarket<DECIMALS> for &mut M {
    type Num = M::Num;

    type Signed = M::Signed;

    type Pool = M::Pool;

    fn liquidity_pool(&self) -> crate::Result<&Self::Pool> {
        (**self).liquidity_pool()
    }

    fn swap_impact_pool(&self) -> crate::Result<&Self::Pool> {
        (**self).swap_impact_pool()
    }

    fn claimable_fee_pool(&self) -> crate::Result<&Self::Pool> {
        (**self).claimable_fee_pool()
    }

    fn open_interest_pool(&self, is_long: bool) -> crate::Result<&Self::Pool> {
        (**self).open_interest_pool(is_long)
    }

    fn open_interest_in_tokens_pool(&self, is_long: bool) -> crate::Result<&Self::Pool> {
        (**self).open_interest_in_tokens_pool(is_long)
    }

    fn collateral_sum_pool(&self, is_long: bool) -> crate::Result<&Self::Pool> {
        (**self).collateral_sum_pool(is_long)
    }

    fn virtual_inventory_for_swaps_pool(
        &self,
    ) -> crate::Result<Option<impl Deref<Target = Self::Pool>>> {
        (**self).virtual_inventory_for_swaps_pool()
    }

    fn virtual_inventory_for_positions_pool(
        &self,
    ) -> crate::Result<Option<impl Deref<Target = Self::Pool>>> {
        (**self).virtual_inventory_for_positions_pool()
    }

    fn usd_to_amount_divisor(&self) -> Self::Num {
        (**self).usd_to_amount_divisor()
    }

    fn max_pool_amount(&self, is_long_token: bool) -> crate::Result<Self::Num> {
        (**self).max_pool_amount(is_long_token)
    }

    fn pnl_factor_config(&self, kind: PnlFactorKind, is_long: bool) -> crate::Result<Self::Num> {
        (**self).pnl_factor_config(kind, is_long)
    }

    fn reserve_factor(&self) -> crate::Result<Self::Num> {
        (**self).reserve_factor()
    }

    fn open_interest_reserve_factor(&self) -> crate::Result<Self::Num> {
        (**self).open_interest_reserve_factor()
    }

    fn max_open_interest(&self, is_long: bool) -> crate::Result<Self::Num> {
        (**self).max_open_interest(is_long)
    }

    fn ignore_open_interest_for_usage_factor(&self) -> crate::Result<bool> {
        (**self).ignore_open_interest_for_usage_factor()
    }
}

impl<M: BaseMarketMut<DECIMALS>, const DECIMALS: u8> BaseMarketMut<DECIMALS> for &mut M {
    fn liquidity_pool_mut(&mut self) -> crate::Result<&mut Self::Pool> {
        (**self).liquidity_pool_mut()
    }

    fn claimable_fee_pool_mut(&mut self) -> crate::Result<&mut Self::Pool> {
        (**self).claimable_fee_pool_mut()
    }

    fn virtual_inventory_for_swaps_pool_mut(
        &mut self,
    ) -> crate::Result<Option<impl DerefMut<Target = Self::Pool>>> {
        (**self).virtual_inventory_for_swaps_pool_mut()
    }
}

/// Extension trait for [`BaseMarket`].
pub trait BaseMarketExt<const DECIMALS: u8>: BaseMarket<DECIMALS> {
    /// Get the usd value of primary pool without pnl for one side.
    #[inline]
    fn pool_value_without_pnl_for_one_side(
        &self,
        prices: &Prices<Self::Num>,
        is_long: bool,
        maximize: bool,
    ) -> crate::Result<Self::Num> {
        if is_long {
            self.liquidity_pool()?
                .long_usd_value(prices.long_token_price.pick_price(maximize))
        } else {
            self.liquidity_pool()?
                .short_usd_value(prices.short_token_price.pick_price(maximize))
        }
    }

    /// Get total open interest as a [`Balance`].
    fn open_interest(&self) -> crate::Result<Merged<&Self::Pool, &Self::Pool>> {
        Ok(self
            .open_interest_pool(true)?
            .merge(self.open_interest_pool(false)?))
    }

    /// Get total open interest in tokens as a merged [`Balance`].
    ///
    /// The long amount is the total long open interest in tokens,
    /// while the short amount is the total short open interest in tokens.
    fn open_interest_in_tokens(&self) -> crate::Result<Merged<&Self::Pool, &Self::Pool>> {
        Ok(self
            .open_interest_in_tokens_pool(true)?
            .merge(self.open_interest_in_tokens_pool(false)?))
    }

    /// Get total pnl of the market for one side.
    fn pnl(
        &self,
        index_token_price: &Price<Self::Num>,
        is_long: bool,
        maximize: bool,
    ) -> crate::Result<Self::Signed> {
        use num_traits::CheckedMul;

        let open_interest = self.open_interest()?.amount(is_long)?;
        let open_interest_in_tokens = self.open_interest_in_tokens()?.amount(is_long)?;
        if open_interest.is_zero() && open_interest_in_tokens.is_zero() {
            return Ok(Zero::zero());
        }

        let price = index_token_price.pick_price_for_pnl(is_long, maximize);

        let open_interest_value = open_interest_in_tokens
            .checked_mul(price)
            .ok_or(crate::Error::Computation("calculating open interest value"))?;

        if is_long {
            open_interest_value
                .to_signed()?
                .checked_sub(&open_interest.to_signed()?)
                .ok_or(crate::Error::Computation("calculating pnl for long"))
        } else {
            open_interest
                .to_signed()?
                .checked_sub(&open_interest_value.to_signed()?)
                .ok_or(crate::Error::Computation("calculating pnl for short"))
        }
    }

    /// Get pnl factor with pool value.
    fn pnl_factor_with_pool_value(
        &self,
        prices: &Prices<Self::Num>,
        is_long: bool,
        maximize: bool,
    ) -> crate::Result<(Self::Signed, Self::Num)> {
        let pool_value = self.pool_value_without_pnl_for_one_side(prices, is_long, !maximize)?;
        let pnl = self.pnl(&prices.index_token_price, is_long, maximize)?;
        crate::utils::div_to_factor_signed(&pnl, &pool_value)
            .ok_or(crate::Error::Computation("calculating pnl factor"))
            .map(|factor| (factor, pool_value))
    }

    /// Get pnl factor.
    fn pnl_factor(
        &self,
        prices: &Prices<Self::Num>,
        is_long: bool,
        maximize: bool,
    ) -> crate::Result<Self::Signed> {
        Ok(self
            .pnl_factor_with_pool_value(prices, is_long, maximize)?
            .0)
    }

    /// Validate (primary) pool amount.
    fn validate_pool_amount(&self, is_long_token: bool) -> crate::Result<()> {
        let amount = self.liquidity_pool()?.amount(is_long_token)?;
        let max_pool_amount = self.max_pool_amount(is_long_token)?;
        if amount > max_pool_amount {
            Err(crate::Error::MaxPoolAmountExceeded(get_msg_by_side(
                is_long_token,
            )))
        } else {
            Ok(())
        }
    }

    /// Get the excess of pending pnl.
    ///
    /// Return `Some` if the pnl factor is exceeded the given kind of pnl factor.
    fn pnl_factor_exceeded(
        &self,
        prices: &Prices<Self::Num>,
        kind: PnlFactorKind,
        is_long: bool,
    ) -> crate::Result<Option<PnlFactorExceeded<Self::Num>>> {
        let (pnl_factor, pool_value) = self.pnl_factor_with_pool_value(prices, is_long, true)?;
        let max_pnl_factor = self.pnl_factor_config(kind, is_long)?;

        let is_exceeded = pnl_factor.is_positive() && pnl_factor.unsigned_abs() > max_pnl_factor;

        Ok(is_exceeded.then(|| PnlFactorExceeded {
            pnl_factor,
            max_pnl_factor,
            pool_value,
        }))
    }

    /// Validate pnl factor.
    fn validate_pnl_factor(
        &self,
        prices: &Prices<Self::Num>,
        kind: PnlFactorKind,
        is_long: bool,
    ) -> crate::Result<()> {
        if self.pnl_factor_exceeded(prices, kind, is_long)?.is_some() {
            Err(crate::Error::PnlFactorExceeded(
                kind,
                get_msg_by_side(is_long),
            ))
        } else {
            Ok(())
        }
    }

    /// Validate max pnl.
    fn validate_max_pnl(
        &self,
        prices: &Prices<Self::Num>,
        long_kind: PnlFactorKind,
        short_kind: PnlFactorKind,
    ) -> crate::Result<()> {
        self.validate_pnl_factor(prices, long_kind, true)?;
        self.validate_pnl_factor(prices, short_kind, false)?;
        Ok(())
    }

    /// Get reserved value.
    fn reserved_value(
        &self,
        index_token_price: &Price<Self::Num>,
        is_long: bool,
    ) -> crate::Result<Self::Num> {
        if is_long {
            // For longs calculate the reserved USD based on the open interest and current index_token_price.
            // This works well for e.g. an ETH / USD market with long collateral token as WETH
            // the available amount to be reserved would scale with the price of ETH.
            // This also works for e.g. a SOL / USD market with long collateral token as WETH
            // if the price of SOL increases more than the price of ETH, additional amounts would be
            // automatically reserved.
            self.open_interest_in_tokens()?
                .long_usd_value(index_token_price.pick_price(true))
        } else {
            // For shorts use the open interest as the reserved USD value.
            // This works well for e.g. an ETH / USD market with short collateral token as USDC
            // the available amount to be reserved would not change with the price of ETH.
            self.open_interest()?.short_amount()
        }
    }

    /// Validate reserve.
    fn validate_reserve(&self, prices: &Prices<Self::Num>, is_long: bool) -> crate::Result<()> {
        let pool_value = self.pool_value_without_pnl_for_one_side(prices, is_long, false)?;

        let max_reserved_value =
            crate::utils::apply_factor(&pool_value, &self.reserve_factor()?)
                .ok_or(crate::Error::Computation("calculating max reserved value"))?;

        let reserved_value = self.reserved_value(&prices.index_token_price, is_long)?;

        if reserved_value > max_reserved_value {
            Err(crate::Error::InsufficientReserve(
                reserved_value.to_string(),
                max_reserved_value.to_string(),
            ))
        } else {
            Ok(())
        }
    }

    /// Expected min token balance excluding collateral amount.
    ///
    /// # Notes
    /// Note that **"one token side"** here means calculating based on half of the side.
    /// For markets where the long token and short token are different, there is no ambiguity.
    /// However, if the long token and short token are the same, choosing the long token side
    /// will result in a value that is not actually the total amount of the long token,
    /// but rather the total amount belonging to the long token side (often only half of it).
    ///
    /// For example, if both the long token and the short token are WSOL, and the liquidity
    /// pool has a total of 1000 WSOL. Then, in a typical pool implementation, the long token
    /// side of the liquidity pool has only 500 **WSOL**, while the short token side also has 500 WSOL.
    /// In this case, this function will only consider one side, taking into account only 500 WSOL
    /// in the calculation.
    fn expected_min_token_balance_excluding_collateral_amount_for_one_token_side(
        &self,
        is_long_side: bool,
    ) -> crate::Result<Self::Num> {
        // Liquidity Pool Amount
        let mut balance = self.liquidity_pool()?.amount(is_long_side)?;

        // Swap Impact Pool Amount
        balance = balance
            .checked_add(&self.swap_impact_pool()?.amount(is_long_side)?)
            .ok_or(crate::Error::Computation(
                "overflow adding swap impact pool amount",
            ))?;

        // Claimable Fee Pool Amount
        balance = balance
            .checked_add(&self.claimable_fee_pool()?.amount(is_long_side)?)
            .ok_or(crate::Error::Computation(
                "overflow adding claimable fee amount",
            ))?;

        Ok(balance)
    }

    /// Get total collateral amount for one token side.
    ///
    /// # Notes
    /// Note that **"one token side"** here means calculating based on half of the side.
    /// (See also [`expected_min_token_balance_excluding_collateral_amount_for_one_token_side`](BaseMarketExt::expected_min_token_balance_excluding_collateral_amount_for_one_token_side)).
    fn total_collateral_amount_for_one_token_side(
        &self,
        is_long_side: bool,
    ) -> crate::Result<Self::Num> {
        let mut collateral_amount = self.collateral_sum_pool(true)?.amount(is_long_side)?;
        collateral_amount = collateral_amount
            .checked_add(&self.collateral_sum_pool(false)?.amount(is_long_side)?)
            .ok_or(crate::Error::Computation(
                "calculating total collateral sum for one side",
            ))?;
        Ok(collateral_amount)
    }

    /// Returns the liquidity pool and virtual inventory for swaps pool after applying the delta.
    fn checked_apply_delta(
        &self,
        delta: Delta<&Self::Signed>,
    ) -> crate::Result<(Self::Pool, Option<Self::Pool>)> {
        let liquidity_pool = self.liquidity_pool()?.checked_apply_delta(delta)?;
        let virtual_inventory_for_swaps_pool = self
            .virtual_inventory_for_swaps_pool()?
            .map(|p| p.checked_apply_delta(delta))
            .transpose()?;

        Ok((liquidity_pool, virtual_inventory_for_swaps_pool))
    }
}

impl<M: BaseMarket<DECIMALS> + ?Sized, const DECIMALS: u8> BaseMarketExt<DECIMALS> for M {}

/// Extension trait for [`BaseMarketMut`].
pub trait BaseMarketMutExt<const DECIMALS: u8>: BaseMarketMut<DECIMALS> {
    /// Apply delta to the primary pool.
    fn apply_delta(&mut self, is_long_token: bool, delta: &Self::Signed) -> crate::Result<()> {
        let delta = if is_long_token {
            Delta::new_with_long(delta)
        } else {
            Delta::new_with_short(delta)
        };
        let (liquidity_pool, virtual_inventory_for_swaps_pool) = self.checked_apply_delta(delta)?;

        *self
            .liquidity_pool_mut()
            .expect("liquidity pool must be valid") = liquidity_pool;
        if let Some(virtual_inventory_for_swaps_pool) = virtual_inventory_for_swaps_pool {
            *self
                .virtual_inventory_for_swaps_pool_mut()
                .expect("virtual inventory for_swaps pool must be valid")
                .expect("virtual inventory for_swaps pool must exist") =
                virtual_inventory_for_swaps_pool;
        }

        Ok(())
    }

    /// Apply delta to claimable fee pool.
    fn apply_delta_to_claimable_fee_pool(
        &mut self,
        is_long_token: bool,
        delta: &Self::Signed,
    ) -> crate::Result<()> {
        self.claimable_fee_pool_mut()?
            .apply_delta_amount(is_long_token, delta)?;
        Ok(())
    }
}

impl<M: BaseMarketMut<DECIMALS> + ?Sized, const DECIMALS: u8> BaseMarketMutExt<DECIMALS> for M {}

/// Pnl Factor Kind.
#[derive(
    Debug,
    Clone,
    Copy,
    num_enum::TryFromPrimitive,
    num_enum::IntoPrimitive,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
)]
#[cfg_attr(
    feature = "strum",
    derive(strum::EnumIter, strum::EnumString, strum::Display)
)]
#[cfg_attr(feature = "strum", strum(serialize_all = "snake_case"))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "js", derive(tsify_next::Tsify))]
#[repr(u8)]
#[non_exhaustive]
pub enum PnlFactorKind {
    /// For deposit.
    MaxAfterDeposit,
    /// For withdrawal.
    MaxAfterWithdrawal,
    /// For trader.
    MaxForTrader,
    /// For auto-deleveraging.
    ForAdl,
    /// Min factor after auto-deleveraging.
    MinAfterAdl,
}

/// PnL factor exceeded.
pub struct PnlFactorExceeded<T: Unsigned> {
    /// Current PnL factor.
    pub pnl_factor: T::Signed,
    /// Max PnL factor.
    pub max_pnl_factor: T,
    /// Current pool value.
    pub pool_value: T,
}

impl<T: Unsigned> PnlFactorExceeded<T> {
    /// Get the exceeded pnl.
    pub fn exceeded_pnl<const DECIMALS: u8>(&self) -> Option<T>
    where
        T: CheckedSub,
        T: FixedPointOps<DECIMALS>,
    {
        if !self.pnl_factor.is_positive() || self.pool_value.is_zero() {
            return None;
        }

        let pnl_factor = self.pnl_factor.unsigned_abs();

        let diff_factor = pnl_factor.checked_sub(&self.max_pnl_factor)?;

        crate::utils::apply_factor(&self.pool_value, &diff_factor)
    }
}
