use crate::{decoder::Decoder, error::DecodeError};

/// Visitor.
pub mod visitor;

/// Type that can be decoded by a [`Decoder`].
pub trait Decode: Send + Sync + Sized {
    /// Decode with the given [`Decoder`].
    fn decode<D: Decoder>(decoder: D) -> Result<Self, DecodeError>;
}

impl<T: Decode> Decode for Box<T> {
    fn decode<D: Decoder>(decoder: D) -> Result<Self, DecodeError> {
        let decoded = T::decode(decoder)?;
        Ok(Box::new(decoded))
    }
}
