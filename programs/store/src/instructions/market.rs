use crate::{
    events::EventEmitter,
    ops::market::MarketTransferOutOperation,
    states::{
        market::{
            revertible::{Revertible, RevertibleMarket},
            status::MarketStatus,
            utils::ValidateMarketBalances,
        },
        Factor, HasMarketMeta,
    },
    ModelError,
};

use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};
use gmsol_model::{
    num::Unsigned, price::Prices, BalanceExt, Bank, BaseMarketMut, LiquidityMarketExt,
    PnlFactorKind, PoolExt,
};
use gmsol_utils::InitSpace;

use crate::{
    constants,
    states::{
        market::config::{EntryArgs, MarketConfigBuffer},
        Market, Seed, Store, TokenMapAccess, TokenMapHeader, TokenMapLoader,
    },
    utils::internal,
    CoreError,
};

/// The accounts definition for [`initialize_market`](crate::gmsol_store::initialize_market).
///
/// *[See also the documentation for the instruction.](crate::gmsol_store::initialize_market)*
#[derive(Accounts)]
#[instruction(index_token_mint: Pubkey)]
pub struct InitializeMarket<'info> {
    /// The address authorized to execute this instruction.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// The store account.
    #[account(has_one = token_map)]
    pub store: AccountLoader<'info, Store>,
    /// Market token mint.
    #[account(
        init,
        payer = authority,
        mint::decimals = constants::MARKET_TOKEN_DECIMALS,
        // We directly use the store as the authority.
        mint::authority = store.key(),
        seeds = [
            constants::MAREKT_TOKEN_MINT_SEED,
            store.key().as_ref(),
            index_token_mint.as_ref(),
            long_token_mint.key().as_ref(),
            short_token_mint.key().as_ref(),
        ],
        bump,
    )]
    pub market_token_mint: Account<'info, Mint>,
    /// Long token.
    pub long_token_mint: Account<'info, Mint>,
    /// Short token.
    pub short_token_mint: Account<'info, Mint>,
    /// The market account.
    #[account(
        init,
        payer = authority,
        space = 8 + Market::INIT_SPACE,
        seeds = [
            Market::SEED,
            store.key().as_ref(),
            market_token_mint.key().as_ref(),
        ],
        bump,
    )]
    pub market: AccountLoader<'info, Market>,
    /// The token map account.
    #[account(has_one = store)]
    pub token_map: AccountLoader<'info, TokenMapHeader>,
    /// Long token vault must exist.
    #[account(
        token::mint = long_token_mint,
        // We use the store as the authority of the token account.
        token::authority = store,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            long_token_mint.key().as_ref(),
        ],
        bump,
    )]
    pub long_token_vault: Account<'info, TokenAccount>,
    /// Short token vault must exist.
    #[account(
        token::mint = short_token_mint,
        // We use the store as the authority of the token account.
        token::authority = store,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            short_token_mint.key().as_ref(),
        ],
        bump,
    )]
    pub short_token_vault: Account<'info, TokenAccount>,
    /// The system program.
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
}

/// Initialize the account for [`Market`].
///
/// ## CHECK
/// - Only MARKET_KEEPER can create new market.
pub(crate) fn unchecked_initialize_market(
    ctx: Context<InitializeMarket>,
    index_token_mint: Pubkey,
    name: &str,
    enable: bool,
) -> Result<()> {
    {
        let token_map = ctx.accounts.token_map.load_token_map()?;
        require!(
            token_map
                .get(&index_token_mint)
                .ok_or_else(|| error!(CoreError::NotFound))?
                .is_enabled(),
            CoreError::InvalidArgument
        );

        let long_token = &ctx.accounts.long_token_mint;
        let long_token_config = token_map
            .get(&long_token.key())
            .ok_or_else(|| error!(CoreError::NotFound))?;
        require!(
            long_token_config.is_enabled(),
            CoreError::TokenConfigDisabled
        );
        require!(
            long_token_config.is_valid_pool_token_config(),
            CoreError::InvalidArgument
        );
        // This is a redundant check to prevent the decimals in the token config from
        // being inconsistent with the actual values.
        require_eq!(
            long_token_config.token_decimals(),
            long_token.decimals,
            CoreError::TokenDecimalsMismatched
        );

        let short_token = &ctx.accounts.short_token_mint;
        let short_token_config = token_map
            .get(&short_token.key())
            .ok_or_else(|| error!(CoreError::NotFound))?;
        require!(
            short_token_config.is_enabled(),
            CoreError::TokenConfigDisabled
        );
        require!(
            short_token_config.is_valid_pool_token_config(),
            CoreError::InvalidArgument
        );
        // This is a redundant check to prevent the decimals in the token config from
        // being inconsistent with the actual values.
        require_eq!(
            short_token_config.token_decimals(),
            short_token.decimals,
            CoreError::TokenDecimalsMismatched
        );
    }
    let market = &ctx.accounts.market;
    market.load_init()?.init(
        ctx.bumps.market,
        ctx.accounts.store.key(),
        name,
        ctx.accounts.market_token_mint.key(),
        index_token_mint,
        ctx.accounts.long_token_mint.key(),
        ctx.accounts.short_token_mint.key(),
        enable,
    )?;
    Ok(())
}

