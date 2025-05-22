use solana_sdk::signature::Signature;

use crate::{decode::Decode, error::DecodeError};

/// Access a Anchor CPI Event.
pub trait AnchorCPIEventsAccess<'a> {
    /// Get the slot of the transaction where the events were generated.
    fn slot(&self) -> Result<u64, DecodeError>;

    /// Get the index in the block of the transaction where the events were generated.
    ///
    /// ## Note
    /// The `index` may be `None` because for old transaction info format,
    /// the `index` of the transaction is not provided.
    fn index(&self) -> Result<Option<usize>, DecodeError>;

    /// Get the signature of the transaction where the events were generated.
    fn signature(&self) -> Result<&Signature, DecodeError>;

    /// Decode next event.
    fn next_event<T>(&mut self) -> Result<Option<T>, DecodeError>
    where
        T: Decode;
}
