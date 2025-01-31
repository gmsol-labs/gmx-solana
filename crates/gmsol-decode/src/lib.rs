#![deny(missing_docs)]
#![deny(unreachable_pub)]
//! This crate provides utils for decoding GMSOL types.

/// Decoder.
pub mod decoder;

/// Type that can be decoded by [`Decoder`].
pub mod decode;

/// Values.
pub mod value;

/// Errors.
pub mod error;

#[cfg(feature = "gmsol")]
pub(crate) mod gmsol;

pub use self::{
    decode::{visitor::Visitor, Decode},
    decoder::{account_access::AccountAccess, cpi_event_access::AnchorCPIEventsAccess, Decoder},
    error::DecodeError,
};

#[cfg(feature = "gmsol")]
pub use self::gmsol::{GMSOLAccountData, GMSOLCPIEvent, GMSOLData};

pub use paste;

pub use tracing;
