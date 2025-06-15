use anchor_lang::prelude::*;
use anchor_spl::token::Mint;
use gmsol_utils::token_config::TokenConfigFlag;

use crate::{
    states::{
        PriceProviderKind, Store, TokenConfigExt, TokenMapAccess, TokenMapAccessMut,
        TokenMapHeader, TokenMapLoader, UpdateTokenConfigParams,
    },
    utils::internal,
    CoreError,
};

/// The accounts definition for [`initialize_token_map`](crate::gmsol_store::initialize_token_map).
///
/// [*See also the documentation for the instruction.*](crate::gmsol_store::initialize_token_map)
#[derive(Accounts)]
pub struct InitializeTokenMap<'info> {
    /// The payer.
    #[account(mut)]
    pub payer: Signer<'info>,
    /// The store account for the token map.
    pub store: AccountLoader<'info, Store>,
    /// The token map account to be initialized.
    #[account(
        init,
        payer = payer,
        space = 8 + TokenMapHeader::space(0),
    )]
    pub token_map: AccountLoader<'info, TokenMapHeader>,
    /// The system program.
    pub system_program: Program<'info, System>,
}

/// Initialize a new token map.
pub(crate) fn initialize_token_map(ctx: Context<InitializeTokenMap>) -> Result<()> {
    ctx.accounts.token_map.load_init()?.store = ctx.accounts.store.key();
    Ok(())
}

/// The accounts definition for [`push_to_token_map`](crate::gmsol_store::push_to_token_map).
#[derive(Accounts)]
pub struct PushToTokenMap<'info> {
    /// The authority of the instruction.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// The store that owns the token map.
    pub store: AccountLoader<'info, Store>,
    /// The token map to push config to.
    #[account(
        mut,
        has_one = store,
    )]
    pub token_map: AccountLoader<'info, TokenMapHeader>,
    /// The token to push config for.
    pub token: Account<'info, Mint>,
    /// The system program.
    pub system_program: Program<'info, System>,
}

/// Push a new token config to the token map.
///
/// ## CHECK
/// - Only [`MARKET_KEEPER`](crate::states::RoleKey::MARKET_KEEPER) can perform this action.
pub(crate) fn unchecked_push_to_token_map(
    ctx: Context<PushToTokenMap>,
    name: &str,
    builder: UpdateTokenConfigParams,
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

/// The accounts definition for
/// [`push_to_token_map_synthetic`](crate::gmsol_store::push_to_token_map_synthetic).
///
/// [*See also the documentation for the instruction.*](crate::gmsol_store::push_to_token_map_synthetic)
#[derive(Accounts)]
pub struct PushToTokenMapSynthetic<'info> {
    /// The authority of the instruction.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// The store that owns the token map.
    pub store: AccountLoader<'info, Store>,
    /// The token map to push config to.
    #[account(mut, has_one = store)]
    pub token_map: AccountLoader<'info, TokenMapHeader>,
    /// The system program.
    pub system_program: Program<'info, System>,
}

/// Push a new synthetic token config to the token map.
///
/// ## CHECK
/// - Only [`MARKET_KEEPER`](crate::states::RoleKey::MARKET_KEEPER) can perform this action.
pub(crate) fn unchecked_push_to_token_map_synthetic(
    ctx: Context<PushToTokenMapSynthetic>,
    name: &str,
    token: Pubkey,
    token_decimals: u8,
    builder: UpdateTokenConfigParams,
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

/// The accounts definition for [`toggle_token_config`](crate::gmsol_store::toggle_token_config).
///
/// [*See also the documentation for the instruction.*](crate::gmsol_store::toggle_token_config)
#[derive(Accounts)]
pub struct ToggleTokenConfig<'info> {
    /// The authority of the instruction.
    pub authority: Signer<'info>,
    /// The store that owns the token map.
    pub store: AccountLoader<'info, Store>,
    /// The token map to update.
    #[account(
        mut,
        has_one = store,
    )]
    pub token_map: AccountLoader<'info, TokenMapHeader>,
}

