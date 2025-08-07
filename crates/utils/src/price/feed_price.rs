use anchor_lang::prelude::zero_copy;

use crate::{
    price::{
        decimal::{Decimal, DecimalError},
        Price, PriceFlag, MAX_PRICE_FLAG,
    },
    token_config::TokenConfig,
};

crate::flags!(PriceFlag, MAX_PRICE_FLAG, u8);

const NANOS_PER_SECOND_U32: u32 = 1_000_000_000;

/// Price structure for Price Feed.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct PriceFeedPrice {
    decimals: u8,
    flags: PriceFlagContainer,
    padding: [u8; 2],
    last_update_diff: u32,
    ts: i64,
    price: u128,
    min_price: u128,
    max_price: u128,
}

impl PriceFeedPrice {
    /// Create a new [`PriceFeedPrice`].
    pub fn new(
        decimals: u8,
        ts: i64,
        price: u128,
        min_price: u128,
        max_price: u128,
        last_update_diff: u32,
    ) -> Self {
        Self {
            decimals,
            flags: Default::default(),
            padding: [0; 2],
            last_update_diff,
            ts,
            price,
            min_price,
            max_price,
        }
    }

    /// Set flag.
    pub fn set_flag(&mut self, flag: PriceFlag, value: bool) -> bool {
        self.flags.set_flag(flag, value)
    }

    /// Get ts.
    pub fn ts(&self) -> i64 {
        self.ts
    }

    /// Returns the nanoseconds the last update time is earlier than the report time.
    /// `None` means the field is disabled.
    ///
    /// # Panics
    /// - Panics if `LastUpdateDiffSecs` flag is enabled and converting `last_update_diff` to nanoseconds
    ///   would exceed the `u32` range.
    #[deprecated(since = "0.7.1", note = "use `last_update_diff_secs` instead")]
    pub fn last_update_diff_nanos(&self) -> Option<u32> {
        if !self.flags.get_flag(PriceFlag::LastUpdateDiffEnabled) {
            return None;
        }

        if self.flags.get_flag(PriceFlag::LastUpdateDiffSecs) {
            Some(
                self.last_update_diff
                    .checked_mul(NANOS_PER_SECOND_U32)
                    .expect("out of range for `u32`"),
            )
        } else {
            Some(self.last_update_diff)
        }
    }

    /// Returns the seconds the last update time is earlier than the report time.
    /// `None` means the field is disabled.
    pub fn last_update_diff_secs(&self) -> Option<u32> {
        if !self.flags.get_flag(PriceFlag::LastUpdateDiffEnabled) {
            return None;
        }

        if self.flags.get_flag(PriceFlag::LastUpdateDiffSecs) {
            Some(self.last_update_diff)
        } else {
            Some(self.last_update_diff.div_ceil(NANOS_PER_SECOND_U32))
        }
    }

    /// Get min price.
    pub fn min_price(&self) -> &u128 {
        &self.min_price
    }

    /// Get max price.
    pub fn max_price(&self) -> &u128 {
        &self.max_price
    }

    /// Get price.
    pub fn price(&self) -> &u128 {
        &self.price
    }

    /// Returns whether the market is open.
    pub fn is_market_open(&self, current_timestamp: i64, market_close_timeout: u32) -> bool {
        if !self.flags.get_flag(PriceFlag::Open) {
            return false;
        }

        let Some(last_update_diff_secs) = self.last_update_diff_secs() else {
            return true;
        };

        let last_update_diff_secs = i64::from(last_update_diff_secs);

        // The use of `saturating_sub` here is valid because:
        //   - In the case of overflow, the function returns `false`,
        //     and since `current_timestamp >= ts + i64::MAX`, it must
        //     also hold that `current_timestamp >= ts + market_close_timeout`,
        //     and thus `current_timestamp > last_update + market_close_timeout`.
        //   - In the case of underflow, the function returns `true`,
        //     and since `current_timestamp <= ts + i64::MIN`, it follows that
        //     `current_timestamp <= ts - last_update_diff_secs`, and thus
        //     `current_timestamp - last_update <= market_close_timeout`.
        // Therefore, we only need to check the case where no overflow or underflow occurs.
        let current_diff = current_timestamp.saturating_sub(self.ts);
        let market_close_timeout = market_close_timeout.into();
        if current_diff > market_close_timeout {
            return false;
        }

        last_update_diff_secs
            <= market_close_timeout
                // The use of `saturating_sub` is valid because:
                //   - Underflow is impossible because of the check above, and in the case of
                //     overflow, the function returns `true`, and since
                //     `market_close_timeout >= current_diff + i64::MAX`, it must also hold that
                //     `market_close_timeout >= current_diff + last_update_diff_secs`, and thus
                //     `current_timestamp - last_update <= market_close_timeout`.
                .saturating_sub(current_diff)
    }

