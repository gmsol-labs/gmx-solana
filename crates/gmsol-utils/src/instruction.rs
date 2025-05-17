use anchor_lang::{
    prelude::{zero_copy, AccountMeta, Pubkey},
    solana_program::instruction::Instruction,
};

const MAX_FLAGS: usize = 8;

/// Instruction error.
#[derive(Debug, thiserror::Error)]
pub enum InstructionError {
    /// Failed to get wallet.
    #[error("failed to get wallet")]
    FailedToGetWallet,
}

/// Instruction Account.
#[zero_copy]
pub struct InstructionAccount {
    /// Flags.
    pub flags: InstructionAccountFlagContainer,
    /// Pubkey.
    pub pubkey: Pubkey,
}

impl crate::InitSpace for InstructionAccount {
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

crate::flags!(InstructionAccountFlag, MAX_FLAGS, u8);

impl<'a> From<&'a InstructionAccount> for AccountMeta {
    fn from(a: &'a InstructionAccount) -> Self {
        Self {
            pubkey: a.pubkey,
            is_signer: a.flags.get_flag(InstructionAccountFlag::Signer),
            is_writable: a.flags.get_flag(InstructionAccountFlag::Writable),
        }
    }
}

/// Instruction Access.
pub trait InstructionAccess {
    /// Get wallet.
    fn wallet(&self) -> Result<Pubkey, InstructionError>;

    /// Get program ID.
    fn program_id(&self) -> &Pubkey;

    /// Get data.
    fn data(&self) -> &[u8];

    /// Get the number of accounts.
    fn num_accounts(&self) -> usize;

    /// Get accounts.
    fn accounts(&self) -> impl Iterator<Item = &InstructionAccount>;

    /// Convert to instruction.
    fn to_instruction(
        &self,
        mark_executor_wallet_as_signer: bool,
    ) -> Result<Instruction, InstructionError> {
        let mut accounts = self
            .accounts()
            .map(From::from)
            .collect::<Vec<AccountMeta>>();

        // When performing a CPI, the PDA doesn't need to be explicitly marked as a signer,
        // so we've made it optional to reduce computational overhead.
        if mark_executor_wallet_as_signer {
            let executor_wallet = self.wallet()?;
            accounts
                .iter_mut()
                .filter(|a| a.pubkey == executor_wallet)
                .for_each(|a| a.is_signer = true);
        }

        Ok(Instruction {
            program_id: *self.program_id(),
            accounts,
            data: self.data().to_vec(),
        })
    }
}
