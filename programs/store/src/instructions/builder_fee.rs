use anchor_lang::prelude::*;
use anchor_spl::token_interface::Mint;

use crate::{
    states::{builder_fee::BuilderFeeTokenController, Seed, Store},
    utils::internal,
};

use gmsol_utils::InitSpace;

/// The accounts definition for
/// [`initialize_builder_fee_token_controller`](crate::gmsol_store::initialize_builder_fee_token_controller).
#[derive(Accounts)]
pub struct InitializeBuilderFeeTokenController<'info> {
    /// The caller.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// The token mint to initialize the controller for.
    pub token_mint: InterfaceAccount<'info, Mint>,
    /// The controller account to initialize.
    #[account(
        init,
        payer = authority,
        space = 8 + BuilderFeeTokenController::INIT_SPACE,
        seeds = [
            BuilderFeeTokenController::SEED,
            store.key().as_ref(),
            token_mint.key().as_ref(),
        ],
        bump,
    )]
    pub controller: AccountLoader<'info, BuilderFeeTokenController>,
    /// System program.
    pub system_program: Program<'info, System>,
}

/// Initialize the per-token access control account for builder fees.
///
/// ## CHECK
/// - Only MARKET_KEEPER can initialize the controller.
pub(crate) fn unchecked_initialize_builder_fee_token_controller(
    ctx: Context<InitializeBuilderFeeTokenController>,
) -> Result<()> {
    ctx.accounts.controller.load_init()?.init(
        &ctx.accounts.store.key(),
        &ctx.accounts.token_mint.key(),
        ctx.bumps.controller,
    );
    Ok(())
}

impl<'info> internal::Authentication<'info> for InitializeBuilderFeeTokenController<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}
