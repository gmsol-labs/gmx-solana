use std::{fmt, marker::PhantomData};

use solana_sdk::pubkey::Pubkey;

use crate::{value::utils::OwnedDataDecoder, Decode, Visitor};

/// Data owned by a program.
#[derive(Debug, Clone, Copy)]
pub struct OwnedData<T> {
    owner: Pubkey,
    data: T,
}

impl<T> OwnedData<T> {
    /// Get owner program id.
    pub fn owner(&self) -> &Pubkey {
        &self.owner
    }

    /// Get data.
    pub fn data(&self) -> &T {
        &self.data
    }

    /// Consume the [`OwnedData`] and get the inner data.
    pub fn into_data(self) -> T {
        self.data
    }
}

impl<T: Decode> Decode for OwnedData<T> {
    fn decode<D: crate::Decoder>(decoder: D) -> Result<Self, crate::DecodeError> {
        struct Data<T>(PhantomData<T>);

        impl<T: Decode> Visitor for Data<T> {
            type Value = OwnedData<T>;

            fn visit_owned_data(
                self,
                program_id: &Pubkey,
                data: &[u8],
            ) -> Result<Self::Value, crate::DecodeError> {
                let data = T::decode(OwnedDataDecoder::new(program_id, data))?;
                let program_id = *program_id;
                Ok(OwnedData {
                    owner: program_id,
                    data,
                })
            }
        }

        decoder.decode_owned_data(Data::<T>(PhantomData))
    }
}

/// Unknown Data.
#[derive(Clone)]
pub struct UnknownOwnedData {
    program_id: Pubkey,
    data: Vec<u8>,
}

impl fmt::Debug for UnknownOwnedData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use base64::prelude::*;
        write!(
            f,
            "UnknownData({} => {})",
            self.program_id,
            BASE64_STANDARD.encode(&self.data)
        )
    }
}

impl Decode for UnknownOwnedData {
    fn decode<D: crate::Decoder>(decoder: D) -> Result<Self, crate::DecodeError> {
        struct Data;

        impl Visitor for Data {
            type Value = UnknownOwnedData;

            fn visit_owned_data(
                self,
                program_id: &Pubkey,
                data: &[u8],
            ) -> Result<Self::Value, crate::DecodeError> {
                Ok(UnknownOwnedData {
                    program_id: *program_id,
                    data: data.to_owned(),
                })
            }
        }

        decoder.decode_owned_data(Data)
    }
}
