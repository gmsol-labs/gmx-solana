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
    states::{
        glv::{Glv, UpdateGlvParams},
        Market, Seed, Store,
    },
    utils::{internal, token::is_associated_token_account_with_program_id},
    CoreError,
};

/// The accounts definition for [`initialize_glv`](crate::gmsol_store::initialize_glv) instruction.
///
/// Remaining accounts expected by this instruction:
///
///   - 0..N. `[]` N unique market accounts.
///   - N..2N. `[]` N corresponding market token accounts. Should be sorted by addresses.
///   - 2N..3N. `[writable]` N corresponding market token vault accounts,
///     which will be initialized as the associated token accounts of the GLV.
///     This vault accounts should be sorted by market token addresses.
#[derive(Accounts)]
#[instruction(index: u16)]
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
            &index.to_le_bytes(),
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
    index: u16,
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
        use anchor_spl::associated_token::{create_idempotent, Create};

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

            create_idempotent(CpiContext::new(
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

impl<'info> internal::Authentication<'info> for UpdateGlvMarketConfig<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// Update the config for the given market.
///
/// # CHECK
/// - Only MARKET_KEEPER is allowed to call this function.
pub(crate) fn unchecked_update_glv_market_config(
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

/// Toggle flag of the given market.
///
/// # CHECK
/// - Only MARKET_KEEPER is allowed to call this function.
pub(crate) fn unchecked_toggle_glv_market_flag(
    ctx: Context<UpdateGlvMarketConfig>,
    flag: &str,
    enable: bool,
) -> Result<()> {
    let flag = flag
        .parse()
        .map_err(|_| error!(CoreError::InvalidArgument))?;
    let mut glv = ctx.accounts.glv.load_mut()?;
    let market_token = ctx.accounts.market_token.key();
    let previous = glv.toggle_market_config_flag(&market_token, flag, enable)?;
    msg!(
        "[GLV] toggled market flag {}: {} -> {}",
        flag,
        previous,
        enable
    );
    Ok(())
}

/// The accounts definition for [`update_glv_config`](crate::gmsol_store::update_glv_config) instruction.
#[derive(Accounts)]
pub struct UpdateGlvConfig<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// GLV to update.
    #[account(mut, has_one = store)]
    pub glv: AccountLoader<'info, Glv>,
}

/// Update thte config of GLV.
///
/// # CHECK
/// - Only MARKET_KEEPER can use.
pub(crate) fn unchecked_update_glv(
    ctx: Context<UpdateGlvConfig>,
    params: &UpdateGlvParams,
) -> Result<()> {
    params.validate()?;
    ctx.accounts.glv.load_mut()?.update_config(params)?;
    Ok(())
}

impl<'info> internal::Authentication<'info> for UpdateGlvConfig<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition for [`insert_glv_market`](crate::gmsol_store::insert_glv_market) instruction.
#[derive(Accounts)]
pub struct InsertGlvMarket<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// GLV to modify.
    #[account(
        mut,
        has_one = store,
    )]
    pub glv: AccountLoader<'info, Glv>,
    /// Market token.
    #[account(
        mint::authority = store,
        mint::token_program = token_program,
        constraint = !glv.load()?.contains(&market_token.key()) @ CoreError::PreconditionsAreNotMet,
    )]
    pub market_token: InterfaceAccount<'info, token_interface::Mint>,
    /// Market.
    #[account(
        has_one = store,
        constraint = market.load()?.meta().market_token_mint == market_token.key() @ CoreError::MarketTokenMintMismatched,
    )]
    pub market: AccountLoader<'info, Market>,
    /// Vault.
    #[account(
        init_if_needed,
        payer = authority,
        associated_token::mint = market_token,
        associated_token::authority = glv,
        associated_token::token_program = token_program,
    )]
    pub vault: InterfaceAccount<'info, token_interface::TokenAccount>,
    /// System program.
    pub system_program: Program<'info, System>,
    /// Token program for market token.
    pub token_program: Interface<'info, token_interface::TokenInterface>,
    /// Associated token program.
    pub associated_token_program: Program<'info, AssociatedToken>,
}

