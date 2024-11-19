use std::collections::BTreeSet;

use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_2022::Token2022,
    token_interface::{self, Mint},
};
use gmsol_utils::InitSpace;

use crate::{
    constants,
    states::{glv::Glv, Seed, Store},
    utils::{internal, token::is_associated_token_account_with_program_id},
    CoreError,
};

/// The accounts definitions for [`initialize_glv`](crate::gmsol_store::initialize_glv) instruction.
///
/// Remaining accounts expected by this instruction:
///
///   - 0..N. `[]` N unique market accounts.
///   - N..2N. `[]` N corresponding market token accounts. Should be sorted by addresses.
///   - 2N..3N. `[writable]` N corresponding market token vault accounts,
///     which will be initialized as the associated token accounts of the GLV.
///     This vault accounts should be sorted by market token addresses.
#[derive(Accounts)]
#[instruction(index: u8)]
pub struct InitializeGlv<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// Glv token.
    #[account(
        init,
        payer = authority,
        mint::authority = store,
        mint::decimals = constants::MARKET_TOKEN_DECIMALS,
        seeds = [
            Glv::GLV_TOKEN_SEED,
            store.key().as_ref(),
            &[index],
        ],
        bump,
        owner = token_program.key(),
    )]
    pub glv_token: InterfaceAccount<'info, Mint>,
    /// Glv account.
    #[account(
        init,
        payer = authority,
        space = 8 + Glv::INIT_SPACE,
        seeds = [
            Glv::SEED,
            glv_token.key().as_ref(),
        ],
        bump,
    )]
    pub glv: AccountLoader<'info, Glv>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token2022>,
    pub market_token_program: Interface<'info, token_interface::TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

/// Initialize GLV token and account.
///
/// # CHECK
/// - Only MARKET_KEEPER is allowed to call this function.
pub(crate) fn unchecked_initialize_glv<'info>(
    ctx: Context<'_, '_, 'info, 'info, InitializeGlv<'info>>,
    index: u8,
    length: usize,
) -> Result<()> {
    let remaining_accounts = ctx.remaining_accounts;

    require_gte!(
        Glv::MAX_ALLOWED_NUMBER_OF_MARKETS,
        length,
        CoreError::ExceedMaxLengthLimit
    );

    require_gt!(length, 0, CoreError::InvalidArgument);

    let markets_end = length;
    let market_tokens_end = markets_end + length;
    let vaults_end = market_tokens_end + length;

    require_gte!(
        remaining_accounts.len(),
        vaults_end,
        ErrorCode::AccountNotEnoughKeys
    );

    let markets = &remaining_accounts[0..markets_end];
    let market_tokens = &remaining_accounts[markets_end..market_tokens_end];
    let vaults = &remaining_accounts[market_tokens_end..vaults_end];

    let store = ctx.accounts.store.key();
    let (long_token, short_token, expected_market_tokens) =
        Glv::process_and_validate_markets_for_init(markets, &store)?;

    ctx.accounts
        .initialize_vaults(&expected_market_tokens, market_tokens, vaults)?;

    ctx.accounts.glv.load_init()?.unchecked_init(
        ctx.bumps.glv,
        index,
        &store,
        &ctx.accounts.glv_token.key(),
        &long_token,
        &short_token,
        &expected_market_tokens,
    )?;

    Ok(())
}

impl<'info> internal::Authentication<'info> for InitializeGlv<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

impl<'info> InitializeGlv<'info> {
    fn initialize_vaults(
        &self,
        expected_market_tokens: &BTreeSet<Pubkey>,
        market_tokens: &'info [AccountInfo<'info>],
        vaults: &'info [AccountInfo<'info>],
    ) -> Result<()> {
        use anchor_spl::associated_token::{create, Create};

        require_eq!(
            expected_market_tokens.len(),
            vaults.len(),
            CoreError::Internal
        );

        require_eq!(
            expected_market_tokens.len(),
            market_tokens.len(),
            CoreError::Internal
        );

        let glv = self.glv.key();
        let token_program_id = self.market_token_program.key;

        for ((expected_market_token, market_token), vault) in
            expected_market_tokens.iter().zip(market_tokens).zip(vaults)
        {
            require!(
                is_associated_token_account_with_program_id(
                    vault.key,
                    &glv,
                    expected_market_token,
                    token_program_id,
                ),
                ErrorCode::AccountNotAssociatedTokenAccount
            );

            create(CpiContext::new(
                self.associated_token_program.to_account_info(),
                Create {
                    payer: self.authority.to_account_info(),
                    associated_token: vault.clone(),
                    authority: self.glv.to_account_info(),
                    mint: market_token.clone(),
                    system_program: self.system_program.to_account_info(),
                    token_program: self.market_token_program.to_account_info(),
                },
            ))?;
        }

        Ok(())
    }
}

/// The accounts definition for [`update_glv_market_config`](crate::gmsol_store::update_glv_market_config) instruction.
#[derive(Accounts)]
pub struct UpdateGlvMarketConfig<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// GLV.
    #[account(
        mut,
        has_one = store,
        constraint = glv.load()?.contains(&market_token.key()) @ CoreError::InvalidArgument,
    )]
    pub glv: AccountLoader<'info, Glv>,
    /// Market token.
    pub market_token: Box<Account<'info, anchor_spl::token::Mint>>,
}

/// Update the config for the given market.
///
/// # CHECK
/// - Only MARKET_KEEPER is allowed to call this function.
pub fn unchecked_update_glv_market_config(
    ctx: Context<UpdateGlvMarketConfig>,
    max_amount: Option<u64>,
    max_value: Option<u128>,
) -> Result<()> {
    require!(
        max_amount.is_some() || max_value.is_some(),
        CoreError::InvalidArgument
    );
    let mut glv = ctx.accounts.glv.load_mut()?;
    glv.update_market_config(&ctx.accounts.market_token.key(), max_amount, max_value)?;
    Ok(())
}

impl<'info> internal::Authentication<'info> for UpdateGlvMarketConfig<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}
