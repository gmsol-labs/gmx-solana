use anchor_lang::{prelude::*, solana_program::program::invoke_signed};
use gmsol_store::{
    program::GmsolStore,
    utils::{CpiAuthentication, WithStore},
    CoreError,
};

use crate::states::{Executor, InstructionAccess, InstructionHeader, InstructionLoader};

/// The accounts definition for [`create_instruction_buffer`](crate::gmsol_timelock::create_instruction_buffer).
#[derive(Accounts)]
#[instruction(num_accounts: u16, data_len: u16)]
pub struct CreateInstructionBuffer<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    pub store: UncheckedAccount<'info>,
    /// Expected executor.
    #[account(has_one = store)]
    pub executor: AccountLoader<'info, Executor>,
    /// Instruction buffer to create.
    #[account(
        init,
        payer = authority,
        space = 8 + InstructionHeader::init_space(num_accounts, data_len),
    )]
    pub instruction_buffer: AccountLoader<'info, InstructionHeader>,
    /// Instruction Program.
    /// CHECK: only used as an address.
    pub instruction_program: UncheckedAccount<'info>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
    /// The system program.
    pub system_program: Program<'info, System>,
}

/// Create instruction buffer.
/// # CHECK
/// Only [`TIMELOCK_KEEPER`](crate::roles::TIMELOCK_KEEPER) can use.
pub(crate) fn unchecked_create_instruction_buffer<'info>(
    ctx: Context<'_, '_, 'info, 'info, CreateInstructionBuffer<'info>>,
    num_accounts: u16,
    data_len: u16,
    data: &[u8],
) -> Result<()> {
    let remaining_accounts = ctx.remaining_accounts;
    let num_accounts = usize::from(num_accounts);
    require_gte!(
        remaining_accounts.len(),
        num_accounts,
        CoreError::InvalidArgument
    );
    require_eq!(
        data.len(),
        usize::from(data_len),
        CoreError::InvalidArgument
    );

    {
        let executor = ctx.accounts.executor.load()?;
        msg!(
            "[Timelock] creating instruction buffer for program {}, with executor {}",
            ctx.accounts.instruction_program.key,
            executor.role_name()?,
        );
    }

    ctx.accounts.instruction_buffer.load_and_init_instruction(
        ctx.accounts.executor.key(),
        ctx.accounts.instruction_program.key(),
        data,
        &remaining_accounts[0..num_accounts],
    )?;

    Ok(())
}

impl<'info> WithStore<'info> for CreateInstructionBuffer<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for CreateInstructionBuffer<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}

/// The accounts definition for [`approve_instruction`](crate::gmsol_timelock::approve_instruction).
#[derive(Accounts)]
pub struct ApproveInstruction<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    pub store: UncheckedAccount<'info>,
    /// Instruction to approve.
    #[account(mut)]
    pub instruction: AccountLoader<'info, InstructionHeader>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
}

/// Approve instruction.
/// # CHECK
/// Only [`TIMELOCK_ADMIN`](crate::roles::TIMELOCK_ADMIN) can use.
pub(crate) fn unchecked_approve_instruction(ctx: Context<ApproveInstruction>) -> Result<()> {
    ctx.accounts.instruction.load_mut()?.approve()
}

impl<'info> WithStore<'info> for ApproveInstruction<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for ApproveInstruction<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}

/// The accounts definition for [`cancel_instruction`](crate::gmsol_timelock::cancel_instruction).
#[derive(Accounts)]
pub struct CancelInstruction<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    pub store: UncheckedAccount<'info>,
    /// Instruction to cancel.
    #[account(mut, close = authority)]
    pub instruction: AccountLoader<'info, InstructionHeader>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
}

/// Cancel instruction.
/// # CHECK
/// Only [`TIMELOCK_ADMIN`](crate::roles::TIMELOCK_ADMIN) can use.
pub(crate) fn unchecked_cancel_instruction(_ctx: Context<CancelInstruction>) -> Result<()> {
    Ok(())
}

impl<'info> WithStore<'info> for CancelInstruction<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for CancelInstruction<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}

/// The acccounts definition for [`execute_instruction`](crate::gmsol_timelock::execute_instruction).
#[derive(Accounts)]
pub struct ExecuteInstruction<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    pub store: UncheckedAccount<'info>,
    /// Executor.
    #[account(has_one = store)]
    pub executor: AccountLoader<'info, Executor>,
    /// Instruction to execute.
    #[account(mut, has_one = executor, close = authority)]
    pub instruction: AccountLoader<'info, InstructionHeader>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
}

/// Execute instruction.
/// # CHECK
/// Only [`TIMELOCK_ADMIN`](crate::roles::TIMELOCK_ADMIN) can use.
pub(crate) fn unchecked_execute_instruction(ctx: Context<ExecuteInstruction>) -> Result<()> {
    let remaining_accounts = ctx.remaining_accounts;

    let instruction = ctx.accounts.instruction.load_instruction()?;

    require!(
        instruction.header().is_executable(86400)?,
        CoreError::PreconditionsAreNotMet
    );

    let signer = ctx.accounts.executor.load()?.signer();

    invoke_signed(
        &instruction.to_instruction(),
        remaining_accounts,
        &[&signer.as_seeds()],
    )?;
    Ok(())
}

impl<'info> WithStore<'info> for ExecuteInstruction<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for ExecuteInstruction<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}
