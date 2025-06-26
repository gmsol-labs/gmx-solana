use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
    token_interface,
};

use crate::{
    constants,
    states::Store,
    utils::{internal, token::must_be_uninitialized},
};

/// The accounts definition for [`initialize_market_vault`](crate::gmsol_store::initialize_market_vault).
///
/// *[See also the documentation for the instruction.](crate::gmsol_store::initialize_market_vault)*
#[derive(Accounts)]
pub struct InitializeMarketVault<'info> {
    /// The caller.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// Token mint.
    pub mint: Account<'info, Mint>,
    /// The vault to create.
    #[account(
        init_if_needed,
        payer = authority,
        token::mint = mint,
        // We use the store as the authority of the token account.
        token::authority = store,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            mint.key().as_ref(),
        ],
        bump,
    )]
    pub vault: Account<'info, TokenAccount>,
    /// System Program.
    pub system_program: Program<'info, System>,
    /// Token Program.
    pub token_program: Program<'info, Token>,
}

/// Initialize a vault of the given token for a market.
/// The address is derived from token mint addresses (the `market_token_mint` seed is optional).
///
/// ## CHECK
/// - Only MARKET_KEEPER can initialize market vault.
#[allow(unused_variables)]
pub(crate) fn unchecked_initialize_market_vault(ctx: Context<InitializeMarketVault>) -> Result<()> {
    Ok(())
}

impl<'info> internal::Authentication<'info> for InitializeMarketVault<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition for [`use_claimable_account`](crate::gmsol_store::use_claimable_account).
///
/// *[See also the documentation for the instruction.](crate::gmsol_store::use_claimable_account)*
#[derive(Accounts)]
#[instruction(timestamp: i64)]
pub struct UseClaimableAccount<'info> {
    /// The caller.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// Mint.
    pub mint: Account<'info, Mint>,
    /// Owner.
    /// CHECK: check by CPI.
    pub owner: UncheckedAccount<'info>,
    /// The claimable account.
    #[account(
        init_if_needed,
        payer = authority,
        token::mint = mint,
        // We use the store as the authority of the token account.
        token::authority = store,
        seeds = [
            constants::CLAIMABLE_ACCOUNT_SEED,
            store.key().as_ref(),
            mint.key().as_ref(),
            owner.key().as_ref(),
            &store.load()?.claimable_time_key(timestamp)?,
        ],
        bump,
    )]
    pub account: Account<'info, TokenAccount>,
    /// System Program.
    pub system_program: Program<'info, System>,
    /// Token Program.
    pub token_program: Program<'info, Token>,
}

/// Prepare claimable account.
///
/// ## CHECK
/// - Only ORDER_KEEPER can use claimable account.
pub(crate) fn unchecked_use_claimable_account(
    ctx: Context<UseClaimableAccount>,
    _timestamp: i64,
    amount: u64,
) -> Result<()> {
    if ctx.accounts.account.delegate.is_none() || ctx.accounts.account.delegated_amount != amount {
        anchor_spl::token::approve(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                anchor_spl::token::Approve {
                    to: ctx.accounts.account.to_account_info(),
                    delegate: ctx.accounts.owner.to_account_info(),
                    authority: ctx.accounts.store.to_account_info(),
                },
                &[&ctx.accounts.store.load()?.signer_seeds()],
            ),
            amount,
        )?;
    }
    Ok(())
}

impl<'info> internal::Authentication<'info> for UseClaimableAccount<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition for [`close_empty_claimable_account`](crate::gmsol_store::close_empty_claimable_account).
///
/// *[See also the documentation for the instruction.](crate::gmsol_store::close_empty_claimable_account)*
#[derive(Accounts)]
#[instruction(timestamp: i64)]
pub struct CloseEmptyClaimableAccount<'info> {
    /// The caller.
    #[account(mut)]
    pub authority: Signer<'info>,
    pub store: AccountLoader<'info, Store>,
    pub mint: Account<'info, Mint>,
    /// CHECK: only use to reference the owner.
    pub owner: UncheckedAccount<'info>,
    /// CHECK: will be checked during the execution.
    #[account(
        mut,
        seeds = [
            constants::CLAIMABLE_ACCOUNT_SEED,
            store.key().as_ref(),
            mint.key().as_ref(),
            owner.key().as_ref(),
            &store.load()?.claimable_time_key(timestamp)?,
        ],
        bump,
    )]
    pub account: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
}

/// Close claimable account if it is empty.
///
/// ## CHECK
/// - Only ORDER_KEEPER can close claimable account.
pub(crate) fn unchecked_close_empty_claimable_account(
    ctx: Context<CloseEmptyClaimableAccount>,
    _timestamp: i64,
) -> Result<()> {
    if must_be_uninitialized(&ctx.accounts.account) {
        return Ok(());
    }
    let account = ctx.accounts.account.to_account_info();
    let amount = anchor_spl::token::accessor::amount(&account)?;
    if amount == 0 {
        anchor_spl::token::close_account(CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            anchor_spl::token::CloseAccount {
                account: ctx.accounts.account.to_account_info(),
                destination: ctx.accounts.authority.to_account_info(),
                authority: ctx.accounts.store.to_account_info(),
            },
            &[&ctx.accounts.store.load()?.signer_seeds()],
        ))?;
    }
    Ok(())
}