impl<'info> internal::Authentication<'info> for InitializeMarket<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition for [`toggle_market`](crate::gmsol_store::toggle_market).
///
/// *[See also the documentation for the instruction.](crate::gmsol_store::toggle_market)*
#[derive(Accounts)]
pub struct ToggleMarket<'info> {
    /// The caller.
    pub authority: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// Market.
    #[account(mut, has_one = store)]
    pub market: AccountLoader<'info, Market>,
}

/// Toggle Market.
///
/// ## CHECK
/// - Only MARKET_KEEPER can toggle market.
pub(crate) fn unchecked_toggle_market(ctx: Context<ToggleMarket>, enable: bool) -> Result<()> {
    ctx.accounts.market.load_mut()?.set_enabled(enable);
    Ok(())
}

impl<'info> internal::Authentication<'info> for ToggleMarket<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition for [`market_transfer_in`](crate::gmsol_store::market_transfer_in).
#[event_cpi]
#[derive(Accounts)]
pub struct MarketTransferIn<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// The authority of the source account.
    pub from_authority: Signer<'info>,
    /// Market.
    #[account(mut, has_one = store)]
    pub market: AccountLoader<'info, Market>,
    /// The source account.
    #[account(mut, token::mint = vault.mint, constraint = from.key() != vault.key())]
    pub from: Account<'info, TokenAccount>,
    /// The market vault.
    #[account(
        mut,
        token::authority = store,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            vault.mint.as_ref(),
        ],
        bump,
    )]
    pub vault: Account<'info, TokenAccount>,
    /// Token Program.
    pub token_program: Program<'info, Token>,
}

/// Transfer some tokens into the market.
///
/// ## CHECK
/// - Only MARKET_KEEPER can transfer in tokens with this method.
pub(crate) fn unchecked_market_transfer_in(
    ctx: Context<MarketTransferIn>,
    amount: u64,
) -> Result<()> {
    use anchor_spl::token;

    {
        let is_collateral_token = ctx
            .accounts
            .market
            .load()?
            .validated_meta(&ctx.accounts.store.key())?
            .is_collateral_token(&ctx.accounts.from.mint);
        require!(is_collateral_token, CoreError::InvalidArgument);
    }

    if amount != 0 {
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.from.to_account_info(),
                    to: ctx.accounts.vault.to_account_info(),
                    authority: ctx.accounts.from_authority.to_account_info(),
                },
            ),
            amount,
        )?;
        let event_emitter =
            EventEmitter::new(&ctx.accounts.event_authority, ctx.bumps.event_authority);
        let token = &ctx.accounts.vault.mint;
        let mut market = RevertibleMarket::new(
            &ctx.accounts.market,
            // Virtual inventory feature is not required here.
            None,
            event_emitter,
        )?;
        market
            .record_transferred_in_by_token(token, &amount)
            .map_err(ModelError::from)?;
        market.commit();
    }

    Ok(())
}

impl<'info> internal::Authentication<'info> for MarketTransferIn<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition for [`update_market_config`](crate::gmsol_store::update_market_config)
/// and [`update_market_config_flag`](crate::gmsol_store::update_market_config_flag).
#[derive(Accounts)]
pub struct UpdateMarketConfig<'info> {
    /// The caller.
    pub authority: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// Market.
    #[account(mut, has_one = store)]
    pub market: AccountLoader<'info, Market>,
}

