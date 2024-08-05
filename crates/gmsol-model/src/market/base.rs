use crate::{
    action::Prices,
    fixed::FixedPointOps,
    num::{MulDiv, Num, Unsigned, UnsignedAbs},
    pool::{balance::Merged, Balance, BalanceExt, Pool},
    PoolExt,
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

    /// Get the reference of open interest pool.
    fn open_interest_pool(&self, is_long: bool) -> crate::Result<&Self::Pool>;

    /// Get the reference of open interest pool.
    fn open_interest_in_tokens_pool(&self, is_long: bool) -> crate::Result<&Self::Pool>;

    /// Get collateral sum pool.
    fn collateral_sum_pool(&self, is_long: bool) -> crate::Result<&Self::Pool>;

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
}

/// Base Market trait for mutable access.
pub trait BaseMarketMut<const DECIMALS: u8>: BaseMarket<DECIMALS> {
    /// Get the liquidity pool mutably.
    fn liquidity_pool_mut(&mut self) -> crate::Result<&mut Self::Pool>;

    /// Get the mutable reference of the claimable fee pool.
    fn claimable_fee_pool_mut(&mut self) -> crate::Result<&mut Self::Pool>;
}

impl<'a, M: BaseMarket<DECIMALS>, const DECIMALS: u8> BaseMarket<DECIMALS> for &'a mut M {
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
}

impl<'a, M: BaseMarketMut<DECIMALS>, const DECIMALS: u8> BaseMarketMut<DECIMALS> for &'a mut M {
    fn liquidity_pool_mut(&mut self) -> crate::Result<&mut Self::Pool> {
        (**self).liquidity_pool_mut()
    }

    fn claimable_fee_pool_mut(&mut self) -> crate::Result<&mut Self::Pool> {
        (**self).claimable_fee_pool_mut()
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
        _maximize: bool,
    ) -> crate::Result<Self::Num> {
        // TODO: apply maximize by choosing price.
        if is_long {
            self.liquidity_pool()?
                .long_usd_value(&prices.long_token_price)
        } else {
            self.liquidity_pool()?
                .short_usd_value(&prices.short_token_price)
        }
    }

