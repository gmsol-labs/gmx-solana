use num_traits::{CheckedAdd, CheckedDiv};

/// Price.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "anchor-lang",
    derive(anchor_lang::AnchorDeserialize, anchor_lang::AnchorSerialize)
)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Price<T> {
    /// Minimum Price.
    pub min: T,
    /// Maximum Price.
    pub max: T,
}

impl<T: Clone> Price<T> {
    /// Set prices for test.
    #[cfg(test)]
    pub fn set_price_for_test(&mut self, price: T) {
        self.min = price.clone();
        self.max = price;
    }
}

impl<T> Price<T>
where
    T: Ord,
{
    /// Pick price for PnL.
    pub fn pick_price_for_pnl(&self, is_long: bool, maximize: bool) -> &T {
        if is_long ^ maximize {
            &self.min
        } else {
            &self.max
        }
    }

    /// Pick price.
    pub fn pick_price(&self, maximize: bool) -> &T {
        if maximize {
            &self.max
        } else {
            &self.min
        }
    }
}

impl<T: num_traits::Zero> Price<T> {
    /// Return whether the min price or max price is zero.
    pub fn has_zero(&self) -> bool {
        self.min.is_zero() || self.max.is_zero()
    }
}

impl<T> Price<T>
where
    T: CheckedAdd + CheckedDiv + num_traits::One,
{
    /// Get mid price checked.
    pub fn checked_mid(&self) -> Option<T> {
        self.min
            .checked_add(&self.max)
            .and_then(|p| p.checked_div(&(T::one() + T::one())))
    }

    /// Get mid price.
    ///
    /// # Panic
    /// Panic if cannot calculate the mid price.
    pub fn mid(&self) -> T {
        self.checked_mid().expect("cannot calculate the mid price")
    }
}

impl<T> Price<T>
where
    T: num_traits::Zero + CheckedAdd + CheckedDiv + num_traits::One,
{
    fn is_valid(&self) -> bool {
        !self.min.is_zero() && !self.max.is_zero() && self.checked_mid().is_some()
    }
}

/// Prices for execution.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "anchor-lang",
    derive(anchor_lang::AnchorDeserialize, anchor_lang::AnchorSerialize)
)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Prices<T> {
    /// Index token price.
    pub index_token_price: Price<T>,
    /// Long token price.
    pub long_token_price: Price<T>,
    /// Short token price.
    pub short_token_price: Price<T>,
}

impl<T> Prices<T> {
    /// Create a new [`Prices`].
    #[cfg(test)]
    pub fn new_for_test(index: T, long: T, short: T) -> Self
    where
        T: Clone,
    {
        Self {
            index_token_price: Price {
                min: index.clone(),
                max: index,
            },
            long_token_price: Price {
                min: long.clone(),
                max: long,
            },
            short_token_price: Price {
                min: short.clone(),
                max: short,
            },
        }
    }

    /// Get collateral token price.
    pub fn collateral_token_price(&self, is_long: bool) -> &Price<T> {
        if is_long {
            &self.long_token_price
        } else {
            &self.short_token_price
        }
    }
}

impl<T> Prices<T>
where
    T: num_traits::Zero + CheckedAdd + CheckedDiv + num_traits::One,
{
    /// Check if the prices is valid.
    pub fn is_valid(&self) -> bool {
        self.index_token_price.is_valid()
            && self.long_token_price.is_valid()
            && self.short_token_price.is_valid()
    }

    /// Validate the prices.
    pub fn validate(&self) -> crate::Result<()> {
        if self.is_valid() {
            Ok(())
        } else {
            Err(crate::Error::InvalidArgument("invalid prices"))
        }
    }
}
