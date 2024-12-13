use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{Mint, TokenAccount, TokenInterface},
};
use gmsol_store::{
    cpi::{
        accounts::{ClaimFeesFromMarket, SetReceiver},
        claim_fees_from_market, set_receiver,
    },
    program::GmsolStore,
    utils::{CpiAuthentication, WithStore},
    CoreError,
};

use crate::states::Config;

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
    /// Config to initialize with.
    #[account(has_one = store)]
    pub config: AccountLoader<'info, Config>,
    /// The new receiver.
    pub receiver: UncheckedAccount<'info>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
    /// The system program.
    pub system_program: Program<'info, System>,
}

/// Claim fees from a market.
/// # CHECK
/// Only [`TREASURY_OWNER`](crate::roles::TREASURY_OWNER) can use.
pub(crate) fn unchecked_transfer_receiver(ctx: Context<TransferReceiver>) -> Result<()> {
    let signer = ctx.accounts.config.load()?.signer();
    let cpi_ctx = ctx.accounts.set_receiver_ctx();
    set_receiver(cpi_ctx.with_signer(&[&signer.as_seeds()]))?;
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
    fn set_receiver_ctx(&self) -> CpiContext<'_, '_, '_, 'info, SetReceiver<'info>> {
        CpiContext::new(
            self.store_program.to_account_info(),
            SetReceiver {
                authority: self.config.to_account_info(),
                store: self.store.to_account_info(),
                receiver: self.receiver.to_account_info(),
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
        associated_token::authority = config,
        associated_token::mint = token,
    )]
    pub receiver_vault: InterfaceAccount<'info, TokenAccount>,
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
pub(crate) fn unchecked_claim_fees(ctx: Context<ClaimFees>) -> Result<()> {
    let signer = ctx.accounts.config.load()?.signer();
    let cpi_ctx = ctx.accounts.claim_fees_from_market_ctx();
    let amount = claim_fees_from_market(cpi_ctx.with_signer(&[&signer.as_seeds()]))?;
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
                authority: self.config.to_account_info(),
                store: self.store.to_account_info(),
                market: self.market.to_account_info(),
                token_mint: self.token.to_account_info(),
                vault: self.vault.to_account_info(),
                target: self.receiver_vault.to_account_info(),
                token_program: self.token_program.to_account_info(),
                associated_token_program: self.associated_token_program.to_account_info(),
            },
        )
    }
}
