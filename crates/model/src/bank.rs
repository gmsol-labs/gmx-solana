use std::borrow::Borrow;

use num_traits::{CheckedSub, Zero};

/// A bank of tokens.
pub trait Bank<K> {
    /// Number type.
    type Num;

    /// Record transferred in amount by token.
    fn record_transferred_in_by_token<Q: Borrow<K> + ?Sized>(
        &mut self,
        token: &Q,
        amount: &Self::Num,
    ) -> crate::Result<()>;

    /// Record transferred out amount by token.
    fn record_transferred_out_by_token<Q: Borrow<K> + ?Sized>(
        &mut self,
        token: &Q,
        amount: &Self::Num,
    ) -> crate::Result<()>;

    /// Get the balance of the given token.
    fn balance<Q: Borrow<K> + ?Sized>(&self, token: &Q) -> crate::Result<Self::Num>;

    /// Get the balance of the given token excluding `excluded` amount.
    fn balance_excluding<Q: Borrow<K> + ?Sized>(
        &self,
        token: &Q,
        excluded: &Self::Num,
    ) -> crate::Result<Self::Num>
    where
        Self::Num: CheckedSub + Zero,
    {
        let balance = self.balance(token)?;
        if excluded.is_zero() {
            Ok(balance)
        } else {
            balance
                .checked_sub(excluded)
                .ok_or(crate::Error::Computation(
                    "underflow when excluding amount of balance",
                ))
        }
    }
}
