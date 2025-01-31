use solana_sdk::pubkey::Pubkey;

use crate::Decoder;

/// A data decoder.
pub struct OwnedDataDecoder<'a>(&'a Pubkey, &'a [u8]);

impl<'a> OwnedDataDecoder<'a> {
    /// Create a [`Decoder`] from the data directly.
    pub fn new(program_id: &'a Pubkey, data: &'a [u8]) -> Self {
        Self(program_id, data)
    }
}

impl Decoder for OwnedDataDecoder<'_> {
    fn decode_account<V>(&self, _visitor: V) -> Result<V::Value, crate::DecodeError>
    where
        V: crate::Visitor,
    {
        Err(crate::DecodeError::InvalidType(
            "Expecting `Account` but found `Data`".to_string(),
        ))
    }

    fn decode_transaction<V>(&self, _visitor: V) -> Result<V::Value, crate::DecodeError>
    where
        V: crate::Visitor,
    {
        Err(crate::DecodeError::InvalidType(
            "Expecting `Transaction` but found `Data`".to_string(),
        ))
    }

    fn decode_anchor_cpi_events<V>(&self, _visitor: V) -> Result<V::Value, crate::DecodeError>
    where
        V: crate::Visitor,
    {
        Err(crate::DecodeError::InvalidType(
            "Expecting `AnchorCPIEvents` but found `Data`".to_string(),
        ))
    }

    fn decode_owned_data<V>(&self, visitor: V) -> Result<V::Value, crate::DecodeError>
    where
        V: crate::Visitor,
    {
        visitor.visit_owned_data(self.0, self.1)
    }

    fn decode_bytes<V>(&self, visitor: V) -> Result<V::Value, crate::DecodeError>
    where
        V: crate::Visitor,
    {
        visitor.visit_bytes(self.1)
    }
}
