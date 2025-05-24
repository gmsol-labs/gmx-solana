use crate::num::{Num, Unsigned};
use num_traits::CheckedMul;

use super::PoolDelta;

/// Balanced amounts.
pub trait Balance {
    /// Unsigned number type.
    type Num: Num + Unsigned<Signed = Self::Signed>;

    /// Signed number type.
    type Signed;

    /// Get the long token amount (when this is a token balance), or long usd value (when this is a usd value balance).
    fn long_amount(&self) -> crate::Result<Self::Num>;

    /// Get the short token amount (when this is a token balance), or short usd value (when this is a usd value balance).
    fn short_amount(&self) -> crate::Result<Self::Num>;
}

/// Extension trait for [`Balance`] with utils.
pub trait BalanceExt: Balance {
    /// Get the long amount value in USD.
    fn long_usd_value(&self, price: &Self::Num) -> crate::Result<Self::Num> {
        self.long_amount()?
            .checked_mul(price)
            .ok_or(crate::Error::Overflow)
    }

    /// Get the short amount value in USD.
    fn short_usd_value(&self, price: &Self::Num) -> crate::Result<Self::Num> {
        self.short_amount()?
            .checked_mul(price)
            .ok_or(crate::Error::Overflow)
    }

    /// Get pool value information after applying delta.
    fn pool_delta_with_amounts(
        &self,
        long_token_delta_amount: &Self::Signed,
        short_token_delta_amount: &Self::Signed,
        long_token_price: &Self::Num,
        short_token_price: &Self::Num,
    ) -> crate::Result<PoolDelta<Self::Num>> {
        PoolDelta::try_from_delta_amounts(
            self,
            long_token_delta_amount,
            short_token_delta_amount,
            long_token_price,
            short_token_price,
        )
    }

    /// Get pool value information after applying delta.
    fn pool_delta_with_values(
        &self,
        delta_long_token_usd_value: Self::Signed,
        delta_short_token_usd_value: Self::Signed,
        long_token_price: &Self::Num,
        short_token_price: &Self::Num,
    ) -> crate::Result<PoolDelta<Self::Num>> {
        PoolDelta::try_new(
            self,
            delta_long_token_usd_value,
            delta_short_token_usd_value,
            long_token_price,
            short_token_price,
        )
    }

    /// Merge the amounts with other [`Balance`].
    ///
    /// The result [`Balance`] will consider the total amounts (`long_amount + short_amount`) of `self` as the long amount,
    /// and the total amount of `short` as the short amount.
    fn merge<B: Balance>(&self, short: B) -> Merged<&Self, B> {
        Merged(self, short)
    }

    /// Get amount by side.
    #[inline]
    fn amount(&self, is_long: bool) -> crate::Result<Self::Num> {
        if is_long {
            self.long_amount()
        } else {
            self.short_amount()
        }
    }
}

impl<P: Balance + ?Sized> BalanceExt for P {}

impl<P: Balance> Balance for &P {
    type Num = P::Num;

    type Signed = P::Signed;

    fn long_amount(&self) -> crate::Result<Self::Num> {
        (**self).long_amount()
    }

    fn short_amount(&self) -> crate::Result<Self::Num> {
        (**self).short_amount()
    }
}

/// Merged balanced pool.
/// A [`Balance`] returned by [`BalanceExt::merge`].
#[derive(Debug, Clone, Copy)]
pub struct Merged<A, B>(A, B);

impl<A, B, Num, Signed> Balance for Merged<A, B>
where
    Num: crate::num::Num + Unsigned<Signed = Signed>,
    A: Balance<Num = Num, Signed = Signed>,
    B: Balance<Num = Num, Signed = Signed>,
{
    type Num = Num;

    type Signed = Signed;

    fn long_amount(&self) -> crate::Result<Self::Num> {
        self.0
            .long_amount()?
            .checked_add(&self.0.short_amount()?)
            .ok_or(crate::Error::Overflow)
    }

    fn short_amount(&self) -> crate::Result<Self::Num> {
        self.1
            .long_amount()?
            .checked_add(&self.1.short_amount()?)
            .ok_or(crate::Error::Overflow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{test::TestPool, Pool};

    #[test]
    fn test_merge_balances() -> crate::Result<()> {
        let mut open_interest_for_long = TestPool::<u64>::default();
        let mut open_interest_for_short = TestPool::<u64>::default();

        open_interest_for_long.apply_delta_to_long_amount(&1000)?;
        open_interest_for_long.apply_delta_to_short_amount(&2000)?;
        open_interest_for_short.apply_delta_to_long_amount(&3000)?;
        open_interest_for_short.apply_delta_to_short_amount(&4000)?;

        let open_interest = open_interest_for_long.merge(&open_interest_for_short);

        assert_eq!(open_interest.long_amount()?, 3000);
        assert_eq!(open_interest.short_amount()?, 7000);

        Ok(())
    }
}