    /// Try converting to [`Price`].
    pub fn try_to_price(&self, token_config: &TokenConfig) -> Result<Price, DecimalError> {
        let token_decimals = token_config.token_decimals();
        let precision = token_config.precision();

        let min =
            Decimal::try_from_price(self.min_price, self.decimals, token_decimals, precision)?;

        let max =
            Decimal::try_from_price(self.max_price, self.decimals, token_decimals, precision)?;

        Ok(Price { min, max })
    }

    /// Returns reference price in [`Decimal`].
    pub fn try_to_ref_price(&self, token_config: &TokenConfig) -> Result<Decimal, DecimalError> {
        let token_decimals = token_config.token_decimals();
        let precision = token_config.precision();
        Decimal::try_from_price(self.price, self.decimals, token_decimals, precision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_market_open_in_nanoseconds() {
        let mut price = PriceFeedPrice::new(0, 0, 1, 1, 1, 0);
        assert!(!price.is_market_open(i64::MAX, 0));
        price.set_flag(PriceFlag::Open, true);
        assert!(price.is_market_open(i64::MAX, 0));

        price.set_flag(PriceFlag::LastUpdateDiffEnabled, true);
        price.last_update_diff = u32::MAX;

        price.ts = i64::MIN;
        assert!(!price.is_market_open(i64::MAX, u32::MAX));

        price.ts = i64::MAX;
        assert!(price.is_market_open(i64::MIN, 0));

        let delay = 10i64;
        price.ts = i64::MAX - delay;
        assert!(!price.is_market_open(i64::MAX, u32::MAX / NANOS_PER_SECOND_U32 + delay as u32));
        assert!(price.is_market_open(
            i64::MAX,
            u32::MAX.div_ceil(NANOS_PER_SECOND_U32) + delay as u32
        ));

        let diff = 1i64;
        price.ts = i64::MAX;
        let current = i64::MAX - diff;
        assert!(!price.is_market_open(current, u32::MAX / NANOS_PER_SECOND_U32 - diff as u32));
        assert!(price.is_market_open(
            current,
            u32::MAX.div_ceil(NANOS_PER_SECOND_U32) - diff as u32
        ));
    }

    #[test]
    fn test_is_market_open_in_seconds() {
        let mut price = PriceFeedPrice::new(0, 0, 1, 1, 1, 0);
        assert!(!price.is_market_open(i64::MAX, 0));
        price.set_flag(PriceFlag::Open, true);
        assert!(price.is_market_open(i64::MAX, 0));

        price.set_flag(PriceFlag::LastUpdateDiffEnabled, true);
        price.set_flag(PriceFlag::LastUpdateDiffSecs, true);

        price.last_update_diff = u32::MAX;

        price.ts = i64::MIN;
        assert!(!price.is_market_open(i64::MAX, u32::MAX));

        price.ts = i64::MAX;
        assert!(price.is_market_open(i64::MIN, 0));

        let delay = 10u32;
        let max_diff = u32::MAX - delay;
        price.last_update_diff = max_diff;
        price.ts = i64::MAX - delay as i64;
        assert!(!price.is_market_open(i64::MAX, u32::MAX - 1));
        assert!(price.is_market_open(i64::MAX, u32::MAX));

        let diff = 1u32;
        price.last_update_diff = u32::MAX;
        price.ts = i64::MAX;
        let current = i64::MAX - diff as i64;
        assert!(!price.is_market_open(current, u32::MAX - diff - 1));
        assert!(price.is_market_open(current, u32::MAX - diff));
    }
}
