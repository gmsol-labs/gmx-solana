/// Report.
pub mod report;

/// Interface.
pub mod interface;

/// Verifier Program.
pub mod verifier;

/// Utils.
pub mod utils;

/// Error type.
pub mod error;

/// Mock Program.
#[cfg(feature = "mock")]
pub mod mock;

/// GMSOL support.
#[cfg(feature = "gmsol")]
pub mod gmsol;

pub use chainlink_data_streams_report;
pub use error::Error;
pub use report::Report;

/// Type that can be created from [`Report`].
pub trait FromChainlinkReport: Sized {
    /// Create from [`Report`].
    fn from_chainlink_report(report: &Report) -> Result<Self, Error>;
}
