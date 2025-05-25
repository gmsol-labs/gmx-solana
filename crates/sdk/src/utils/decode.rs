use gmsol_decode::{AccountAccess, DecodeError};
use gmsol_solana_utils::utils::WithSlot;
use solana_sdk::{account::Account, pubkey::Pubkey};

/// Account with pubkey.
pub struct KeyedAccount {
    /// The pubkey of the account.
    pub pubkey: Pubkey,
    /// The account data.
    pub account: WithSlot<Account>,
}

impl AccountAccess for KeyedAccount {
    fn owner(&self) -> Result<Pubkey, DecodeError> {
        Ok(self.account.value().owner)
    }

    fn pubkey(&self) -> Result<Pubkey, DecodeError> {
        Ok(self.pubkey)
    }

    fn lamports(&self) -> Result<u64, DecodeError> {
        Ok(self.account.value().lamports)
    }

    fn data(&self) -> Result<&[u8], DecodeError> {
        Ok(&self.account.value().data)
    }

    fn slot(&self) -> Result<u64, DecodeError> {
        Ok(self.account.slot())
    }
}
