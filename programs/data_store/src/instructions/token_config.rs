use anchor_lang::prelude::*;
use anchor_spl::token::Mint;

use crate::{
    states::{
        FeedConfig, PriceProviderKind, Store, TokenConfigBuilder, TokenMapAccess, TokenMapHeader,
        TokenMapLoader, TokenMapMutAccess,
    },
    utils::internal,
    DataStoreError,
};

#[derive(Accounts)]
pub struct InitializeTokenMap<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    pub store: AccountLoader<'info, Store>,
    #[account(
        init,
        payer = payer,
        space = 8 + TokenMapHeader::space(0),
    )]
    pub token_map: AccountLoader<'info, TokenMapHeader>,
    pub system_program: Program<'info, System>,
}

/// Initialize a new token map.
pub fn initialize_token_map(ctx: Context<InitializeTokenMap>) -> Result<()> {
    ctx.accounts.token_map.load_init()?.store = ctx.accounts.store.key();
    Ok(())
}

#[derive(Accounts)]
pub struct PushToTokenMap<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    pub store: AccountLoader<'info, Store>,
    #[account(
        mut,
        has_one = store,
    )]
    pub token_map: AccountLoader<'info, TokenMapHeader>,
    pub token: Account<'info, Mint>,
    pub system_program: Program<'info, System>,
}

/// Push a new token config to the token map.
///
/// ## CHECK
/// - Only [`MARKET_KEEPER`](crate::states::RoleKey::MARKET_KEEPER) can perform this action.
pub fn unchecked_push_to_token_map(
    ctx: Context<PushToTokenMap>,
    name: &str,
    builder: TokenConfigBuilder,
    enable: bool,
    new: bool,
) -> Result<()> {
    let token = ctx.accounts.token.key();
    let token_decimals = ctx.accounts.token.decimals;
    do_push_token_map(
        ctx.accounts.authority.to_account_info(),
        &ctx.accounts.token_map,
        ctx.accounts.system_program.to_account_info(),
        false,
        name,
        &token,
        token_decimals,
        builder,
        enable,
        new,
    )
}

impl<'info> internal::Authentication<'info> for PushToTokenMap<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

#[derive(Accounts)]
pub struct PushToTokenMapSynthetic<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    pub store: AccountLoader<'info, Store>,
    #[account(mut, has_one = store)]
    pub token_map: AccountLoader<'info, TokenMapHeader>,
    pub system_program: Program<'info, System>,
}

/// Push a new synthetic token config to the token map.
///
/// ## CHECK
/// - Only [`MARKET_KEEPER`](crate::states::RoleKey::MARKET_KEEPER) can perform this action.
pub fn unchecked_push_to_token_map_synthetic(
    ctx: Context<PushToTokenMapSynthetic>,
    name: &str,
    token: Pubkey,
    token_decimals: u8,
    builder: TokenConfigBuilder,
    enable: bool,
    new: bool,
) -> Result<()> {
    do_push_token_map(
        ctx.accounts.authority.to_account_info(),
        &ctx.accounts.token_map,
        ctx.accounts.system_program.to_account_info(),
        true,
        name,
        &token,
        token_decimals,
        builder,
        enable,
        new,
    )
}

impl<'info> internal::Authentication<'info> for PushToTokenMapSynthetic<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

#[derive(Accounts)]
pub struct ToggleTokenConfig<'info> {
    pub authority: Signer<'info>,
    pub store: AccountLoader<'info, Store>,
    #[account(
        mut,
        has_one = store,
    )]
    pub token_map: AccountLoader<'info, TokenMapHeader>,
}

/// Toggle the config for the given token.
///
/// ## CHECK
/// - Only [`MARKET_KEEPER`](crate::states::RoleKey::MARKET_KEEPER) can perform this action.
pub fn unchecked_toggle_token_config(
    ctx: Context<ToggleTokenConfig>,
    token: Pubkey,
    enable: bool,
) -> Result<()> {
    ctx.accounts
        .token_map
        .load_token_map_mut()?
        .get_mut(&token)
        .ok_or(error!(DataStoreError::RequiredResourceNotFound))?
        .set_enabled(enable);
    Ok(())
}

impl<'info> internal::Authentication<'info> for ToggleTokenConfig<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

#[derive(Accounts)]
pub struct SetExpectedProvider<'info> {
    pub authority: Signer<'info>,
    pub store: AccountLoader<'info, Store>,
    #[account(mut, has_one = store)]
    pub token_map: AccountLoader<'info, TokenMapHeader>,
}

/// Set the expected provider for the given token.
///
/// ## CHECK
/// - Only [`MARKET_KEEPER`](crate::states::RoleKey::MARKET_KEEPER) can perform this action.
pub fn unchecked_set_expected_provider(
    ctx: Context<SetExpectedProvider>,
    token: Pubkey,
    provider: PriceProviderKind,
) -> Result<()> {
    ctx.accounts
        .token_map
        .load_token_map_mut()?
        .get_mut(&token)
        .ok_or(error!(DataStoreError::RequiredResourceNotFound))?
        .set_expected_provider(provider);
    Ok(())
}

