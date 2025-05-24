use typed_builder::TypedBuilder;

/// Basic Position Parameters.
#[derive(Debug, Clone, Copy, TypedBuilder)]
pub struct PositionParams<T> {
    min_position_size_usd: T,
    min_collateral_value: T,
    min_collateral_factor: T,
    max_positive_position_impact_factor: T,
    max_negative_position_impact_factor: T,
    max_position_impact_factor_for_liquidations: T,
}

impl<T> PositionParams<T> {
    /// Create a new [`PositionParams`].
    pub fn new(
        min_position_size_usd: T,
        min_collateral_value: T,
        min_collateral_factor: T,
        max_positive_position_impact_factor: T,
        max_negative_position_impact_factor: T,
        max_position_impact_factor_for_liquidations: T,
    ) -> Self {
        Self {
            min_position_size_usd,
            min_collateral_value,
            min_collateral_factor,
            max_positive_position_impact_factor,
            max_negative_position_impact_factor,
            max_position_impact_factor_for_liquidations,
        }
    }

    /// Get min position size usd.
    pub fn min_position_size_usd(&self) -> &T {
        &self.min_position_size_usd
    }

    /// Get min collateral value.
    pub fn min_collateral_value(&self) -> &T {
        &self.min_collateral_value
    }

    /// Get min collateral factor.
    pub fn min_collateral_factor(&self) -> &T {
        &self.min_collateral_factor
    }

    /// Get max positive position impact factor.
    pub fn max_positive_position_impact_factor(&self) -> &T {
        &self.max_positive_position_impact_factor
    }

    /// Get max negative position impact factor.
    pub fn max_negative_position_impact_factor(&self) -> &T {
        &self.max_negative_position_impact_factor
    }

    /// Get max position impact factor for liquidations.
    pub fn max_position_impact_factor_for_liquidations(&self) -> &T {
        &self.max_position_impact_factor_for_liquidations
    }
}

/// Position Impact Distribution Parameters.
#[derive(Debug, Clone, Copy, TypedBuilder)]
pub struct PositionImpactDistributionParams<T> {
    distribute_factor: T,
    min_position_impact_pool_amount: T,
}

impl<T> PositionImpactDistributionParams<T> {
    /// Get distribution rate factor.
    pub fn distribute_factor(&self) -> &T {
        &self.distribute_factor
    }

    /// Get min position impact pool amount.
    pub fn min_position_impact_pool_amount(&self) -> &T {
        &self.min_position_impact_pool_amount
    }
}
