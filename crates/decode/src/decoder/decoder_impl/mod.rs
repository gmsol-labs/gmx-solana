/// Decoders for Solana datas.
#[cfg(feature = "solana-decoder")]
pub mod solana_decoder;

#[cfg(feature = "solana-decoder")]
pub use solana_decoder::{CPIEventFilter, CPIEvents, TransactionDecoder};

use crate::{AccountAccess, DecodeError, Visitor};

use super::Decoder;

/// Decoder derived from [`AccountAccess`].
#[derive(Debug, Clone)]
pub struct AccountAccessDecoder<A>(A);

impl<A> AccountAccessDecoder<A> {
    /// Create a [`Decoder`] from an [`AccountAccess`].
    pub fn new(account: A) -> Self {
        Self(account)
    }
}

impl<A: AccountAccess> Decoder for AccountAccessDecoder<A> {
    fn decode_account<V>(&self, visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor,
    {
        visitor.visit_account(&self.0)
    }

    fn decode_transaction<V>(&self, _visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor,
    {
        Err(DecodeError::InvalidType(
            "Expecting `Transaction` but found `AccountInfo`".to_string(),
        ))
    }

    fn decode_anchor_cpi_events<V>(&self, _visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor,
    {
        Err(DecodeError::InvalidType(
            "Expecting `AnchorCPIEvents` but found `AccountInfo`".to_string(),
        ))
    }

    fn decode_owned_data<V>(&self, visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor,
    {
        visitor.visit_owned_data(&self.0.owner()?, self.0.data()?)
    }

    fn decode_bytes<V>(&self, visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor,
    {
        visitor.visit_bytes(self.0.data()?)
    }
}
