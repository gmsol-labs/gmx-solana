use anchor_lang::{error, Result};
use gmsol_utils::fixed_str::{self, FixedStrError};

use crate::CoreError;

/// Fixed size string to bytes.
pub fn fixed_str_to_bytes<const MAX_LEN: usize>(name: &str) -> Result<[u8; MAX_LEN]> {
    fixed_str::fixed_str_to_bytes(name)
        .map_err(CoreError::from)
        .map_err(|err| error!(err))
}

/// Bytes to fixed size string.
pub fn bytes_to_fixed_str<const MAX_LEN: usize>(bytes: &[u8; MAX_LEN]) -> Result<&str> {
    fixed_str::bytes_to_fixed_str(bytes)
        .map_err(CoreError::from)
        .map_err(|err| error!(err))
}

impl From<FixedStrError> for CoreError {
    fn from(err: FixedStrError) -> Self {
        anchor_lang::prelude::msg!("Fixed Str Error: {}", err);
        match err {
            FixedStrError::ExceedMaxLengthLimit => Self::ExceedMaxLengthLimit,
            FixedStrError::InvalidFormat | FixedStrError::Utf8(_) => Self::InvalidArgument,
        }
    }
}
