use gmsol_model::price::Prices;
use status::PositionStatus;

/// Position status.
pub mod status;

/// Position Calculations.
pub trait PositionCalculations {
    /// Calculate position status.
    fn status(&self, prices: &Prices<u128>) -> crate::Result<PositionStatus>;
}