    /// Get the usd value of primary pool.
    fn pool_value(
        &self,
        long_token_price: &Self::Num,
        short_token_price: &Self::Num,
    ) -> crate::Result<Self::Num> {
        // TODO: All pending values should be taken into consideration.
        let long_value = self.liquidity_pool()?.long_usd_value(long_token_price)?;
        let short_value = self.liquidity_pool()?.short_usd_value(short_token_price)?;
        long_value
            .checked_add(&short_value)
            .ok_or(crate::Error::Overflow)
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
        index_token_price: &Self::Num,
        is_long: bool,
        _maximize: bool,
    ) -> crate::Result<Self::Signed> {
        use num_traits::CheckedMul;

        let open_interest = self.open_interest()?.amount(is_long)?;
        let open_interest_in_tokens = self.open_interest_in_tokens()?.amount(is_long)?;
        if open_interest.is_zero() && open_interest_in_tokens.is_zero() {
            return Ok(Zero::zero());
        }

        // TODO: pick price according to the `maximize` flag.
        let price = index_token_price;

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

    /// Cap pnl with max pnl factor.
    fn cap_pnl(
        &self,
        prices: &Prices<Self::Num>,
        is_long: bool,
        pnl: &Self::Signed,
        kind: PnlFactorKind,
    ) -> crate::Result<Self::Signed> {
        if pnl.is_positive() {
            let max_pnl_factor = self.pnl_factor_config(kind, is_long)?;
            let pool_value = self.pool_value_without_pnl_for_one_side(prices, is_long, false)?;
            let max_pnl = crate::utils::apply_factor(&pool_value, &max_pnl_factor)
                .ok_or(crate::Error::Computation("calculating max pnl"))?
                .to_signed()?;
            if *pnl > max_pnl {
                Ok(max_pnl)
            } else {
                Ok(pnl.clone())
            }
        } else {
            Ok(pnl.clone())
        }
    }

    /// Get pnl factor.
    fn pnl_factor(
        &self,
        prices: &Prices<Self::Num>,
        is_long: bool,
        maximize: bool,
    ) -> crate::Result<Self::Signed> {
        let pool_value = self.pool_value_without_pnl_for_one_side(prices, is_long, !maximize)?;
        let pnl = self.pnl(&prices.index_token_price, is_long, maximize)?;
        crate::utils::div_to_factor_signed(&pnl, &pool_value)
            .ok_or(crate::Error::Computation("calculating pnl factor"))
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
    ) -> crate::Result<Option<(Self::Signed, Self::Num)>> {
        let pnl_factor = self.pnl_factor(prices, is_long, true)?;
        let max_pnl_factor = self.pnl_factor_config(kind, is_long)?;

        let is_exceeded = pnl_factor.is_positive() && pnl_factor.unsigned_abs() > max_pnl_factor;

        Ok(is_exceeded.then_some((pnl_factor, max_pnl_factor)))
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
        index_token_price: &Self::Num,
        is_long: bool,
    ) -> crate::Result<Self::Num> {
        // TODO: add comment to explain the difference.
        if is_long {
            // TODO: use max price.
            self.open_interest_in_tokens()?
                .long_usd_value(index_token_price)
        } else {
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
            Err(crate::Error::InsufficientReserve)
        } else {
            Ok(())
        }
    }

    /// Expected min token balance excluding collateral amount.
    fn expected_min_token_balance_excluding_collateral_amount(
        &self,
        is_long_token: bool,
    ) -> crate::Result<Self::Num> {
        // Primary Pool Amount
        let mut balance = self.liquidity_pool()?.amount(is_long_token)?;

        // Swap Impact Pool Amount
        balance = balance
            .checked_add(&self.swap_impact_pool()?.amount(is_long_token)?)
            .ok_or(crate::Error::Computation(
                "overflow adding swap impact pool amount",
            ))?;

        // Claimable Fee Pool Amount
        balance = balance
            .checked_add(&self.claimable_fee_pool()?.amount(is_long_token)?)
            .ok_or(crate::Error::Computation(
                "overflow adding claimable fee amount",
            ))?;

        // TODO: Claimable UI Fee Amount.
        // TODO: Affiliate Reward Amount.
        Ok(balance)
    }

    /// Validate token balance of the market.
    fn validate_token_balance_for_one_side(
        &self,
        balance: &Self::Num,
        is_long_token: bool,
    ) -> crate::Result<()> {
        let expected =
            self.expected_min_token_balance_excluding_collateral_amount(is_long_token)?;
        if *balance < expected {
            return Err(crate::Error::InvalidTokenBalance(
                "Less than expected min token balance excluding collateral amount",
                expected.to_string(),
                balance.to_string(),
            ));
        }

        let mut collateral_amount = self.collateral_sum_pool(true)?.amount(is_long_token)?;
        collateral_amount = collateral_amount
            .checked_add(&self.collateral_sum_pool(false)?.amount(is_long_token)?)
            .ok_or(crate::Error::Computation(
                "calculating total collateral sum for one side",
            ))?;
        if *balance < collateral_amount {
            return Err(crate::Error::InvalidTokenBalance(
                "Less than total collateral amount",
                collateral_amount.to_string(),
                balance.to_string(),
            ));
        }

        // We don't have to validate the claimable funding amount since they are claimed immediately.

        Ok(())
    }
}

impl<M: BaseMarket<DECIMALS> + ?Sized, const DECIMALS: u8> BaseMarketExt<DECIMALS> for M {}

/// Extension trait for [`BaseMarket`].
pub trait BaseMarketMutExt<const DECIMALS: u8>: BaseMarketMut<DECIMALS> {
    /// Apply delta to the primary pool.
    fn apply_delta(&mut self, is_long_token: bool, delta: &Self::Signed) -> crate::Result<()> {
        if is_long_token {
            self.liquidity_pool_mut()?
                .apply_delta_to_long_amount(delta)?;
        } else {
            self.liquidity_pool_mut()?
                .apply_delta_to_short_amount(delta)?;
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
#[derive(Debug, Clone, Copy)]
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
    /// Min factor after auto-deleveraing.
    MinAfterAdl,
}
