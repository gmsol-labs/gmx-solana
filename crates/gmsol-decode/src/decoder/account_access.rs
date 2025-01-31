use solana_sdk::pubkey::Pubkey;

use super::DecodeError;

/// Access an account info.
pub trait AccountAccess {
    /// Get the owner of the account.
    fn owner(&self) -> Result<Pubkey, DecodeError>;

    /// Get the pubkey of the account.
    fn pubkey(&self) -> Result<Pubkey, DecodeError>;

    /// Get the lamports of the account.
    fn lamports(&self) -> Result<u64, DecodeError>;

    /// Get the account data.
    fn data(&self) -> Result<&[u8], DecodeError>;

    /// Get the slot at which the account data was updated.
    fn slot(&self) -> Result<u64, DecodeError>;
}

impl<A: AccountAccess> AccountAccess for &A {
    fn owner(&self) -> Result<Pubkey, DecodeError> {
        (**self).owner()
    }

    fn pubkey(&self) -> Result<Pubkey, DecodeError> {
        (**self).pubkey()
    }

    fn lamports(&self) -> Result<u64, DecodeError> {
        (**self).lamports()
    }

    fn data(&self) -> Result<&[u8], DecodeError> {
        (**self).data()
    }

    fn slot(&self) -> Result<u64, DecodeError> {
        (**self).slot()
    }
}
