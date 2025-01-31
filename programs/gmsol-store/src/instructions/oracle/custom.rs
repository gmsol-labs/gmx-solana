use anchor_lang::prelude::*;
use chainlink_datastreams::interface::ChainlinkDataStreamsInterface;
use gmsol_utils::InitSpace;

use crate::{
    states::{PriceFeed, PriceFeedPrice, PriceProviderKind, Seed, Store},
    utils::internal,
    CoreError,
};

/// The accounts definition for [`initialize_price_feed`](crate::initialize_price_feed) instruction.
#[derive(Accounts)]
#[instruction(index: u8, provider: u8, token: Pubkey)]
pub struct InitializePriceFeed<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// Price feed.
    #[account(
        init,
        payer = authority,
        space = 8 + PriceFeed::INIT_SPACE,
        seeds = [
            PriceFeed::SEED,
            store.key().as_ref(),
            authority.key().as_ref(),
            &[index],
            &[provider],
            token.as_ref(),
        ],
        bump,
    )]
    pub price_feed: AccountLoader<'info, PriceFeed>,
    /// The system program.
    pub system_program: Program<'info, System>,
}

/// CHECK: only ORDER_KEEPER is allowed to initialize price feed.
pub(crate) fn unchecked_initialize_price_feed(
    ctx: Context<InitializePriceFeed>,
    index: u8,
    provider: PriceProviderKind,
    token: &Pubkey,
    feed_id: &Pubkey,
) -> Result<()> {
    require!(
        matches!(provider, PriceProviderKind::ChainlinkDataStreams),
        CoreError::NotSupportedCustomPriceProvider
    );
    let mut feed = ctx.accounts.price_feed.load_init()?;
    feed.init(
        ctx.bumps.price_feed,
        index,
        provider,
        &ctx.accounts.store.key(),
        &ctx.accounts.authority.key(),
        token,
        feed_id,
    )?;
    Ok(())
}

impl<'info> internal::Authentication<'info> for InitializePriceFeed<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition for [`update_price_feed_with_chainlink`](crate::update_price_feed_with_chainlink) instruction.
#[derive(Accounts)]
pub struct UpdatePriceFeedWithChainlink<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// Verifier Account.
    /// CHECK: checked by CPI.
    pub verifier_account: UncheckedAccount<'info>,
    /// Access Controller Account.
    /// CHECK: check by CPI.
    pub access_controller: UncheckedAccount<'info>,
    /// Config Account.
    /// CHECK: check by CPI.
    pub config_account: UncheckedAccount<'info>,
    /// Price Feed Account.
    #[account(mut, has_one = store, has_one = authority)]
    pub price_feed: AccountLoader<'info, PriceFeed>,
    /// Chainlink Data Streams Program.
    pub chainlink: Interface<'info, ChainlinkDataStreamsInterface>,
}

/// CHECK: only ORDER_KEEPER can update custom price feed.
pub(crate) fn unchecked_update_price_feed_with_chainlink(
    ctx: Context<UpdatePriceFeedWithChainlink>,
    compressed_report: Vec<u8>,
) -> Result<()> {
    let accounts = ctx.accounts;

    require_eq!(
        accounts.price_feed.load()?.provider()?,
        PriceProviderKind::ChainlinkDataStreams,
        CoreError::InvalidArgument
    );

    let price = accounts.decode_and_validate_report(&compressed_report)?;
    accounts.verify_report(compressed_report)?;

    accounts.price_feed.load_mut()?.update(&price)?;

    Ok(())
}

impl<'info> internal::Authentication<'info> for UpdatePriceFeedWithChainlink<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

impl UpdatePriceFeedWithChainlink<'_> {
    fn decode_and_validate_report(&self, compressed_full_report: &[u8]) -> Result<PriceFeedPrice> {
        use chainlink_datastreams::report::decode_compressed_full_report;

        let report = decode_compressed_full_report(compressed_full_report).map_err(|err| {
            msg!("[Decode Error] {}", err);
            error!(CoreError::InvalidPriceReport)
        })?;

        require_keys_eq!(
            Pubkey::new_from_array(report.feed_id.0),
            self.price_feed.load()?.feed_id,
            CoreError::InvalidPriceReport
        );

        PriceFeedPrice::from_chainlink_report(&report)
    }

    fn verify_report(&self, signed_report: Vec<u8>) -> Result<()> {
        use chainlink_datastreams::interface::{verify, VerifyContext};

        let ctx = CpiContext::new(
            self.chainlink.to_account_info(),
            VerifyContext {
                verifier_account: self.verifier_account.to_account_info(),
                access_controller: self.access_controller.to_account_info(),
                user: self.store.to_account_info(),
                config_account: self.config_account.to_account_info(),
            },
        );

        verify(
            ctx.with_signer(&[&self.store.load()?.signer_seeds()]),
            signed_report,
        )?;

        Ok(())
    }
}
