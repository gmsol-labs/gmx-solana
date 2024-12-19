use anchor_lang::{prelude::*, solana_program::program::invoke_signed};
use gmsol_store::{
    program::GmsolStore,
    states::{Seed, MAX_ROLE_NAME_LEN},
    utils::{fixed_str::fixed_str_to_bytes, CpiAuthenticate, CpiAuthentication, WithStore},
    CoreError,
};

use crate::{
    roles,
    states::{
        config::TimelockConfig, Executor, InstructionAccess, InstructionHeader, InstructionLoader,
    },
};

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
    signers: &[u16],
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
        signers,
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
#[instruction(role: String)]
pub struct ApproveInstruction<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    pub store: UncheckedAccount<'info>,
    /// Executor.
    #[account(
        has_one = store,
        constraint = executor.load()?.role_name()? == role.as_str() @ CoreError::InvalidArgument,
        seeds = [
            Executor::SEED,
            store.key.as_ref(),
            &fixed_str_to_bytes::<MAX_ROLE_NAME_LEN>(&role)?,
        ],
        bump = executor.load()?.bump,
    )]
    pub executor: AccountLoader<'info, Executor>,
    /// Instruction to approve.
    #[account(mut, has_one = executor)]
    pub instruction: AccountLoader<'info, InstructionHeader>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
}

/// Approve instruction.
pub(crate) fn approve_instruction(ctx: Context<ApproveInstruction>, role: &str) -> Result<()> {
    let timelocked_role = [roles::TIMELOCKED, role].concat();
    CpiAuthenticate::only(&ctx, &timelocked_role)?;
    msg!(
        "[Timelock] approving a `{}` instruction by a `{}`",
        role,
        timelocked_role
    );
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
    /// Executor.
    #[account(has_one = store)]
    pub executor: AccountLoader<'info, Executor>,
    /// Instruction to cancel.
    #[account(mut, has_one = executor, close = authority)]
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
    /// Timelock config.
    #[account(has_one = store)]
    pub timelock_config: AccountLoader<'info, TimelockConfig>,
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
/// Only [`TIMELOCK_KEEPER`](crate::roles::TIMELOCK_KEEPER) can use.
pub(crate) fn unchecked_execute_instruction(ctx: Context<ExecuteInstruction>) -> Result<()> {
    let remaining_accounts = ctx.remaining_accounts;

    let instruction = ctx.accounts.instruction.load_instruction()?;

    let delay = ctx.accounts.timelock_config.load()?.delay();
    require!(
        instruction.header().is_executable(delay)?,
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