impl<'info> internal::Authentication<'info> for CloseEmptyClaimableAccount<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition for [`prepare_associated_token_account`](crate::gmsol_store::prepare_associated_token_account).
///
/// *[See also the documentation for the instruction.](crate::gmsol_store::prepare_associated_token_account)*
#[derive(Accounts)]
pub struct PrepareAssociatedTokenAccount<'info> {
    /// The payer.
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: only use as the owner of the token account.
    pub owner: UncheckedAccount<'info>,
    /// The mint account for the token account.
    pub mint: InterfaceAccount<'info, token_interface::Mint>,
    /// The token account to prepare.
    #[account(
        init_if_needed,
        payer = payer,
        associated_token::mint = mint,
        associated_token::authority = owner,
    )]
    pub account: InterfaceAccount<'info, token_interface::TokenAccount>,
    /// The [`System`] program.
    pub system_program: Program<'info, System>,
    /// The [`Token`] program.
    pub token_program: Interface<'info, token_interface::TokenInterface>,
    /// The [`AssociatedToken`] program.
    pub associated_token_program: Program<'info, AssociatedToken>,
}

pub(crate) fn prepare_associated_token_account(
    _ctx: Context<PrepareAssociatedTokenAccount>,
) -> Result<()> {
    Ok(())
}

/// The accounts definition for [`create_token_metadata`](crate::gmsol_store::create_token_metadata).
#[derive(Accounts)]
pub struct CreateTokenMetadata<'info> {
    /// Authority account.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store account.
    pub store: AccountLoader<'info, Store>,
    /// The mint account for the token account.
    #[account(mut)]
    pub mint: InterfaceAccount<'info, token_interface::Mint>,
    /// The metadata account to create.
    /// CHECK: check by CPI.
    #[account(mut)]
    pub metadata: UncheckedAccount<'info>,
    /// The [`System`] program.
    pub system_program: Program<'info, System>,
    /// The sysvar instructions.
    /// CHECK: check by CPI.
    pub sysvar_instructions: UncheckedAccount<'info>,
    /// The metadata program.
    pub metadata_program: Program<'info, anchor_spl::metadata::Metadata>,
}

impl CreateTokenMetadata<'_> {
    /// Create token metadata for a token whose mint authority is `store`.
    ///
    /// # CHECK
    /// - Only MARKET_KEEPER is allowed to invoke.
    pub(crate) fn invoke_unchecked(
        ctx: Context<Self>,
        name: String,
        symbol: String,
        uri: String,
    ) -> Result<()> {
        ctx.accounts.create_token_metadata(name, symbol, uri)?;
        Ok(())
    }

    fn create_token_metadata(&self, name: String, symbol: String, uri: String) -> Result<()> {
        use mpl_token_metadata::{instructions::CreateV1CpiBuilder, types::TokenStandard};

        let store = self.store.load()?;
        let signer_seeds = store.signer_seeds();

        CreateV1CpiBuilder::new(&self.metadata_program)
            .metadata(&self.metadata)
            .mint(self.mint.as_ref(), false)
            .authority(self.store.as_ref())
            .payer(&self.authority)
            .update_authority(self.store.as_ref(), true)
            .system_program(&self.system_program)
            .sysvar_instructions(&self.sysvar_instructions)
            .name(name)
            .symbol(symbol)
            .uri(uri)
            .seller_fee_basis_points(0)
            .token_standard(TokenStandard::Fungible)
            .invoke_signed(&[&signer_seeds])?;

        Ok(())
    }
}

impl<'info> internal::Authentication<'info> for CreateTokenMetadata<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition for [`update_token_metadata`](crate::gmsol_store::update_token_metadata).
#[derive(Accounts)]
pub struct UpdateTokenMetadata<'info> {
    /// Authority account.
    pub authority: Signer<'info>,
    /// Store account.
    pub store: AccountLoader<'info, Store>,
    /// The metadata account to create.
    /// CHECK: check by CPI.
    #[account(mut)]
    pub metadata: UncheckedAccount<'info>,
    /// The metadata program.
    pub metadata_program: Program<'info, anchor_spl::metadata::Metadata>,
}

impl UpdateTokenMetadata<'_> {
    /// Update a token metadata whose update authority is `store`.
    ///
    /// # CHECK
    /// - Only MARKET_KEEPER is allowed to invoke.
    pub(crate) fn invoke_unchecked(
        ctx: Context<Self>,
        name: String,
        symbol: String,
        uri: String,
    ) -> Result<()> {
        use anchor_spl::metadata::mpl_token_metadata::types::DataV2;

        ctx.accounts.update_token_metadata(DataV2 {
            name,
            symbol,
            uri,
            seller_fee_basis_points: 0,
            creators: None,
            collection: None,
            uses: None,
        })?;
        Ok(())
    }

    fn update_token_metadata(
        &self,
        data: anchor_spl::metadata::mpl_token_metadata::types::DataV2,
    ) -> Result<()> {
        use anchor_spl::metadata;

        let store = self.store.load()?;
        let signer_seeds = store.signer_seeds();

        let ctx = CpiContext::new(
            self.metadata_program.to_account_info(),
            metadata::UpdateMetadataAccountsV2 {
                metadata: self.metadata.to_account_info(),
                update_authority: self.store.to_account_info(),
            },
        );

        metadata::update_metadata_accounts_v2(
            ctx.with_signer(&[&signer_seeds]),
            None,
            Some(data),
            None,
            None,
        )?;
        Ok(())
    }
}

impl<'info> internal::Authentication<'info> for UpdateTokenMetadata<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}
