/// States.
pub mod states;

/// Instructions.
pub mod instructions;

/// Roles.
pub mod roles;

use anchor_lang::prelude::*;
use gmsol_store::utils::CpiAuthenticate;
use instructions::*;

declare_id!("GTtRSYha5h8S26kPFHgYKUf8enEgabkTFwW7UToXAHoY");

#[program]
pub mod gmsol_treasury {
    use super::*;

    /// Initialize a treasury [`Config`](crate::states::Config) account.
    pub fn initialize_config(ctx: Context<InitializeConfig>) -> Result<()> {
        instructions::initialize_config(ctx)
    }

    /// Set treasury.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_OWNER))]
    pub fn set_treasury(ctx: Context<SetTreasury>) -> Result<()> {
        instructions::unchecked_set_treasury(ctx)
    }

    /// Initialize a [`Treasury`](crate::states::Treasury) account.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_OWNER))]
    pub fn initialize_treasury(ctx: Context<InitializeTreasury>) -> Result<()> {
        instructions::unchecked_initialize_treasury(ctx)
    }

    /// Insert a token to the given [`Treasury`](crate::states::Treasury) account.
    ///
    /// # Errors
    /// - The [`token`](InsertTokenToTreasury::token) must not have been inserted.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_OWNER))]
    pub fn insert_token_to_treasury(ctx: Context<InsertTokenToTreasury>) -> Result<()> {
        instructions::unchecked_insert_token_to_treasury(ctx)
    }

    /// Remove a token from the given [`Treasury`](crate::states::Treasury) account.
    ///
    /// # Errors
    /// - The [`token`](RemoveTokenFromTreasury::token) must have been inserted.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_OWNER))]
    pub fn remove_token_from_treasury(ctx: Context<RemoveTokenFromTreasury>) -> Result<()> {
        instructions::unchecked_remove_token_from_treasury(ctx)
    }

    /// Toggle a flag of the given token.
    ///
    /// # Arguments
    /// - `flag`: the flag to toggle.
    /// - `value`: the value to be changed to.
    ///
    /// # Errors.
    /// - The [`token`](ToggleTokenFlag::token) must be in the token list.
    /// - `flag` must be defined in [`TokenFlag`](crate::states::treasury::TokenFlag).
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_OWNER))]
    pub fn toggle_token_flag(
        ctx: Context<ToggleTokenFlag>,
        flag: String,
        value: bool,
    ) -> Result<()> {
        instructions::unchecked_toggle_token_flag(ctx, &flag, value)
    }
}
