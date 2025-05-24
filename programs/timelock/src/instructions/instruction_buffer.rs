use anchor_lang::{prelude::*, solana_program::program::invoke_signed};
use gmsol_store::{
    program::GmsolStore,
    states::{Seed, Store, MAX_ROLE_NAME_LEN},
    utils::{fixed_str::fixed_str_to_bytes, CpiAuthenticate, CpiAuthentication, WithStore},
    CoreError,
};

use crate::{
    roles,
    states::{
        config::TimelockConfig, Executor, ExecutorWalletSigner, InstructionAccess,
        InstructionHeader, InstructionLoader,
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

    let wallet_bump = {
        let executor = ctx.accounts.executor.load()?;
        let wallet_bump = executor.wallet_bump;
        msg!(
            "[Timelock] creating instruction buffer for program {}, with executor `{}`",
            ctx.accounts.instruction_program.key,
            executor.role_name()?,
        );
        wallet_bump
    };

    ctx.accounts.instruction_buffer.load_and_init_instruction(
        ctx.accounts.executor.key(),
        wallet_bump,
        ctx.accounts.authority.key(),
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
    validate_timelocked_role(&ctx, role)?;
    ctx.accounts
        .instruction
        .load_mut()?
        .approve(ctx.accounts.authority.key())
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

/// The accounts definition for [`approve_instructions`](crate::gmsol_timelock::approve_instructions).
#[derive(Accounts)]
#[instruction(role: String)]
pub struct ApproveInstructions<'info> {
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
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
}

/// Approve instructions.
pub(crate) fn approve_instructions<'info>(
    ctx: Context<'_, '_, 'info, 'info, ApproveInstructions<'info>>,
    role: &str,
) -> Result<()> {
    validate_timelocked_role(&ctx, role)?;

    let executor = ctx.accounts.executor.key();
    let approver = ctx.accounts.authority.key();
    for account in ctx.remaining_accounts {
        require!(account.is_writable, ErrorCode::AccountNotMutable);
        let loader = AccountLoader::<InstructionHeader>::try_from(account)?;
        require_keys_eq!(
            *loader.load()?.executor(),
            executor,
            CoreError::InvalidArgument
        );
        loader.load_mut()?.approve(approver)?;
    }

    Ok(())
}

impl<'info> WithStore<'info> for ApproveInstructions<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for ApproveInstructions<'info> {
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
    /// Rent receiver.
    /// CHECK: only used to receive funds.
    #[account(mut)]
    pub rent_receiver: UncheckedAccount<'info>,
    /// Instruction to cancel.
    #[account(
        mut,
        has_one = executor,
        has_one = rent_receiver,
        close = rent_receiver,
    )]
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

/// The accounts definition for [`cancel_instructions`](crate::gmsol_timelock::cancel_instructions).
#[derive(Accounts)]
pub struct CancelInstructions<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    pub store: UncheckedAccount<'info>,
    /// Executor.
    #[account(has_one = store)]
    pub executor: AccountLoader<'info, Executor>,
    /// Rent receiver.
    /// CHECK: only used to receive funds.
    #[account(mut)]
    pub rent_receiver: UncheckedAccount<'info>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
}

/// Cancel instructions that sharing the same `executor` and `rent_receiver`.
/// # CHECK
/// Only [`TIMELOCK_ADMIN`](crate::roles::TIMELOCK_ADMIN) can use.
pub(crate) fn unchecked_cancel_instructions<'info>(
    ctx: Context<'_, '_, 'info, 'info, CancelInstructions<'info>>,
) -> Result<()> {
    let executor = ctx.accounts.executor.key();
    let rent_receiver = ctx.accounts.rent_receiver.key();

    for account in ctx.remaining_accounts {
        require!(account.is_writable, ErrorCode::AccountNotMutable);
        let loader = AccountLoader::<InstructionHeader>::try_from(account)?;

        {
            let header = loader.load()?;
            require_keys_eq!(*header.executor(), executor, CoreError::InvalidArgument);
            require_keys_eq!(
                *header.rent_receiver(),
                rent_receiver,
                CoreError::InvalidArgument
            );
        }

        loader.close(ctx.accounts.rent_receiver.to_account_info())?;
    }

    Ok(())
}

impl<'info> WithStore<'info> for CancelInstructions<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for CancelInstructions<'info> {
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
    pub store: AccountLoader<'info, Store>,
    /// Timelock config.
    #[account(has_one = store)]
    pub timelock_config: AccountLoader<'info, TimelockConfig>,
    /// Executor.
    #[account(has_one = store)]
    pub executor: AccountLoader<'info, Executor>,
    /// Executor Wallet.
    /// CHECK: `wallet` doesn't have to be a system account, allowing
    /// the instruction to close it.
    #[account(
        mut,
        seeds = [Executor::WALLET_SEED, executor.key().as_ref()],
        bump = executor.load()?.wallet_bump,
    )]
    pub wallet: UncheckedAccount<'info>,
    /// Rent receiver.
    /// CHECK: only used to receive funds.
    #[account(mut)]
    pub rent_receiver: UncheckedAccount<'info>,
    /// Instruction to execute.
    #[account(
        mut,
        has_one = executor,
        has_one = rent_receiver,
        close = rent_receiver,
    )]
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

    // Validate that the approver still have the required role.
    {
        let store = ctx.accounts.store.load()?;
        let approver = instruction
            .header()
            .apporver()
            .ok_or_else(|| error!(CoreError::PreconditionsAreNotMet))?;
        let timelocked_role = roles::timelocked_role(ctx.accounts.executor.load()?.role_name()?);
        require!(
            store.has_role(approver, &timelocked_role)?,
            CoreError::PreconditionsAreNotMet
        );
    }

    let delay = ctx.accounts.timelock_config.load()?.delay();
    require!(
        instruction.header().is_executable(delay)?,
        CoreError::PreconditionsAreNotMet
    );

    let signer = ExecutorWalletSigner::new(
        ctx.accounts.executor.key(),
        ctx.accounts.executor.load()?.wallet_bump,
    );

    invoke_signed(
        &instruction.to_instruction(false).map_err(|err| {
            msg!("Instruction error: {}", err);
            error!(CoreError::InvalidArgument)
        })?,
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

fn validate_timelocked_role<'info>(
    ctx: &Context<impl CpiAuthenticate<'info>>,
    role: &str,
) -> Result<()> {
    let timelocked_role = roles::timelocked_role(role);
    CpiAuthenticate::only(ctx, &timelocked_role)?;
    msg!(
        "[Timelock] approving `{}` instruction by a `{}`",
        role,
        timelocked_role
    );
    Ok(())
}
