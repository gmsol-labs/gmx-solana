use gmsol_model::price::Prices;
use gmsol_programs::model::PositionModel;
use status::PositionStatus;

/// Position status.
pub mod status;

/// Position Calculations.
pub trait PositionCalculations {
    /// Calculate position status.
    fn status(&self, prices: &Prices<u128>) -> crate::Result<PositionStatus>;
}

// impl PositionCalculations for PositionModel {
//     fn status(&self, _prices: &Prices<u128>) -> crate::Result<PositionStatus> {
//         Err(crate::Error::unknown("unimplemented"))
//     }
// }
