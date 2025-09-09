use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};

#[cfg(anchor_lang)]
use anchor_lang::{Id, InstructionData, ToAccountMetas};

/// A solana program.
pub trait Program {
    /// Returns the current program ID.
    fn id(&self) -> &Pubkey;
}

impl<P: Program> Program for &P {
    fn id(&self) -> &Pubkey {
        (**self).id()
    }
}

/// Extension trait for [`Program`].
pub trait ProgramExt: Program {
    /// Create an [`InstructionBuilder`]
    fn instruction(&self, data: Vec<u8>) -> InstructionBuilder<Self>
    where
        Self: Sized,
    {
        InstructionBuilder {
            program: self,
            data,
            accounts: vec![],
        }
    }

    /// Create [`InstructionBuilder`] with [`InstructionData`].
    #[cfg(anchor_lang)]
    fn anchor_instruction(&self, args: impl InstructionData) -> InstructionBuilder<Self>
    where
        Self: Sized,
    {
        self.instruction(args.data())
    }

    /// Convert to account metas.
    ///
    /// If `convert_optional` is `true`, read-only non-signer accounts with
    /// the default program ID as pubkey will be replaced with the current
    /// program ID.
    #[cfg(anchor_lang)]
    fn anchor_accounts(
        &self,
        accounts: impl ToAccountMetas,
        convert_optional: bool,
    ) -> Vec<AccountMeta>
    where
        Self: Id,
    {
        if convert_optional {
            fix_optional_account_metas(accounts, &<Self as Id>::id(), self.id())
        } else {
            accounts.to_account_metas(None)
        }
    }
}

impl<P: ?Sized + Program> ProgramExt for P {}

/// Generic Instruction Builder.
#[derive(Debug, Clone)]
pub struct InstructionBuilder<'a, P> {
    program: &'a P,
    data: Vec<u8>,
    accounts: Vec<AccountMeta>,
}

impl<P> InstructionBuilder<'_, P> {
    /// Append accounts to account list.
    pub fn accounts(mut self, mut accounts: Vec<AccountMeta>) -> Self {
        self.accounts.append(&mut accounts);
        self
    }
}

impl<P: Program> InstructionBuilder<'_, P> {
    /// Build an [`Instruction`].
    pub fn build(self) -> Instruction {
        Instruction {
            program_id: *self.program.id(),
            accounts: self.accounts,
            data: self.data,
        }
    }
}

#[cfg(anchor_lang)]
impl<P: Program + Id> InstructionBuilder<'_, P> {
    /// Append a [`ToAccountMetas`] to account list.
    pub fn anchor_accounts(self, accounts: impl ToAccountMetas, convert_optional: bool) -> Self {
        let accounts = self.program.anchor_accounts(accounts, convert_optional);
        self.accounts(accounts)
    }
}

/// Change the `pubkey` of any readonly, non-signer [`AccountMeta`]
/// with the `pubkey` equal to the original program id to the new one.
///
/// This is a workaround since Anchor will automatically set optional accounts
/// to the Program ID of the program that defines them when they are `None`s,
/// if we use the same program but with different Program IDs, the optional
/// accounts will be set to the wrong addresses.
///
/// ## Warning
/// Use this function only if you fully understand the implications.
#[cfg(anchor_lang)]
pub fn fix_optional_account_metas(
    accounts: impl ToAccountMetas,
    original: &Pubkey,
    current: &Pubkey,
) -> Vec<AccountMeta> {
    let mut metas = accounts.to_account_metas(None);
    if *original == *current {
        // No-op in this case.
        return metas;
    }
    metas.iter_mut().for_each(|meta| {
        if !meta.is_signer && !meta.is_writable && meta.pubkey == *original {
            // We consider it a `None` account. If it is not, please do not use this function.
            meta.pubkey = *current;
        }
    });
    metas
}
