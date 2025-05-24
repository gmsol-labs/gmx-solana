use anchor_lang::prelude::*;

/// States.
pub mod states;

/// Roles for the timelock program.
pub mod roles;

/// Instructions;
pub mod instructions;

use gmsol_store::{utils::CpiAuthenticate, CoreError};
use instructions::*;

declare_id!("TimeBQ7gQyWyQMD3bTteAdy7hTVDNWSwELdSVZHfSXL");

#[program]
pub mod gmsol_timelock {

    use super::*;

    /// Initialize timelock config.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TIMELOCK_ADMIN))]
    pub fn initialize_config(ctx: Context<InitializeConfig>, delay: u32) -> Result<()> {
        instructions::unchecked_initialize_config(ctx, delay)
    }

    /// Increase timelock delay.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TIMELOCK_ADMIN))]
    pub fn increase_delay(ctx: Context<IncreaseDelay>, delta: u32) -> Result<()> {
        instructions::unchecked_increase_delay(ctx, delta)
    }

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
        signers: Vec<u16>,
    ) -> Result<()> {
        instructions::unchecked_create_instruction_buffer(
            ctx,
            num_accounts,
            data_len,
            &data,
            &signers,
        )
    }

    /// Approve instruction.
    pub fn approve_instruction(ctx: Context<ApproveInstruction>, role: String) -> Result<()> {
        instructions::approve_instruction(ctx, &role)
    }

    /// Approve multiple instructions.
    pub fn approve_instructions<'info>(
        ctx: Context<'_, '_, 'info, 'info, ApproveInstructions<'info>>,
        role: String,
    ) -> Result<()> {
        instructions::approve_instructions(ctx, &role)
    }

    /// Cancel instruction.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TIMELOCK_ADMIN))]
    pub fn cancel_instruction(ctx: Context<CancelInstruction>) -> Result<()> {
        instructions::unchecked_cancel_instruction(ctx)
    }

    /// Cancel multiple instructions that sharing the same `executor` and `rent_receiver`.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TIMELOCK_ADMIN))]
    pub fn cancel_instructions<'info>(
        ctx: Context<'_, '_, 'info, 'info, CancelInstructions<'info>>,
    ) -> Result<()> {
        instructions::unchecked_cancel_instructions(ctx)
    }

    /// Execute instruction.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TIMELOCK_KEEPER))]
    pub fn execute_instruction(ctx: Context<ExecuteInstruction>) -> Result<()> {
        instructions::unchecked_execute_instruction(ctx)
    }

    /// Revoke role.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TIMELOCKED_ADMIN))]
    pub fn revoke_role(ctx: Context<RevokeRole>, role: String) -> Result<()> {
        instructions::unchecked_revoke_role(ctx, role)
    }

    /// Set expected price provider.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TIMELOCKED_MARKET_KEEPER))]
    pub fn set_expected_price_provider(
        ctx: Context<SetExpectedPriceProvider>,
        new_expected_price_provider: u8,
    ) -> Result<()> {
        instructions::unchecked_set_expected_price_provider(
            ctx,
            new_expected_price_provider
                .try_into()
                .map_err(|_| error!(CoreError::InvalidProviderKindIndex))?,
        )
    }
}