impl ToggleTokenConfig<'_> {
    /// Toggle the config for the given token.
    ///
    /// ## CHECK
    /// - Only [`MARKET_KEEPER`](crate::states::RoleKey::MARKET_KEEPER) can perform this action.
    pub(crate) fn invoke_unchecked(
        ctx: Context<Self>,
        token: Pubkey,
        flag: TokenConfigFlag,
        enable: bool,
    ) -> Result<()> {
        // Only these flags are permitted to change.
        require!(
            matches!(
                flag,
                TokenConfigFlag::Enabled | TokenConfigFlag::AllowPriceAdjustment
            ),
            CoreError::Internal
        );
        ctx.accounts
            .token_map
            .load_token_map_mut()?
            .get_mut(&token)
            .ok_or_else(|| error!(CoreError::NotFound))?
            .set_flag(flag, enable);
        Ok(())
    }
}

impl<'info> internal::Authentication<'info> for ToggleTokenConfig<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition for [`set_expected_provider`](crate::gmsol_store::set_expected_provider).
///
/// [*See also the documentation for the instruction.*](crate::gmsol_store::set_expected_provider)
#[derive(Accounts)]
pub struct SetExpectedProvider<'info> {
    /// The authority of the instruction.
    pub authority: Signer<'info>,
    /// The store that owns the token map.
    pub store: AccountLoader<'info, Store>,
    /// The token map to update.
    #[account(mut, has_one = store)]
    pub token_map: AccountLoader<'info, TokenMapHeader>,
}

/// Set the expected provider for the given token.
///
/// ## CHECK
/// - Only [`MARKET_KEEPER`](crate::states::RoleKey::MARKET_KEEPER) can perform this action.
pub(crate) fn unchecked_set_expected_provider(
    ctx: Context<SetExpectedProvider>,
    token: Pubkey,
    provider: PriceProviderKind,
) -> Result<()> {
    let mut token_map = ctx.accounts.token_map.load_token_map_mut()?;

    let config = token_map
        .get_mut(&token)
        .ok_or_else(|| error!(CoreError::NotFound))?;

    require_neq!(
        config.expected_provider().map_err(CoreError::from)?,
        provider,
        CoreError::PreconditionsAreNotMet
    );

    config.set_expected_provider(provider);
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

/// The accounts definition for [`set_feed_config_v2`](crate::gmsol_store::set_feed_config_v2).
///
/// [*See also the documentation for the instruction.*](crate::gmsol_store::set_feed_config_v2)
#[derive(Accounts)]
pub struct SetFeedConfig<'info> {
    /// The authority of the instruction.
    pub authority: Signer<'info>,
    /// The store that owns the token map.
    pub store: AccountLoader<'info, Store>,
    /// The token map to update.
    #[account(mut, has_one = store)]
    pub token_map: AccountLoader<'info, TokenMapHeader>,
}

impl SetFeedConfig<'_> {
    /// Set feed config for the given token.
    ///
    /// ## CHECK
    /// - Only [`MARKET_KEEPER`](crate::states::RoleKey::MARKET_KEEPER) can perform this action.
    pub(crate) fn invoke_unchecked(
        ctx: Context<SetFeedConfig>,
        token: Pubkey,
        provider: &PriceProviderKind,
        feed: Option<Pubkey>,
        timestamp_adjustment: Option<u32>,
        max_deviation_factor: Option<u128>,
    ) -> Result<()> {
        require_eq!(
            feed.is_some() || timestamp_adjustment.is_some() || max_deviation_factor.is_some(),
            true,
            {
                msg!("[Set Feed Config] empty update");
                CoreError::InvalidArgument
            }
        );

        let mut map = ctx.accounts.token_map.load_token_map_mut()?;
        let feed_config = map
            .get_mut(&token)
            .ok_or_else(|| error!(CoreError::NotFound))?
            .get_feed_config_mut(provider)
            .map_err(CoreError::from)
            .map_err(|err| error!(err))?;

        let mut new_config = *feed_config;

        if let Some(feed) = feed {
            new_config = new_config.with_feed(feed);
        }

        if let Some(timestamp_adjustment) = timestamp_adjustment {
            new_config = new_config.with_timestamp_adjustment(timestamp_adjustment);
        }

        if let Some(max_deviation_factor) = max_deviation_factor {
            new_config = new_config
                .with_max_deviation_factor(if max_deviation_factor == 0 {
                    None
                } else {
                    Some(max_deviation_factor)
                })
                .map_err(CoreError::from)
                .map_err(|err| error!(err))?;
        }

        *feed_config = new_config;

        Ok(())
    }
}