impl<'info> internal::Authentication<'info> for UpdateMarketConfig<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// Update market config by key.
///
/// ## CHECK
/// - Only MARKET_KEEPER can update the config of market.
pub(crate) fn unchecked_update_market_config(
    ctx: Context<UpdateMarketConfig>,
    key: &str,
    value: Factor,
) -> Result<()> {
    *ctx.accounts.market.load_mut()?.get_config_mut(key)? = value;
    msg!(
        "{}: set {} = {}",
        ctx.accounts.market.load()?.meta.market_token_mint,
        key,
        value
    );
    Ok(())
}

/// Update market config flag by key.
///
/// ## CHECK
/// - Only MARKET_KEEPER can update the config of market.
pub(crate) fn unchecked_update_market_config_flag(
    ctx: Context<UpdateMarketConfig>,
    key: &str,
    value: bool,
) -> Result<()> {
    let previous = ctx
        .accounts
        .market
        .load_mut()?
        .set_config_flag(key, value)?;
    msg!(
        "{}: set {} = {}, previous = {}",
        ctx.accounts.market.load()?.meta.market_token_mint,
        key,
        value,
        previous,
    );
    Ok(())
}

/// The accounts definition for [`update_market_config_with_buffer`](crate::gmsol_store::update_market_config_with_buffer).
///
/// *[See also the documentation for the instruction.](crate::gmsol_store::update_market_config_with_buffer)*
#[derive(Accounts)]
pub struct UpdateMarketConfigWithBuffer<'info> {
    /// The caller.
    pub authority: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// Market.
    #[account(mut, has_one = store)]
    pub market: AccountLoader<'info, Market>,
    /// The buffer to use.
    #[account(mut, has_one = store, has_one = authority @ CoreError::PermissionDenied)]
    pub buffer: Account<'info, MarketConfigBuffer>,
}

/// Update market config with buffer.
///
/// ## CHECK
/// - Only MARKET_KEEPER can udpate the config of market.
pub(crate) fn unchecked_update_market_config_with_buffer(
    ctx: Context<UpdateMarketConfigWithBuffer>,
) -> Result<()> {
    let buffer = &ctx.accounts.buffer;
    require_gt!(
        buffer.expiry,
        Clock::get()?.unix_timestamp,
        CoreError::InvalidArgument
    );
    ctx.accounts
        .market
        .load_mut()?
        .update_config_with_buffer(buffer)?;
    msg!(
        "{} updated with buffer {}",
        ctx.accounts.market.load()?.description()?,
        buffer.key()
    );
    Ok(())
}

impl<'info> internal::Authentication<'info> for UpdateMarketConfigWithBuffer<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition for read-only instructions for market.
#[derive(Accounts)]
pub struct ReadMarket<'info> {
    /// Market.
    pub market: AccountLoader<'info, Market>,
}

/// Get market status.
pub(crate) fn get_market_status(
    ctx: Context<ReadMarket>,
    prices: &Prices<u128>,
    maximize_pnl: bool,
    maximize_pool_value: bool,
) -> Result<MarketStatus> {
    let market = ctx.accounts.market.load()?;
    let status = MarketStatus::from_market(&market, prices, maximize_pnl, maximize_pool_value)
        .map_err(ModelError::from)?;
    Ok(status)
}

/// The accounts definition for read-only instructions for market.
#[derive(Accounts)]
pub struct ReadMarketWithToken<'info> {
    /// Market.
    #[account(
        constraint = market.load()?.meta.market_token_mint == market_token.key() @ CoreError::InvalidArgument,
    )]
    pub market: AccountLoader<'info, Market>,
    /// Market token.
    pub market_token: Account<'info, Mint>,
}

/// Get market token price.
pub(crate) fn get_market_token_price(
    ctx: Context<ReadMarketWithToken>,
    prices: &Prices<u128>,
    pnl_factor: PnlFactorKind,
    maximize: bool,
) -> Result<u128> {
    let market = ctx.accounts.market.load()?;
    let liquidity_market = market.as_liquidity_market(&ctx.accounts.market_token);
    let price = liquidity_market
        .market_token_price(prices, pnl_factor, maximize)
        .map_err(ModelError::from)?;
    Ok(price)
}

