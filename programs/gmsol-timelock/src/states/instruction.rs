use std::cell::{Ref, RefMut};

use anchor_lang::{
    prelude::*,
    solana_program::instruction::{AccountMeta, Instruction},
};
use gmsol_store::CoreError;
use gmsol_utils::InitSpace;

const MAX_FLAGS: usize = 8;

/// Instruction Header.
#[account(zero_copy)]
pub struct InstructionHeader {
    flags: InstructionFlagContainer,
    padding_0: [u8; 7],
    /// Approved ts.
    approved_at: i64,
    /// Executor.
    pub(crate) executor: Pubkey,
    /// Program ID.
    program_id: Pubkey,
    /// Data length.
    data_len: u16,
    /// Number of accounts.
    num_accounts: u16,
    padding_1: [u8; 12],
}

impl InstructionHeader {
    /// Get space.
    pub(crate) fn init_space(data_len: u16, num_accounts: u16) -> usize {
        std::mem::size_of::<Self>()
            + usize::from(data_len)
            + usize::from(num_accounts) * InstructionAccount::INIT_SPACE
    }

    /// Approve.
    pub(crate) fn approve(&mut self) -> Result<()> {
        require!(!self.is_approved(), CoreError::PreconditionsAreNotMet);

        let clock = Clock::get()?;

        self.flags.set_flag(InstructionFlag::Approved, true);
        self.approved_at = clock.unix_timestamp;

        Ok(())
    }

    /// Returns whether the instruction is approved.
    pub fn is_approved(&self) -> bool {
        self.flags.get_flag(InstructionFlag::Approved)
    }

    /// Get the approved timestamp.
    pub fn approved_at(&self) -> Option<i64> {
        self.is_approved().then_some(self.approved_at)
    }

    /// Return whether the instruction is executable.
    pub fn is_executable(&self, delay: u32) -> Result<bool> {
        let now = Clock::get()?.unix_timestamp;
        let Some(approved_at) = self.approved_at() else {
            return Ok(false);
        };
        let executable_at = approved_at.saturating_add_unsigned(delay as u64);
        Ok(now >= executable_at)
    }

    /// Get executor.
    pub fn executor(&self) -> &Pubkey {
        &self.executor
    }
}

/// Flags of Instruction.
#[derive(num_enum::IntoPrimitive)]
#[repr(u8)]
pub enum InstructionFlag {
    /// Approved.
    Approved,
    // CHECK: cannot have more than `MAX_FLAGS` flags.
}

gmsol_utils::flags!(InstructionFlag, MAX_FLAGS, u8);

/// Instruction Account.
#[zero_copy]
pub struct InstructionAccount {
    /// Flags.
    flags: InstructionAccountFlagContainer,
    /// Pubkey.
    pubkey: Pubkey,
}

impl gmsol_utils::InitSpace for InstructionAccount {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

/// Flags of Instruction Accounts.
#[derive(num_enum::IntoPrimitive)]
#[repr(u8)]
pub enum InstructionAccountFlag {
    /// Is signer.
    Signer,
    /// Is mutable.
    Writable,
    // CHECK: cannot have more than `MAX_FLAGS` flags.
}

gmsol_utils::flags!(InstructionAccountFlag, MAX_FLAGS, u8);

impl<'a> From<&'a InstructionAccount> for AccountMeta {
    fn from(a: &'a InstructionAccount) -> Self {
        Self {
            pubkey: a.pubkey,
            is_signer: a.flags.get_flag(InstructionAccountFlag::Signer),
            is_writable: a.flags.get_flag(InstructionAccountFlag::Writable),
        }
    }
}

/// Reference to the instruction.
pub struct InstructionRef<'a> {
    header: Ref<'a, InstructionHeader>,
    data: Ref<'a, [u8]>,
    accounts: Ref<'a, [u8]>,
}

/// Instruction Loader.
pub trait InstructionLoader<'info> {
    /// Load instruction.
    fn load_instruction(&self) -> Result<InstructionRef>;

    /// Load and initialize the instruction.
    fn load_and_init_instruction(
        &self,
        executor: Pubkey,
        program_id: Pubkey,
        data: &[u8],
        accounts: &[AccountInfo<'info>],
    ) -> Result<InstructionRef>;
}

