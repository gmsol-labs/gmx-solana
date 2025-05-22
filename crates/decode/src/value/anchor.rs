use std::marker::PhantomData;

use crate::Visitor;

/// Visitor that produces a [`ZeroCopy`](anchor_lang::ZeroCopy).
pub struct ZeroCopyVisitor<T>(PhantomData<T>);

impl<T> Default for ZeroCopyVisitor<T> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<T> Visitor for ZeroCopyVisitor<T>
where
    T: anchor_lang::ZeroCopy,
{
    type Value = T;

    fn visit_bytes(self, data: &[u8]) -> Result<Self::Value, crate::DecodeError> {
        use anchor_lang::prelude::{Error, ErrorCode};

        let disc = T::DISCRIMINATOR;
        if data.len() < disc.len() {
            return Err(Error::from(ErrorCode::AccountDiscriminatorNotFound).into());
        }
        let given_disc = &data[..8];
        if disc != given_disc {
            return Err(Error::from(ErrorCode::AccountDiscriminatorMismatch).into());
        }
        let end = std::mem::size_of::<T>() + 8;
        if data.len() < end {
            return Err(Error::from(ErrorCode::AccountDidNotDeserialize).into());
        }
        let data_without_discriminator = data[8..end].to_vec();
        Ok(*bytemuck::try_from_bytes(&data_without_discriminator)
            .map_err(|err| crate::DecodeError::custom(format!("bytemuck: {err}")))?)
    }
}

/// Implement [`Decode`](crate::Decode) for [`ZeroCopy`](anchor_lang::ZeroCopy).
#[macro_export]
macro_rules! impl_decode_for_zero_copy {
    ($decoded:ty) => {
        impl $crate::Decode for $decoded {
            fn decode<D: $crate::Decoder>(decoder: D) -> Result<Self, $crate::DecodeError> {
                decoder.decode_bytes($crate::value::ZeroCopyVisitor::<$decoded>::default())
            }
        }
    };
}

/// Visitor that produces an [`AccountDeserialize`](anchor_lang::AccountDeserialize).
pub struct AccountDeserializeVisitor<T>(PhantomData<T>);

impl<T> Default for AccountDeserializeVisitor<T> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<T> Visitor for AccountDeserializeVisitor<T>
where
    T: anchor_lang::AccountDeserialize,
{
    type Value = T;

    fn visit_bytes(self, mut data: &[u8]) -> Result<Self::Value, crate::DecodeError> {
        Ok(T::try_deserialize(&mut data)?)
    }
}

/// Implement [`Decode`](crate::Decode) for [`AccountDeserialize`](anchor_lang::AccountDeserialize).
#[macro_export]
macro_rules! impl_decode_for_account_deserialize {
    ($decoded:ty) => {
        impl $crate::Decode for $decoded {
            fn decode<D: $crate::Decoder>(decoder: D) -> Result<Self, $crate::DecodeError> {
                decoder
                    .decode_bytes($crate::value::AccountDeserializeVisitor::<$decoded>::default())
            }
        }
    };
}

/// Visitor that produces an CPI [`Event`](anchor_lang::Event).
pub struct CPIEventVisitor<T>(PhantomData<T>);

impl<T> Default for CPIEventVisitor<T> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<T> Visitor for CPIEventVisitor<T>
where
    T: anchor_lang::Event,
{
    type Value = T;

    fn visit_bytes(self, data: &[u8]) -> Result<Self::Value, crate::DecodeError> {
        use anchor_lang::{
            event::EVENT_IX_TAG_LE,
            prelude::{Error, ErrorCode},
        };

        // Valdiate the ix tag.
        if data.len() < EVENT_IX_TAG_LE.len() {
            return Err(Error::from(ErrorCode::InstructionDidNotDeserialize).into());
        }
        let given_tag = &data[..8];
        if given_tag != EVENT_IX_TAG_LE {
            return Err(crate::DecodeError::custom("not an anchor event ix"));
        }

        let data = &data[8..];

        // Validate the discriminator.
        let disc = T::DISCRIMINATOR;
        if data.len() < disc.len() {
            return Err(Error::from(ErrorCode::InstructionDidNotDeserialize).into());
        }
        let given_disc = &data[..8];
        if disc != given_disc {
            return Err(Error::from(ErrorCode::InstructionDidNotDeserialize).into());
        }

        // Deserialize.
        Ok(T::try_from_slice(&data[8..]).map_err(anchor_lang::prelude::Error::from)?)
    }
}

/// Implement [`Decode`](crate::Decode) for CPI events.
#[macro_export]
macro_rules! impl_decode_for_cpi_event {
    ($decoded:ty) => {
        impl $crate::Decode for $decoded {
            fn decode<D: $crate::Decoder>(decoder: D) -> Result<Self, $crate::DecodeError> {
                decoder.decode_bytes($crate::value::CPIEventVisitor::<$decoded>::default())
            }
        }
    };
}
