use anchor_lang::prelude::*;

/// States.
pub mod states;

/// Roles for the timelock program.
pub mod roles;

/// Instructions;
pub mod instructions;

use gmsol_store::utils::CpiAuthenticate;
use instructions::*;

declare_id!("timedreYasWZUyAgofdmjFVJwk3LKZZq6QJtgpc1aqv");

#[program]
pub mod gmsol_timelock {
    use super::*;

    /// Initialize executor.
    pub fn initialize_executor(ctx: Context<InitializeExecutor>, role: String) -> Result<()> {
        instructions::initialize_executor(ctx, &role)
    }

    /// Create instruction buffer.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TIMELOCK_KEEPER))]
    pub fn create_instruction_buffer<'info>(
        ctx: Context<'_, '_, 'info, 'info, CreateInstructionBuffer<'info>>,
        num_accounts: u16,
        data_len: u16,
        data: Vec<u8>,
    ) -> Result<()> {
        instructions::unchecked_create_instruction_buffer(ctx, num_accounts, data_len, &data)
    }

    /// Approve instruction.
    pub fn approve_instruction(ctx: Context<ApproveInstruction>, role: String) -> Result<()> {
        instructions::approve_instruction(ctx, &role)
    }

    /// Cancel instruction.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TIMELOCK_ADMIN))]
    pub fn cancel_instruction(ctx: Context<CancelInstruction>) -> Result<()> {
        instructions::unchecked_cancel_instruction(ctx)
    }

    /// Execute instruction.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TIMELOCK_ADMIN))]
    pub fn execute_instruction(ctx: Context<ExecuteInstruction>) -> Result<()> {
        instructions::unchecked_execute_instruction(ctx)
    }
}
