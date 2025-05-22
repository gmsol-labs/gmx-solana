use std::marker::PhantomData;

use solana_sdk::pubkey::Pubkey;

use crate::{decoder::AccountAccessDecoder, AccountAccess, Decode, DecodeError, Decoder, Visitor};

use super::OwnedData;

/// A decoded account.
#[derive(Debug, Clone, Copy)]
pub struct Account<T> {
    pubkey: Pubkey,
    lamports: u64,
    slot: u64,
    data: OwnedData<T>,
}

impl<T> Account<T> {
    /// Get the owner of the account.
    pub fn owner(&self) -> &Pubkey {
        self.data.owner()
    }

    /// Get the address of the account.
    pub fn pubkey(&self) -> &Pubkey {
        &self.pubkey
    }

    /// Get lamports of the account.
    pub fn lamports(&self) -> u64 {
        self.lamports
    }

    /// Get the data of the account.
    pub fn data(&self) -> &T {
        self.data.data()
    }

    /// Convert into the inner data.
    pub fn into_data(self) -> T {
        self.data.into_data()
    }

    /// Get the slot at which the account was updated.
    pub fn slot(&self) -> u64 {
        self.slot
    }
}

impl<T> Decode for Account<T>
where
    T: Decode,
{
    fn decode<D: Decoder>(decoder: D) -> Result<Self, DecodeError> {
        struct AccountVisitor<T>(PhantomData<T>);

        impl<T: Decode> Visitor for AccountVisitor<T> {
            type Value = Account<T>;

            fn visit_account(
                self,
                account: impl AccountAccess,
            ) -> Result<Self::Value, DecodeError> {
                Ok(Account {
                    pubkey: account.pubkey()?,
                    lamports: account.lamports()?,
                    slot: account.slot()?,
                    data: OwnedData::<T>::decode(AccountAccessDecoder::new(account))?,
                })
            }
        }

        decoder.decode_account(AccountVisitor(PhantomData))
    }
}