/// Insert a new market to the GLV.
///
/// # CHECK
/// - Only MARKET_KEEPER can use.
pub(crate) fn unchecked_insert_glv_market(ctx: Context<InsertGlvMarket>) -> Result<()> {
    let mut glv = ctx.accounts.glv.load_mut()?;
    let market = ctx.accounts.market.load()?;

    glv.insert_market(&ctx.accounts.store.key(), &market)?;

    Ok(())
}

impl<'info> internal::Authentication<'info> for InsertGlvMarket<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition for [`remove_glv_market`](crate::gmsol_store::remove_glv_market) instruction.
#[derive(Accounts)]
pub struct RemoveGlvMarket<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// The store wallet.
    #[account(mut, seeds = [Store::WALLET_SEED, store.key().as_ref()], bump)]
    pub store_wallet: SystemAccount<'info>,
    /// GLV to modify.
    #[account(
        mut,
        has_one = store,
    )]
    pub glv: AccountLoader<'info, Glv>,
    /// Market token.
    #[account(
        mint::authority = store,
        constraint = glv.load()?.contains(&market_token.key()) @ CoreError::PreconditionsAreNotMet,
    )]
    pub market_token: InterfaceAccount<'info, token_interface::Mint>,
    /// Vault.
    #[account(
        mut,
        associated_token::mint = market_token,
        associated_token::authority = glv,
    )]
    pub vault: InterfaceAccount<'info, token_interface::TokenAccount>,
    /// Store wallet ATA.
    #[account(
        init_if_needed,
        payer = authority,
        associated_token::mint = market_token,
        associated_token::authority = store_wallet,
    )]
    pub store_wallet_ata: InterfaceAccount<'info, token_interface::TokenAccount>,
    /// Token program.
    pub token_program: Interface<'info, token_interface::TokenInterface>,
    /// Associated token program.
    pub associated_token_program: Program<'info, AssociatedToken>,
    /// System program.
    pub system_program: Program<'info, System>,
}

/// Remove a new market from the GLV.
///
/// # CHECK
/// - Only MARKET_KEEPER can use.
pub(crate) fn unchecked_remove_glv_market(ctx: Context<RemoveGlvMarket>) -> Result<()> {
    use anchor_spl::token_interface::{close_account, CloseAccount};

    let market_token = ctx.accounts.market_token.key();
    require_eq!(
        ctx.accounts
            .glv
            .load()?
            .market_config(&market_token)
            .ok_or_else(|| error!(CoreError::NotFound))?
            .balance(),
        0,
        CoreError::PreconditionsAreNotMet
    );
    ctx.accounts
        .glv
        .load_mut()?
        .unchecked_remove_market(&market_token)?;

    {
        let glv = ctx.accounts.glv.load()?;
        let signer = glv.signer_seeds();

        let amount = ctx.accounts.vault.amount;

        if amount != 0 {
            let decimals = ctx.accounts.market_token.decimals;
            token_interface::transfer_checked(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    token_interface::TransferChecked {
                        from: ctx.accounts.vault.to_account_info(),
                        mint: ctx.accounts.market_token.to_account_info(),
                        to: ctx.accounts.store_wallet_ata.to_account_info(),
                        authority: ctx.accounts.glv.to_account_info(),
                    },
                )
                .with_signer(&[&signer]),
                amount,
                decimals,
            )?;
        }

        close_account(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                CloseAccount {
                    account: ctx.accounts.vault.to_account_info(),
                    destination: ctx.accounts.store_wallet.to_account_info(),
                    authority: ctx.accounts.glv.to_account_info(),
                },
            )
            .with_signer(&[&signer]),
        )?;
    }

    Ok(())
}

impl<'info> internal::Authentication<'info> for RemoveGlvMarket<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}
