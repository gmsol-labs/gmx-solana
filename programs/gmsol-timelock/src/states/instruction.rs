use std::cell::{Ref, RefMut};

use anchor_lang::prelude::*;
use gmsol_store::{
    utils::pubkey::{optional_address, DEFAULT_PUBKEY},
    CoreError,
};
use gmsol_utils::{instruction::InstructionError, InitSpace};

use crate::states::create_executor_wallet_pda;

pub use gmsol_utils::instruction::{InstructionAccess, InstructionAccount, InstructionAccountFlag};

const MAX_FLAGS: usize = 8;

/// Instruction Header.
#[account(zero_copy)]
pub struct InstructionHeader {
    version: u8,
    flags: InstructionFlagContainer,
    wallet_bump: u8,
    padding_0: [u8; 5],
    /// Approved ts.
    approved_at: i64,
    /// Executor.
    pub(crate) executor: Pubkey,
    /// Program ID.
    program_id: Pubkey,
    /// Number of accounts.
    num_accounts: u16,
    /// Data length.
    data_len: u16,
    padding_1: [u8; 12],
    pub(crate) rent_receiver: Pubkey,
    approver: Pubkey,
    reserved: [u8; 64],
}

impl InstructionHeader {
    /// Get space.
    pub(crate) fn init_space(num_accounts: u16, data_len: u16) -> usize {
        std::mem::size_of::<Self>()
            + usize::from(data_len)
            + usize::from(num_accounts) * InstructionAccount::INIT_SPACE
    }

    /// Approve.
    pub(crate) fn approve(&mut self, approver: Pubkey) -> Result<()> {
        require!(!self.is_approved(), CoreError::PreconditionsAreNotMet);
        require_keys_eq!(
            self.approver,
            DEFAULT_PUBKEY,
            CoreError::PreconditionsAreNotMet
        );

        require_keys_neq!(approver, DEFAULT_PUBKEY, CoreError::InvalidArgument);

        let clock = Clock::get()?;

        self.flags.set_flag(InstructionFlag::Approved, true);
        self.approved_at = clock.unix_timestamp;
        self.approver = approver;

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

    /// Get approver.
    pub fn apporver(&self) -> Option<&Pubkey> {
        optional_address(&self.approver)
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

    /// Get executor wallet.
    pub fn wallet(&self) -> Result<Pubkey> {
        match create_executor_wallet_pda(self.executor(), self.wallet_bump, &crate::ID) {
            Ok(address) => Ok(address),
            Err(err) => {
                msg!("[Ix Buffer] failed to create wallet pda: {}", err);
                err!(CoreError::Internal)
            }
        }
    }

    /// Get rent receiver.
    pub fn rent_receiver(&self) -> &Pubkey {
        &self.rent_receiver
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

/// Reference to the instruction.
pub struct InstructionRef<'a> {
    header: Ref<'a, InstructionHeader>,
    data: Ref<'a, [u8]>,
    accounts: Ref<'a, [u8]>,
}

impl InstructionRef<'_> {
    pub(crate) fn header(&self) -> &InstructionHeader {
        &self.header
    }
}

/// Instruction Loader.
pub trait InstructionLoader<'info> {
    /// Load instruction.
    fn load_instruction(&self) -> Result<InstructionRef>;

    /// Load and initialize the instruction.
    #[allow(clippy::too_many_arguments)]
    fn load_and_init_instruction(
        &self,
        executor: Pubkey,
        wallet_bump: u8,
        rent_receiver: Pubkey,
        program_id: Pubkey,
        data: &[u8],
        accounts: &[AccountInfo<'info>],
        signers: &[u16],
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

        let expected_accounts_len = usize::from(header.num_accounts);
        require_eq!(accounts.len(), expected_accounts_len, CoreError::Internal);

        Ok(InstructionRef {
            header,
            data,
            accounts,
        })
    }

    fn load_and_init_instruction(
        &self,
        executor: Pubkey,
        wallet_bump: u8,
        rent_receiver: Pubkey,
        program_id: Pubkey,
        instruction_data: &[u8],
        instruction_accounts: &[AccountInfo<'info>],
        signers: &[u16],
    ) -> Result<InstructionRef> {
        use gmsol_store::utils::dynamic_access::get_mut;

        // Initialize the header.
        {
            let data_len = instruction_data.len().try_into()?;
            let num_accounts = instruction_accounts.len().try_into()?;
            let mut header = self.load_init()?;
            header.wallet_bump = wallet_bump;
            header.executor = executor;
            header.program_id = program_id;
            header.num_accounts = num_accounts;
            header.data_len = data_len;
            header.rent_receiver = rent_receiver;

            drop(header);

            self.exit(&crate::ID)?;
        }

        // Initialize remaining data.
        {
            // Check the account.
            self.load_mut()?;

            let data = self.as_ref().try_borrow_mut_data()?;

            msg!("[Timelock] buffer size: {}", data.len());

            let (_disc, remaining_data) = RefMut::map_split(data, |d| d.split_at_mut(8));
            let (header, remaining_data) = RefMut::map_split(remaining_data, |d| {
                d.split_at_mut(std::mem::size_of::<InstructionHeader>())
            });
            let header = RefMut::map(header, bytemuck::from_bytes_mut::<InstructionHeader>);
            let data_len = usize::from(header.data_len);
            let (mut data, mut accounts) =
                RefMut::map_split(remaining_data, |d| d.split_at_mut(data_len));

            data.copy_from_slice(instruction_data);

            let wallet = header.wallet()?;

            for (idx, account) in instruction_accounts.iter().enumerate() {
                let idx_u16: u16 = idx
                    .try_into()
                    .map_err(|_| error!(CoreError::InvalidArgument))?;
                let dst = get_mut::<InstructionAccount>(&mut accounts, idx)
                    .ok_or_else(|| error!(CoreError::InvalidArgument))?;

                let address = account.key();
                let is_signer = signers.contains(&idx_u16);
                if is_signer {
                    // Currently only the executor wallet is allowed to be a signer.
                    require_keys_eq!(wallet, address, CoreError::InvalidArgument);
                }

                dst.pubkey = address;
                dst.flags
                    .set_flag(InstructionAccountFlag::Signer, is_signer);
                dst.flags
                    .set_flag(InstructionAccountFlag::Writable, account.is_writable);
            }
        }

        self.load_instruction()
    }
}

impl InstructionAccess for InstructionRef<'_> {
    fn wallet(&self) -> std::result::Result<Pubkey, InstructionError> {
        self.header
            .wallet()
            .map_err(|_| InstructionError::FailedToGetWallet)
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
        use gmsol_store::utils::dynamic_access::get;

        let num_accounts = self.num_accounts();

        (0..num_accounts).map(|idx| get(&self.accounts, idx).expect("must exist"))
    }
}

/// Utils for using instruction buffer.
#[cfg(feature = "utils")]
pub mod utils {

    use anchor_lang::{prelude::Pubkey, AccountDeserialize};
    use bytes::Bytes;
    use gmsol_store::utils::de;
    use gmsol_utils::instruction::InstructionError;

    use super::{InstructionAccess, InstructionHeader};

    /// Instruction Buffer.
    pub struct InstructionBuffer {
        /// Get header.
        pub header: InstructionHeader,
        data: Bytes,
        accounts: Bytes,
    }

    impl InstructionAccess for InstructionBuffer {
        fn wallet(&self) -> Result<Pubkey, InstructionError> {
            self.header
                .wallet()
                .map_err(|_| InstructionError::FailedToGetWallet)
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
