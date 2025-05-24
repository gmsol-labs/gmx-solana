use num_traits::Signed;

use crate::{num::Unsigned, utils, BalanceExt};

use super::{BaseMarket, BaseMarketExt, PnlFactorKind};

/// Utils for market.
pub(crate) trait MarketUtils<const DECIMALS: u8>: BaseMarket<DECIMALS> {
    fn usage_factor(
        &self,
        is_long: bool,
        reserved_value: &Self::Num,
        pool_value: &Self::Num,
    ) -> crate::Result<Self::Num> {
        let reserve_factor = self.open_interest_reserve_factor()?;
        let max_reserved_value = utils::apply_factor(pool_value, &reserve_factor).ok_or(
            crate::Error::Computation("usage factor: calculating max reserved factor"),
        )?;
        let reserve_usage_factor = utils::div_to_factor(reserved_value, &max_reserved_value, false)
            .ok_or(crate::Error::Computation(
                "usage factor: calculating reserve usage factor",
            ))?;

        if self.ignore_open_interest_for_usage_factor()? {
            return Ok(reserve_usage_factor);
        }

        let max_open_interest = self.max_open_interest(is_long)?;
        let open_interest = self.open_interest()?.amount(is_long)?;
        let open_interest_usage_factor =
            utils::div_to_factor(&open_interest, &max_open_interest, false).ok_or(
                crate::Error::Computation("usage factor: calculating open interest usage factor"),
            )?;

        if reserve_usage_factor > open_interest_usage_factor {
            Ok(reserve_usage_factor)
        } else {
            Ok(open_interest_usage_factor)
        }
    }

    /// Cap pnl with the given pool value and the max pnl factor.
    fn cap_pnl(
        &self,
        is_long: bool,
        pnl: &Self::Signed,
        pool_value: &Self::Num,
        kind: PnlFactorKind,
    ) -> crate::Result<Self::Signed> {
        if pnl.is_positive() {
            let max_pnl_factor = self.pnl_factor_config(kind, is_long)?;
            let max_pnl = crate::utils::apply_factor(pool_value, &max_pnl_factor)
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
}

impl<M: BaseMarket<DECIMALS> + ?Sized, const DECIMALS: u8> MarketUtils<DECIMALS> for M {}
