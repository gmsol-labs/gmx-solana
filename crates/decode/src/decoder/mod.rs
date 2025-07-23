use crate::{decode::visitor::Visitor, error::DecodeError};

/// Account Access.
pub mod account_access;

/// Transaction Access.
pub mod transaction_access;

/// CPI Event Access.
pub mod cpi_event_access;

/// Decoder implementations.
pub mod decoder_impl;

pub use decoder_impl::*;

/// Decoder for received program data.
pub trait Decoder {
    /// Hint that the visitor is expecting an `AccountInfo`.
    fn decode_account<V>(&self, visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor;

    /// Hint that the visitor is expecting a `Transaction`.
    fn decode_transaction<V>(&self, visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor;

    /// Hint that the visitor is expecting `AnchorCPIEvent` list.
    fn decode_anchor_cpi_events<V>(&self, visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor;

    /// Hint that the visitor is expecting a `OwnedData`.
    ///
    /// It can be the data of an `Event` of an `Instruction`.
    fn decode_owned_data<V>(&self, visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor;

    /// Hint that the visitor is expecting a `Data`.
    ///
    /// It can be the data of an `Event` of an `Instruction`.
    fn decode_bytes<V>(&self, visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor;
}

impl<D: Decoder> Decoder for &D {
    fn decode_account<V>(&self, visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor,
    {
        (**self).decode_account(visitor)
    }

    fn decode_transaction<V>(&self, visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor,
    {
        (**self).decode_transaction(visitor)
    }

    fn decode_anchor_cpi_events<V>(&self, visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor,
    {
        (**self).decode_anchor_cpi_events(visitor)
    }

    fn decode_owned_data<V>(&self, visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor,
    {
        (**self).decode_owned_data(visitor)
    }

    fn decode_bytes<V>(&self, visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor,
    {
        (**self).decode_bytes(visitor)
    }
}
