pub use anchor_lang::prelude::AccountMeta;
use solana_sdk::instruction::CompiledInstruction;
pub use solana_sdk::{message::v0::MessageAddressTableLookup, signature::Signature};
pub use solana_transaction_status_client_types::UiTransactionStatusMeta;

use crate::DecodeError;

/// Access a transaction.
pub trait TransactionAccess {
    /// Gets the slot of the transaction where the events were generated.
    fn slot(&self) -> Result<u64, DecodeError>;

    /// Gets the index in the block of the transaction where the events were generated.
    ///
    /// ## Note
    /// The `index` may be `None` because for old transaction info format,
    /// the `index` of the transaction is not provided.
    fn index(&self) -> Result<Option<usize>, DecodeError>;

    /// Gets the signature of the transaction where the events were generated.
    fn signature(&self) -> Result<&Signature, DecodeError>;

    /// Returns the number of signers.
    fn num_signers(&self, is_writable: bool) -> Result<usize, DecodeError>;

    /// Returns the number of accounts.
    fn num_accounts(&self) -> usize;

    /// Gets message signature.
    fn message_signature(&self, idx: usize) -> Option<&Signature>;

    /// Gets account meta by index.
    fn account_meta(&self, idx: usize) -> Result<Option<AccountMeta>, DecodeError>;

    /// Returns the number of address table lookups.
    fn num_address_table_lookups(&self) -> usize;

    /// Gets address table lookup by index.
    fn address_table_lookup(&self, idx: usize) -> Option<&MessageAddressTableLookup>;

    /// Returns the number of instructions.
    fn num_instructions(&self) -> usize;

    /// Gets instruction by index.
    fn instruction(&self, idx: usize) -> Option<&CompiledInstruction>;

    /// Returns transaction status meta if available.
    fn transaction_status_meta(&self) -> Option<&UiTransactionStatusMeta>;
}
