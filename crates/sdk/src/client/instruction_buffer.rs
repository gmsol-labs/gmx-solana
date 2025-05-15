use bytes::Bytes;
use gmsol_programs::{
    anchor_lang::{self, AccountDeserialize},
    gmsol_timelock::{accounts::InstructionHeader, ID},
};
use gmsol_utils::{
    dynamic_access::get,
    instruction::{InstructionAccess, InstructionAccount, InstructionError},
};
use solana_sdk::pubkey::{Pubkey, PubkeyError};

use crate::{
    pda::TIMELOCK_EXECUTOR_WALLET_SEED,
    utils::zero_copy::{check_discriminator, try_deserialize_unchecked},
};

/// Instruction Buffer.
pub struct InstructionBuffer {
    /// Get header.
    pub header: InstructionHeader,
    data: Bytes,
    accounts: Bytes,
}

impl InstructionAccess for InstructionBuffer {
    fn wallet(&self) -> Result<Pubkey, InstructionError> {
        match create_executor_wallet_pda(&self.header.executor, self.header.wallet_bump, &ID) {
            Ok(address) => Ok(address),
            Err(_) => Err(InstructionError::FailedToGetWallet),
        }
    }

    fn program_id(&self) -> &Pubkey {
        &self.header.program_id
    }

    fn data(&self) -> &[u8] {
        &self.data
    }

    fn num_accounts(&self) -> usize {
        usize::from(self.header.num_accounts)
    }

    fn accounts(&self) -> impl Iterator<Item = &InstructionAccount> {
        let num_accounts = self.num_accounts();

        (0..num_accounts).map(|idx| get(&self.accounts, idx).expect("must exist"))
    }
}

impl AccountDeserialize for InstructionBuffer {
    fn try_deserialize(buf: &mut &[u8]) -> anchor_lang::Result<Self> {
        check_discriminator::<InstructionHeader>(buf)?;
        Self::try_deserialize_unchecked(buf)
    }

    fn try_deserialize_unchecked(buf: &mut &[u8]) -> anchor_lang::Result<Self> {
        let header = try_deserialize_unchecked::<InstructionHeader>(buf)?;
        let (_disc, data) = buf.split_at(8);
        let (_header, remaining_data) = data.split_at(std::mem::size_of::<InstructionHeader>());
        let data_len = usize::from(header.data_len);
        let (data, accounts) = remaining_data.split_at(data_len);
        Ok(Self {
            header,
            data: Bytes::copy_from_slice(data),
            accounts: Bytes::copy_from_slice(accounts),
        })
    }
}

/// Create executor wallet PDA.
pub fn create_executor_wallet_pda(
    executor: &Pubkey,
    wallet_bump: u8,
    timelock_program_id: &Pubkey,
) -> std::result::Result<Pubkey, PubkeyError> {
    Pubkey::create_program_address(
        &[
            TIMELOCK_EXECUTOR_WALLET_SEED,
            executor.as_ref(),
            &[wallet_bump],
        ],
        timelock_program_id,
    )
}
