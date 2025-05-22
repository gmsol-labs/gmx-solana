use anchor_lang::solana_program::pubkey::Pubkey;

use crate::{
    decoder::{account_access::AccountAccess, cpi_event_access::AnchorCPIEventsAccess},
    error::DecodeError,
};

/// Type that walks through a [`Decoder`](crate::Decoder).
pub trait Visitor: Sized {
    /// Value Type.
    type Value;

    /// Visit an account.
    fn visit_account(self, account: impl AccountAccess) -> Result<Self::Value, DecodeError> {
        _ = account;
        Err(DecodeError::InvalidType(
            "Unexpected type `Account`".to_string(),
        ))
    }

    /// Visit Anchor CPI events.
    fn visit_anchor_cpi_events<'a>(
        self,
        events: impl AnchorCPIEventsAccess<'a>,
    ) -> Result<Self::Value, DecodeError> {
        _ = events;
        Err(DecodeError::InvalidType(
            "Unexpected type `AnchorCPIEvents`".to_string(),
        ))
    }

    /// Visit data owned by a program.
    ///
    /// It can be the data of an `Event`, an `Account` or an `Instruction`.
    fn visit_owned_data(
        self,
        program_id: &Pubkey,
        data: &[u8],
    ) -> Result<Self::Value, DecodeError> {
        _ = program_id;
        _ = data;
        Err(DecodeError::InvalidType(
            "Unexpected type `OwnedData`".to_string(),
        ))
    }

    /// Visit bytes.
    fn visit_bytes(self, data: &[u8]) -> Result<Self::Value, DecodeError> {
        _ = data;
        Err(DecodeError::InvalidType(
            "Unexpected type `Bytes`".to_string(),
        ))
    }
}
