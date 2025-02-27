/// States.
pub mod states;

/// Instructions.
pub mod instructions;

/// Roles.
pub mod roles;

/// Constants.
pub mod constants;

use anchor_lang::prelude::*;
use gmsol_store::utils::CpiAuthenticate;
use instructions::*;

declare_id!("GTuvYD5SxkTq4FLG6JV1FQ5dkczr1AfgDcBHaFsBdtBg");

#[program]
pub mod gmsol_treasury {

    use super::*;

    /// Initialize a treasury [`Config`](crate::states::Config) account.
    pub fn initialize_config(ctx: Context<InitializeConfig>) -> Result<()> {
        instructions::initialize_config(ctx)
    }

    /// Set treasury vault config.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_ADMIN))]
    pub fn set_treasury_vault_config(ctx: Context<SetTreasuryVaultConfig>) -> Result<()> {
        instructions::unchecked_set_treasury_vault_config(ctx)
    }

    /// Set GT factor.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_ADMIN))]
    pub fn set_gt_factor(ctx: Context<UpdateConfig>, factor: u128) -> Result<()> {
        instructions::unchecked_set_gt_factor(ctx, factor)
    }

    /// Set buyback factor.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_ADMIN))]
    pub fn set_buyback_factor(ctx: Context<UpdateConfig>, factor: u128) -> Result<()> {
        instructions::unchecked_set_buyback_factor(ctx, factor)
    }

    /// Initialize a [`TreasuryVaultConfig`](crate::states::TreasuryVaultConfig) account.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_ADMIN))]
    pub fn initialize_treasury_vault_config(
        ctx: Context<InitializeTreasuryVaultConfig>,
        index: u16,
    ) -> Result<()> {
        instructions::unchecked_initialize_treasury_vault_config(ctx, index)
    }

    /// Insert a token to the given [`TreasuryVaultConfig`](crate::states::TreasuryVaultConfig) account.
    ///
    /// # Errors
    /// - The [`token`](InsertTokenToTreasuryVault::token) must not have been inserted.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_ADMIN))]
    pub fn insert_token_to_treasury_vault(ctx: Context<InsertTokenToTreasuryVault>) -> Result<()> {
        instructions::unchecked_insert_token_to_treasury_vault(ctx)
    }

    /// Remove a token from the given [`TreasuryVaultConfig`](crate::states::TreasuryVaultConfig) account.
    ///
    /// # Errors
    /// - The [`token`](RemoveTokenFromTreasuryVault::token) must have been inserted.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_ADMIN))]
    pub fn remove_token_from_treasury_vault(
        ctx: Context<RemoveTokenFromTreasuryVault>,
    ) -> Result<()> {
        instructions::unchecked_remove_token_from_treasury_vault(ctx)
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
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_ADMIN))]
    pub fn toggle_token_flag(
        ctx: Context<ToggleTokenFlag>,
        flag: String,
        value: bool,
    ) -> Result<()> {
        instructions::unchecked_toggle_token_flag(ctx, &flag, value)
    }

    /// Deposit to treasury vault.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_KEEPER))]
    pub fn deposit_to_treasury_vault(ctx: Context<DepositToTreasuryVault>) -> Result<()> {
        instructions::unchecked_deposit_to_treasury_vault(ctx)
    }

    /// Withdraw from treasury vault.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_WITHDRAWER))]
    pub fn withdraw_from_treasury_vault(
        ctx: Context<WithdrawFromTreasuryVault>,
        amount: u64,
        decimals: u8,
    ) -> Result<()> {
        instructions::unchecked_withdraw_from_treasury_vault(ctx, amount, decimals)
    }

    /// Confirm GT buyback.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_KEEPER))]
    pub fn confirm_gt_buyback<'info>(
        ctx: Context<'_, '_, 'info, 'info, ConfirmGtBuyback<'info>>,
    ) -> Result<()> {
        instructions::unchecked_confirm_gt_buyback(ctx)
    }

    /// Transfer the receiver permission to a new address.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_OWNER))]
    pub fn transfer_receiver(ctx: Context<TransferReceiver>) -> Result<()> {
        instructions::unchecked_transfer_receiver(ctx)
    }

    /// Set referral reward factors.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_ADMIN))]
    pub fn set_referral_reward(ctx: Context<SetReferralReward>, factors: Vec<u128>) -> Result<()> {
        instructions::unchecked_set_referral_reward(ctx, factors)
    }

    /// Claim fees.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_KEEPER))]
    pub fn claim_fees(ctx: Context<ClaimFees>, min_amount: u64) -> Result<()> {
        instructions::unchecked_claim_fees(ctx, min_amount)
    }

    /// Prepare GT Bank.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_KEEPER))]
    pub fn prepare_gt_bank(ctx: Context<PrepareGtBank>) -> Result<()> {
        instructions::unchecked_prepare_gt_bank(ctx)
    }

    /// Sync GT Bank.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_WITHDRAWER))]
    pub fn sync_gt_bank(ctx: Context<SyncGtBank>) -> Result<()> {
        instructions::unchecked_sync_gt_bank(ctx)
    }

    /// Complete GT Exchange.
    pub fn complete_gt_exchange<'info>(
        ctx: Context<'_, '_, 'info, 'info, CompleteGtExchange<'info>>,
    ) -> Result<()> {
        instructions::complete_gt_exchange(ctx)
    }

    /// Create a swap.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_KEEPER))]
    pub fn create_swap<'info>(
        ctx: Context<'_, '_, 'info, 'info, CreateSwap<'info>>,
        nonce: [u8; 32],
        swap_path_length: u8,
        swap_in_amount: u64,
        min_swap_out_amount: Option<u64>,
    ) -> Result<()> {
        instructions::unchecked_create_swap(
            ctx,
            nonce,
            swap_path_length,
            swap_in_amount,
            min_swap_out_amount,
        )
    }

    /// Cancel a swap.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_KEEPER))]
    pub fn cancel_swap(ctx: Context<CancelSwap>) -> Result<()> {
        instructions::unchecked_cancel_swap(ctx)
    }
}