/// The accounts definition for [`initialize_market_config_buffer`](crate::gmsol_store::initialize_market_config_buffer).
///
/// *[See also the documentation for the instruction.](crate::gmsol_store::initialize_market_config_buffer)*
#[derive(Accounts)]
pub struct InitializeMarketConfigBuffer<'info> {
    /// The caller.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// Buffer account to create.
    #[account(init, payer = authority, space = 8 + MarketConfigBuffer::init_space(0))]
    pub buffer: Account<'info, MarketConfigBuffer>,
    /// System Program.
    pub system_program: Program<'info, System>,
}

/// Initialize a market config buffer account.
pub(crate) fn initialize_market_config_buffer(
    ctx: Context<InitializeMarketConfigBuffer>,
    expire_after_secs: u32,
) -> Result<()> {
    let buffer = &mut ctx.accounts.buffer;
    buffer.authority = ctx.accounts.authority.key();
    buffer.store = ctx.accounts.store.key();
    buffer.expiry = Clock::get()?
        .unix_timestamp
        .saturating_add_unsigned(expire_after_secs as u64);
    Ok(())
}

/// The accounts definition for [`set_market_config_buffer_authority`](crate::gmsol_store::set_market_config_buffer_authority).
///
/// *[See also the documentation for the instruction.](crate::gmsol_store::set_market_config_buffer_authority)*
#[derive(Accounts)]
pub struct SetMarketConfigBufferAuthority<'info> {
    /// The authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Buffer.
    #[account(mut, has_one = authority @ CoreError::PermissionDenied)]
    pub buffer: Account<'info, MarketConfigBuffer>,
}

/// Set the authority of the buffer account.
pub(crate) fn set_market_config_buffer_authority(
    ctx: Context<SetMarketConfigBufferAuthority>,
    new_authority: Pubkey,
) -> Result<()> {
    ctx.accounts.buffer.authority = new_authority;
    Ok(())
}

/// The accounts definition for [`close_market_config_buffer`](crate::gmsol_store::close_market_config_buffer).
///
/// *[See also the documentation for the instruction.](crate::gmsol_store::close_market_config_buffer)*
#[derive(Accounts)]
pub struct CloseMarketConfigBuffer<'info> {
    /// The authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Buffer.
    #[account(mut, close = receiver, has_one = authority @ CoreError::PermissionDenied)]
    pub buffer: Account<'info, MarketConfigBuffer>,
    /// Receiver.
    /// CHECK: Only used to receive funds after closing the buffer account.
    #[account(mut)]
    pub receiver: UncheckedAccount<'info>,
}

/// Close the buffer account.
pub(crate) fn close_market_config_buffer(_ctx: Context<CloseMarketConfigBuffer>) -> Result<()> {
    Ok(())
}

/// The accounts definition for [`push_to_market_config_buffer`](crate::gmsol_store::push_to_market_config_buffer).
///
/// *[See also the documentation for the instruction.](crate::gmsol_store::push_to_market_config_buffer)*
#[derive(Accounts)]
#[instruction(new_configs: Vec<(String, Factor)>)]
pub struct PushToMarketConfigBuffer<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Buffer.
    #[account(
        mut,
        has_one = authority @ CoreError::PermissionDenied,
        realloc = 8 + buffer.space_after_push(new_configs.len()),
        realloc::payer = authority,
        realloc::zero = false,
    )]
    pub buffer: Account<'info, MarketConfigBuffer>,
    system_program: Program<'info, System>,
}

/// Push to the buffer account.
pub(crate) fn push_to_market_config_buffer(
    ctx: Context<PushToMarketConfigBuffer>,
    new_configs: Vec<EntryArgs>,
) -> Result<()> {
    let buffer = &mut ctx.accounts.buffer;
    for entry in new_configs {
        buffer.push(entry.try_into()?);
    }
    Ok(())
}

/// The accounts definition for [`toggle_gt_minting`](crate::gmsol_store::toggle_gt_minting).
///
/// *[See also the documentation for the instruction.](crate::gmsol_store::toggle_gt_minting)*
#[derive(Accounts)]
pub struct ToggleGTMinting<'info> {
    /// The caller.
    pub authority: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// Market.
    #[account(mut, has_one = store)]
    pub market: AccountLoader<'info, Market>,
}

/// Toggle GT Minting.
///
/// ## CHECK
/// - Only MARKET_KEEPER can use this instruction.
pub(crate) fn unchecked_toggle_gt_minting(
    ctx: Context<ToggleGTMinting>,
    enable: bool,
) -> Result<()> {
    ctx.accounts
        .market
        .load_mut()?
        .set_is_gt_minting_enbaled(enable);
    Ok(())
}