impl<'info> InstructionLoader<'info> for AccountLoader<'info, InstructionHeader> {
    fn load_instruction(&self) -> Result<InstructionRef> {
        // Check the account.
        self.load()?;

        let data = self.as_ref().try_borrow_data()?;

        let (_disc, remaining_data) = Ref::map_split(data, |d| d.split_at(8));
        let (header, remaining_data) = Ref::map_split(remaining_data, |d| {
            d.split_at(std::mem::size_of::<InstructionHeader>())
        });
        let header = Ref::map(header, bytemuck::from_bytes::<InstructionHeader>);
        let data_len = usize::from(header.data_len);
        let (data, accounts) = Ref::map_split(remaining_data, |d| d.split_at(data_len));

        Ok(InstructionRef {
            header,
            data,
            accounts,
        })
    }

    fn load_and_init_instruction(
        &self,
        executor: Pubkey,
        program_id: Pubkey,
        instruction_data: &[u8],
        instruction_accounts: &[AccountInfo<'info>],
    ) -> Result<InstructionRef> {
        use gmsol_store::utils::dynamic_access::get_mut;

        // Initialize the header.
        {
            let data_len = instruction_data.len().try_into()?;
            let num_accounts = instruction_accounts.len().try_into()?;
            let mut header = self.load_init()?;
            header.executor = executor;
            header.program_id = program_id;
            header.data_len = data_len;
            header.num_accounts = num_accounts;

            drop(header);

            self.exit(&crate::ID)?;
        }

        // Initialize remaining data.
        {
            // Check the account.
            self.load_mut()?;

            let data = self.as_ref().try_borrow_mut_data()?;

            let (_disc, remaining_data) = RefMut::map_split(data, |d| d.split_at_mut(8));
            let (header, remaining_data) = RefMut::map_split(remaining_data, |d| {
                d.split_at_mut(std::mem::size_of::<InstructionHeader>())
            });
            let header = RefMut::map(header, bytemuck::from_bytes_mut::<InstructionHeader>);
            let data_len = usize::from(header.data_len);
            let (mut data, mut accounts) =
                RefMut::map_split(remaining_data, |d| d.split_at_mut(data_len));

            data.copy_from_slice(instruction_data);

            for (idx, account) in instruction_accounts.iter().enumerate() {
                let dst = get_mut::<InstructionAccount>(&mut accounts, idx)
                    .ok_or_else(|| error!(CoreError::InvalidArgument))?;
                dst.pubkey = account.key();
                dst.flags
                    .set_flag(InstructionAccountFlag::Signer, account.is_signer);
                dst.flags
                    .set_flag(InstructionAccountFlag::Writable, account.is_writable);
            }
        }

        self.load_instruction()
    }
}

/// Instruction Access.
pub trait InstructionAccess {
    /// Get header.
    fn header(&self) -> &InstructionHeader;

    /// Get data.
    fn data(&self) -> &[u8];

    /// Get the number of accounts.
    fn num_accounts(&self) -> usize {
        usize::from(self.header().num_accounts)
    }

    /// Get accounts.
    fn accounts(&self) -> impl Iterator<Item = &InstructionAccount>;

    /// Convert to instruction.
    fn to_instruction(&self) -> Instruction {
        let accounts = self.accounts().map(From::from).collect();
        Instruction {
            program_id: self.header().program_id,
            accounts,
            data: self.data().to_vec(),
        }
    }
}

impl<'a> InstructionAccess for InstructionRef<'a> {
    fn header(&self) -> &InstructionHeader {
        &self.header
    }

    fn data(&self) -> &[u8] {
        &self.data
    }

    fn accounts(&self) -> impl Iterator<Item = &InstructionAccount> {
        use gmsol_store::utils::dynamic_access::get;

        let num_accounts = self.num_accounts();

        (0..num_accounts).map(|idx| get(&self.accounts, idx).expect("must exist"))
    }
}

/// Utils for using instruction buffer.
#[cfg(feature = "utils")]
pub mod utils {

    use anchor_lang::AccountDeserialize;
    use bytes::Bytes;
    use gmsol_store::utils::de;

    use super::{InstructionAccess, InstructionHeader};

    /// Instruction Buffer.
    pub struct InstructionBuffer {
        header: InstructionHeader,
        data: Bytes,
        accounts: Bytes,
    }

    impl InstructionAccess for InstructionBuffer {
        fn header(&self) -> &InstructionHeader {
            &self.header
        }

        fn data(&self) -> &[u8] {
            &self.data
        }

        fn accounts(&self) -> impl Iterator<Item = &super::InstructionAccount> {
            use gmsol_store::utils::dynamic_access::get;

            let num_accounts = self.num_accounts();

            (0..num_accounts).map(|idx| get(&self.accounts, idx).expect("must exist"))
        }
    }

    impl AccountDeserialize for InstructionBuffer {
        fn try_deserialize(buf: &mut &[u8]) -> anchor_lang::Result<Self> {
            de::check_discriminator::<InstructionHeader>(buf)?;
            Self::try_deserialize_unchecked(buf)
        }

        fn try_deserialize_unchecked(buf: &mut &[u8]) -> anchor_lang::Result<Self> {
            let header = de::try_deserailize_unchecked::<InstructionHeader>(buf)?;
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
}
