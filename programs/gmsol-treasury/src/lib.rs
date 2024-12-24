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

declare_id!("GTtRSYha5h8S26kPFHgYKUf8enEgabkTFwW7UToXAHoY");

#[program]
pub mod gmsol_treasury {

    use super::*;

    /// Initialize a treasury [`Config`](crate::states::Config) account.
    pub fn initialize_config(ctx: Context<InitializeConfig>) -> Result<()> {
        instructions::initialize_config(ctx)
    }

    /// Set treasury.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_ADMIN))]
    pub fn set_treasury(ctx: Context<SetTreasury>) -> Result<()> {
        instructions::unchecked_set_treasury(ctx)
    }

    /// Set GT factor.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_ADMIN))]
    pub fn set_gt_factor(ctx: Context<SetGtFactor>, factor: u128) -> Result<()> {
        instructions::unchecked_set_gt_factor(ctx, factor)
    }

    /// Initialize a [`TreasuryConfig`](crate::states::TreasuryConfig) account.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_ADMIN))]
    pub fn initialize_treasury(ctx: Context<InitializeTreasury>, index: u8) -> Result<()> {
        instructions::unchecked_initialize_treasury(ctx, index)
    }

    /// Insert a token to the given [`TreasuryConfig`](crate::states::TreasuryConfig) account.
    ///
    /// # Errors
    /// - The [`token`](InsertTokenToTreasury::token) must not have been inserted.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_ADMIN))]
    pub fn insert_token_to_treasury(ctx: Context<InsertTokenToTreasury>) -> Result<()> {
        instructions::unchecked_insert_token_to_treasury(ctx)
    }

    /// Remove a token from the given [`TreasuryConfig`](crate::states::TreasuryConfig) account.
    ///
    /// # Errors
    /// - The [`token`](RemoveTokenFromTreasury::token) must have been inserted.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_ADMIN))]
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
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_ADMIN))]
    pub fn toggle_token_flag(
        ctx: Context<ToggleTokenFlag>,
        flag: String,
        value: bool,
    ) -> Result<()> {
        instructions::unchecked_toggle_token_flag(ctx, &flag, value)
    }

    /// Deposit into a treasury vault.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_KEEPER))]
    pub fn deposit_into_treasury(ctx: Context<DepositIntoTreasury>) -> Result<()> {
        instructions::unchecked_deposit_into_treasury(ctx)
    }

    /// Withdraw from a treasury vault.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_WITHDRAWER))]
    pub fn withdraw_from_treasury(
        ctx: Context<WithdrawFromTreasury>,
        amount: u64,
        decimals: u8,
    ) -> Result<()> {
        instructions::unchecked_withdraw_from_treasury(ctx, amount, decimals)
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

    /// Set esGT receiver.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_ADMIN))]
    pub fn set_esgt_receiver(ctx: Context<SetEsgtReceiver>) -> Result<()> {
        instructions::unchecked_set_esgt_receiver(ctx)
    }

    /// Claim fees.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_KEEPER))]
    pub fn claim_fees(ctx: Context<ClaimFees>) -> Result<()> {
        instructions::unchecked_claim_fees(ctx)
    }

    /// Prepare GT Bank.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_KEEPER))]
    pub fn prepare_gt_bank(ctx: Context<PrepareGtBank>) -> Result<()> {
        instructions::unchecked_prepare_gt_bank(ctx)
    }

    /// Sync GT Bank.
    #[access_control(CpiAuthenticate::only(&ctx, roles::TREASURY_KEEPER))]
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