impl<'info> internal::Authentication<'info> for ToggleGTMinting<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition for [`claim_fees_from_market`](crate::gmsol_store::claim_fees_from_market).
///
/// *[See also the documentation for the instruction.](crate::gmsol_store::claim_fees_from_market)*
#[event_cpi]
#[derive(Accounts)]
pub struct ClaimFeesFromMarket<'info> {
    pub authority: Signer<'info>,
    pub store: AccountLoader<'info, Store>,
    #[account(mut, has_one = store)]
    pub market: AccountLoader<'info, Market>,
    pub token_mint: InterfaceAccount<'info, anchor_spl::token_interface::Mint>,
    #[account(
        mut,
        token::mint = token_mint,
        token::authority = store,
        token::token_program = token_program,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            token_mint.key().as_ref(),
        ],
        bump,
    )]
    pub vault: InterfaceAccount<'info, anchor_spl::token_interface::TokenAccount>,
    #[account(
        mut,
        token::mint = token_mint,
    )]
    pub target: InterfaceAccount<'info, anchor_spl::token_interface::TokenAccount>,
    pub token_program: Interface<'info, anchor_spl::token_interface::TokenInterface>,
}

/// Claim fees from the market.
///
/// # Errors
/// - Only the receiver of treasury can claim fees.
pub(crate) fn claim_fees_from_market(ctx: Context<ClaimFeesFromMarket>) -> Result<u64> {
    // Validate the authority to be the receiver for the treasury.
    ctx.accounts
        .store
        .load()?
        .validate_not_restarted()?
        .validate_claim_fees_address(ctx.accounts.authority.key)?;

    let event_emitter = EventEmitter::new(&ctx.accounts.event_authority, ctx.bumps.event_authority);

    let amount = {
        let token = ctx.accounts.token_mint.key();

        let mut market = RevertibleMarket::new(
            &ctx.accounts.market,
            // Virtual inventory feature is not required here.
            None,
            event_emitter,
        )?;
        let is_long_token = market
            .market_meta()
            .to_token_side(&token)
            .map_err(CoreError::from)?;
        let the_opposite_side = !is_long_token;
        let is_pure = market.market_meta().is_pure();
        let pool = market.claimable_fee_pool_mut().map_err(ModelError::from)?;

        let mut deltas = (0, 0);

        // Saturating claim all fees from the pool.
        let mut amount: u64 = pool
            .amount(is_long_token)
            .map_err(ModelError::from)?
            .min(u128::from(u64::MAX))
            .try_into()
            .expect("must success");

        deltas.0 = (u128::from(amount))
            .to_opposite_signed()
            .map_err(ModelError::from)?;

        if is_pure {
            let the_opposite_side_amount: u64 = pool
                .amount(the_opposite_side)
                .map_err(ModelError::from)?
                .min(u128::from(u64::MAX))
                .try_into()
                .expect("must success");
            deltas.1 = (u128::from(the_opposite_side_amount))
                .to_opposite_signed()
                .map_err(ModelError::from)?;
            amount = amount
                .checked_add(the_opposite_side_amount)
                .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
        }

        if deltas.0 != 0 {
            pool.apply_delta_amount(is_long_token, &deltas.0)
                .map_err(ModelError::from)?;
        }

        if deltas.1 != 0 {
            pool.apply_delta_amount(the_opposite_side, &deltas.1)
                .map_err(ModelError::from)?;
        }

        market
            .validate_market_balance_for_the_given_token(&token, amount)
            .map_err(ModelError::from)?;
        market.commit();

        amount
    };

    // Transfer out the tokens.
    let token = &ctx.accounts.token_mint;
    MarketTransferOutOperation::builder()
        .store(&ctx.accounts.store)
        .market(&ctx.accounts.market)
        .amount(amount)
        .decimals(token.decimals)
        .to(ctx.accounts.target.to_account_info())
        .token_mint(token.to_account_info())
        .vault(ctx.accounts.vault.to_account_info())
        .token_program(ctx.accounts.token_program.to_account_info())
        .event_emitter(event_emitter)
        .build()
        .execute()?;

    msg!(
        "Claimed `{}` {} from the {} market",
        amount,
        token.key(),
        ctx.accounts.market.load()?.meta.market_token_mint
    );
    Ok(amount)
}