impl<'info> internal::Authentication<'info> for SetFeedConfig<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition of the instructions to read token map.
#[derive(Accounts)]
pub struct ReadTokenMap<'info> {
    /// Token map.
    pub token_map: AccountLoader<'info, TokenMapHeader>,
}

/// Check if the config of the given token is enabled.
pub(crate) fn is_token_config_enabled(ctx: Context<ReadTokenMap>, token: &Pubkey) -> Result<bool> {
    ctx.accounts
        .token_map
        .load_token_map()?
        .get(token)
        .map(|config| config.is_enabled())
        .ok_or_else(|| error!(CoreError::NotFound))
}

/// Get expected provider for the given token.
pub(crate) fn token_expected_provider(
    ctx: Context<ReadTokenMap>,
    token: &Pubkey,
) -> Result<PriceProviderKind> {
    ctx.accounts
        .token_map
        .load_token_map()?
        .get(token)
        .ok_or_else(|| error!(CoreError::NotFound))?
        .expected_provider()
        .map_err(CoreError::from)
        .map_err(|err| error!(err))
}

/// Get feed address of the price provider of the given token.
pub(crate) fn token_feed(
    ctx: Context<ReadTokenMap>,
    token: &Pubkey,
    provider: &PriceProviderKind,
) -> Result<Pubkey> {
    ctx.accounts
        .token_map
        .load_token_map()?
        .get(token)
        .ok_or_else(|| error!(CoreError::NotFound))?
        .get_feed(provider)
        .map_err(CoreError::from)
        .map_err(|err| error!(err))
}

/// Get timestamp adjustment of the given token.
pub(crate) fn token_timestamp_adjustment(
    ctx: Context<ReadTokenMap>,
    token: &Pubkey,
    provider: &PriceProviderKind,
) -> Result<u32> {
    ctx.accounts
        .token_map
        .load_token_map()?
        .get(token)
        .ok_or_else(|| error!(CoreError::NotFound))?
        .timestamp_adjustment(provider)
        .map_err(CoreError::from)
        .map_err(|err| error!(err))
}

/// Get the name of the given token.
pub(crate) fn token_name(ctx: Context<ReadTokenMap>, token: &Pubkey) -> Result<String> {
    ctx.accounts
        .token_map
        .load_token_map()?
        .get(token)
        .ok_or_else(|| error!(CoreError::NotFound))?
        .name()
        .map(|s| s.to_owned())
        .map_err(CoreError::from)
        .map_err(|err| error!(err))
}

/// Get the decimals of the given token.
pub(crate) fn token_decimals(ctx: Context<ReadTokenMap>, token: &Pubkey) -> Result<u8> {
    Ok(ctx
        .accounts
        .token_map
        .load_token_map()?
        .get(token)
        .ok_or_else(|| error!(CoreError::NotFound))?
        .token_decimals())
}

/// Get the price precision of the given token.
pub(crate) fn token_precision(ctx: Context<ReadTokenMap>, token: &Pubkey) -> Result<u8> {
    Ok(ctx
        .accounts
        .token_map
        .load_token_map()?
        .get(token)
        .ok_or_else(|| error!(CoreError::NotFound))?
        .precision())
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
    builder: UpdateTokenConfigParams,
    enable: bool,
    new: bool,
) -> Result<()> {
    // Note: We have to do the realloc manually because the current implementation of
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