impl<'info> internal::Authentication<'info> for SetExpectedProvider<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

#[derive(Accounts)]
pub struct SetFeedConfig<'info> {
    pub authority: Signer<'info>,
    pub store: AccountLoader<'info, Store>,
    #[account(mut, has_one = store)]
    pub token_map: AccountLoader<'info, TokenMapHeader>,
}

/// Set feed config for the given token.
///
/// ## CHECK
/// - Only [`MARKET_KEEPER`](crate::states::RoleKey::MARKET_KEEPER) can perform this action.
pub fn unchecked_set_feed_config(
    ctx: Context<SetFeedConfig>,
    token: Pubkey,
    provider: &PriceProviderKind,
    feed: Pubkey,
    timestamp_adjustment: u32,
) -> Result<()> {
    ctx.accounts
        .token_map
        .load_token_map_mut()?
        .get_mut(&token)
        .ok_or(error!(DataStoreError::RequiredResourceNotFound))?
        .set_feed_config(
            provider,
            FeedConfig::new(feed).with_timestamp_adjustment(timestamp_adjustment),
        )
}

impl<'info> internal::Authentication<'info> for SetFeedConfig<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

#[derive(Accounts)]
pub struct ReadTokenMap<'info> {
    token_map: AccountLoader<'info, TokenMapHeader>,
}

/// Check if the config of the given token is enabled.
pub fn is_token_config_enabled(ctx: Context<ReadTokenMap>, token: &Pubkey) -> Result<bool> {
    ctx.accounts
        .token_map
        .load_token_map()?
        .get(token)
        .map(|config| config.is_enabled())
        .ok_or(error!(DataStoreError::RequiredResourceNotFound))
}

/// Get expected provider for the given token.
pub fn token_expected_provider(
    ctx: Context<ReadTokenMap>,
    token: &Pubkey,
) -> Result<PriceProviderKind> {
    ctx.accounts
        .token_map
        .load_token_map()?
        .get(token)
        .ok_or(error!(DataStoreError::RequiredResourceNotFound))?
        .expected_provider()
}

/// Get feed address of the price provider of the given token.
pub fn token_feed(
    ctx: Context<ReadTokenMap>,
    token: &Pubkey,
    provider: &PriceProviderKind,
) -> Result<Pubkey> {
    ctx.accounts
        .token_map
        .load_token_map()?
        .get(token)
        .ok_or(error!(DataStoreError::RequiredResourceNotFound))?
        .get_feed(provider)
}

/// Get timestamp adjustemnt of the given token.
pub fn token_timestamp_adjustment(
    ctx: Context<ReadTokenMap>,
    token: &Pubkey,
    provider: &PriceProviderKind,
) -> Result<u32> {
    ctx.accounts
        .token_map
        .load_token_map()?
        .get(token)
        .ok_or(error!(DataStoreError::RequiredResourceNotFound))?
        .timestamp_adjustment(provider)
}

#[allow(clippy::too_many_arguments)]
fn do_push_token_map<'info>(
    authority: AccountInfo<'info>,
    token_map_loader: &AccountLoader<'info, TokenMapHeader>,
    system_program: AccountInfo<'info>,
    synthetic: bool,
    name: &str,
    token: &Pubkey,
    token_decimals: u8,
    builder: TokenConfigBuilder,
    enable: bool,
    new: bool,
) -> Result<()> {
    // FIXME: We have to do the realloc manually because the current implementation of
    // the `realloc` constraint group will throw an error on the following statement:
    // `realloc = 8 + token_map.load()?.space_after_push()?`.
    // The cause of the error is that the generated code directly inserts the above statement
    // into the realloc method, leading to an `already borrowed` error.
    {
        let new_space = 8 + token_map_loader.load()?.space_after_push()?;
        let current_space = token_map_loader.as_ref().data_len();
        let current_lamports = token_map_loader.as_ref().lamports();
        let new_rent_minimum = Rent::get()?.minimum_balance(new_space);
        // Only realloc when we need more space.
        if new_space > current_space {
            if current_lamports < new_rent_minimum {
                anchor_lang::system_program::transfer(
                    CpiContext::new(
                        system_program,
                        anchor_lang::system_program::Transfer {
                            from: authority,
                            to: token_map_loader.to_account_info(),
                        },
                    ),
                    new_rent_minimum.saturating_sub(current_lamports),
                )?;
            }
            token_map_loader.as_ref().realloc(new_space, false)?;
        }
    }

    let mut token_map = token_map_loader.load_token_map_mut()?;
    token_map.push_with(
        token,
        |config| config.update(name, synthetic, token_decimals, builder, enable, new),
        new,
    )?;
    Ok(())
}
