#![cfg_attr(docsrs, feature(doc_auto_cfg))]
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

/// Implement [`Decode`] for GMSOL types.
#[cfg(any(feature = "gmsol", feature = "gmsol-programs"))]
pub mod gmsol;

pub use self::{
    decode::{visitor::Visitor, Decode},
    decoder::{account_access::AccountAccess, cpi_event_access::AnchorCPIEventsAccess, Decoder},
    error::DecodeError,
};

pub use paste;

pub use tracing;
