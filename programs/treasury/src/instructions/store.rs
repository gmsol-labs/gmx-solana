use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{Mint, TokenAccount, TokenInterface},
};
use gmsol_store::{
    cpi::{
        accounts::{ClaimFeesFromMarket, ConfigurateGt, TransferReceiver as StoreTransferReceiver},
        claim_fees_from_market, gt_set_referral_reward_factors, transfer_receiver,
    },
    program::GmsolStore,
    utils::{CpiAuthentication, WithStore},
    CoreError,
};

use crate::{
    constants,
    states::{config::ReceiverSigner, Config},
};

/// The accounts definition for [`transfer_receiver`](crate::gmsol_treasury::transfer_receiver).
#[derive(Accounts)]
pub struct TransferReceiver<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    #[account(mut)]
    pub store: UncheckedAccount<'info>,
    /// Config.
    #[account(has_one = store)]
    pub config: AccountLoader<'info, Config>,
    /// Receiver.
    #[account(
        seeds = [constants::RECEIVER_SEED, config.key().as_ref()],
        bump = config.load()?.receiver_bump,
    )]
    pub receiver: SystemAccount<'info>,
    /// The new receiver.
    /// CHECK: only used as an identifier.
    pub next_receiver: UncheckedAccount<'info>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
    /// The system program.
    pub system_program: Program<'info, System>,
}

/// Claim fees from a market.
/// # CHECK
/// Only [`TREASURY_OWNER`](crate::roles::TREASURY_OWNER) can use.
pub(crate) fn unchecked_transfer_receiver(ctx: Context<TransferReceiver>) -> Result<()> {
    let config = &ctx.accounts.config;
    let signer = ReceiverSigner::new(config.key(), config.load()?.receiver_bump);
    let cpi_ctx = ctx.accounts.set_receiver_ctx();
    transfer_receiver(cpi_ctx.with_signer(&[&signer.as_seeds()]))?;
    Ok(())
}

impl<'info> WithStore<'info> for TransferReceiver<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for TransferReceiver<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}

impl<'info> TransferReceiver<'info> {
    fn set_receiver_ctx(&self) -> CpiContext<'_, '_, '_, 'info, StoreTransferReceiver<'info>> {
        CpiContext::new(
            self.store_program.to_account_info(),
            StoreTransferReceiver {
                authority: self.receiver.to_account_info(),
                store: self.store.to_account_info(),
                next_receiver: self.next_receiver.to_account_info(),
            },
        )
    }
}

/// The accounts definition for [`claim_fees`](crate::gmsol_treasury::claim_fees).
#[derive(Accounts)]
pub struct ClaimFees<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    pub store: UncheckedAccount<'info>,
    /// Config to initialize with.
    #[account(has_one = store)]
    pub config: AccountLoader<'info, Config>,
    /// Receiver.
    #[account(
        seeds = [constants::RECEIVER_SEED, config.key().as_ref()],
        bump = config.load()?.receiver_bump,
    )]
    pub receiver: SystemAccount<'info>,
    /// Market to claim fees from.
    /// CHECK: check by CPI.
    #[account(mut)]
    pub market: UncheckedAccount<'info>,
    /// Token.
    pub token: InterfaceAccount<'info, Mint>,
    /// Vault.
    /// CHECK: check by CPI.
    #[account(mut)]
    pub vault: UncheckedAccount<'info>,
    /// Reciever vault.
    #[account(
        init_if_needed,
        payer = authority,
        associated_token::authority = receiver,
        associated_token::mint = token,
    )]
    pub receiver_vault: InterfaceAccount<'info, TokenAccount>,
    /// Event authority.
    /// CHECK: check by CPI.
    pub event_authority: UncheckedAccount<'info>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
    /// The token program.
    pub token_program: Interface<'info, TokenInterface>,
    /// Associated token program.
    pub associated_token_program: Program<'info, AssociatedToken>,
    /// The system program.
    pub system_program: Program<'info, System>,
}

/// Claim fees from a market.
/// # CHECK
/// Only [`TREASURY_KEEPER`](crate::roles::TREASURY_KEEPER) can use.
pub(crate) fn unchecked_claim_fees(ctx: Context<ClaimFees>, min_amount: u64) -> Result<()> {
    let config = &ctx.accounts.config;
    let signer = ReceiverSigner::new(config.key(), config.load()?.receiver_bump);
    let cpi_ctx = ctx.accounts.claim_fees_from_market_ctx();
    let amount = claim_fees_from_market(cpi_ctx.with_signer(&[&signer.as_seeds()]))?;

    require_gte!(amount.get(), min_amount, CoreError::NotEnoughTokenAmount);

    msg!("[Treasury] claimed {} tokens from the market", amount.get());
    Ok(())
}

impl<'info> WithStore<'info> for ClaimFees<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for ClaimFees<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}

impl<'info> ClaimFees<'info> {
    fn claim_fees_from_market_ctx(
        &self,
    ) -> CpiContext<'_, '_, '_, 'info, ClaimFeesFromMarket<'info>> {
        CpiContext::new(
            self.store_program.to_account_info(),
            ClaimFeesFromMarket {
                authority: self.receiver.to_account_info(),
                store: self.store.to_account_info(),
                market: self.market.to_account_info(),
                token_mint: self.token.to_account_info(),
                vault: self.vault.to_account_info(),
                target: self.receiver_vault.to_account_info(),
                token_program: self.token_program.to_account_info(),
                event_authority: self.event_authority.to_account_info(),
                program: self.store_program.to_account_info(),
            },
        )
    }
}

/// The accounts definition for [`set_referral_reward`](crate::gmsol_treasury::set_referral_reward).
#[derive(Accounts)]
pub struct SetReferralReward<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    #[account(mut)]
    pub store: UncheckedAccount<'info>,
    /// Config.
    #[account(has_one = store)]
    pub config: AccountLoader<'info, Config>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
}

/// Set referral reward.
/// # CHECK
/// Only [`TREASURY_ADMIN`](crate::roles::TREASURY_ADMIN) can use.
pub(crate) fn unchecked_set_referral_reward(
    ctx: Context<SetReferralReward>,
    factors: Vec<u128>,
) -> Result<()> {
    let signer = ctx.accounts.config.load()?.signer();
    let cpi_ctx = ctx.accounts.configurate_gt_ctx();
    gt_set_referral_reward_factors(cpi_ctx.with_signer(&[&signer.as_seeds()]), factors)?;
    Ok(())
}

impl<'info> WithStore<'info> for SetReferralReward<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for SetReferralReward<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}

impl<'info> SetReferralReward<'info> {
    fn configurate_gt_ctx(&self) -> CpiContext<'_, '_, '_, 'info, ConfigurateGt<'info>> {
        CpiContext::new(
            self.store_program.to_account_info(),
            ConfigurateGt {
                authority: self.config.to_account_info(),
                store: self.store.to_account_info(),
            },
        )
    }
}
