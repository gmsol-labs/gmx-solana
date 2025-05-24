use typed_builder::TypedBuilder;

/// Price impact parameters.
#[derive(Debug, Clone, Copy, TypedBuilder)]
pub struct PriceImpactParams<T> {
    exponent: T,
    positive_factor: T,
    negative_factor: T,
}

impl<T> PriceImpactParams<T> {
    /// Exponent.
    pub fn exponent(&self) -> &T {
        &self.exponent
    }

    /// Positive factor.
    pub fn positive_factor(&self) -> &T {
        &self.positive_factor
    }

    /// Negative factor.
    pub fn negative_factor(&self) -> &T {
        &self.negative_factor
    }

    /// Get adjusted swap factors.
    pub fn adjusted_factors(&self) -> (&T, &T)
    where
        T: Ord + crate::num::Unsigned,
    {
        if self.positive_factor > self.negative_factor {
            (&self.negative_factor, &self.negative_factor)
        } else {
            (&self.positive_factor, &self.negative_factor)
        }
    }
}
