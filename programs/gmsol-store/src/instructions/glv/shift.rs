use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
};
use gmsol_utils::InitSpace;

use crate::{
    constants,
    ops::{
        execution_fee::PayExecutionFeeOperation,
        glv::ExecuteGlvShiftOperation,
        shift::{CreateShiftOperation, CreateShiftParams},
    },
    ordered_tokens,
    states::{
        common::action::{Action, ActionExt},
        glv::GlvShift,
        Chainlink, Glv, Market, NonceBytes, Oracle, RoleKey, Seed, Store, TokenMapHeader,
    },
    utils::internal,
    CoreError,
};

/// The accounts definition for [`create_glv_shift`](crate::create_glv_shift) instruction.
#[derive(Accounts)]
#[instruction(nonce: [u8; 32])]
pub struct CreateGlvShift<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// GLV.
    #[account(
        has_one = store,
        constraint = glv.load()?.contains(&from_market_token.key()) @ CoreError::InvalidArgument,
        constraint = glv.load()?.contains(&to_market_token.key()) @ CoreError::InvalidArgument,
    )]
    pub glv: AccountLoader<'info, Glv>,
    /// From market.
    #[account(
        mut,
        has_one = store,
        constraint = from_market.load()?.meta().market_token_mint == from_market_token.key() @ CoreError::MarketTokenMintMismatched,
    )]
    pub from_market: AccountLoader<'info, Market>,
    /// To market.
    #[account(
        mut,
        has_one = store,
        constraint = to_market.load()?.meta().market_token_mint == to_market_token.key() @ CoreError::MarketTokenMintMismatched,
    )]
    pub to_market: AccountLoader<'info, Market>,
    /// GLV shift.
    #[account(
        init,
        payer = authority,
        space = 8 + GlvShift::INIT_SPACE,
        seeds = [GlvShift::SEED, store.key().as_ref(), glv.key().as_ref(), &nonce],
        bump,
    )]
    pub glv_shift: AccountLoader<'info, GlvShift>,
    /// From market token.
    #[account(
        constraint = from_market_token.key() != to_market_token.key() @ CoreError::InvalidShiftMarkets,
    )]
    pub from_market_token: Box<Account<'info, Mint>>,
    /// To market token.
    pub to_market_token: Box<Account<'info, Mint>>,
    /// Vault for from market tokens.
    #[account(
        associated_token::mint = from_market_token,
        associated_token::authority = glv,
    )]
    pub from_market_token_vault: Box<Account<'info, TokenAccount>>,
    /// Vault for to market tokens.
    #[account(
        associated_token::mint = to_market_token,
        associated_token::authority = glv,
    )]
    pub to_market_token_vault: Box<Account<'info, TokenAccount>>,
    /// The system program.
    pub system_program: Program<'info, System>,
    /// The token program.
    pub token_program: Program<'info, Token>,
    /// The associated token program.
    pub associated_token_program: Program<'info, AssociatedToken>,
}

impl<'info> internal::Create<'info, GlvShift> for CreateGlvShift<'info> {
    type CreateParams = CreateShiftParams;

    fn action(&self) -> AccountInfo<'info> {
        self.glv_shift.to_account_info()
    }

    fn payer(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn payer_seeds(&self) -> Result<Option<Vec<Vec<u8>>>> {
        Ok(Some(self.glv.load()?.vec_signer_seeds()))
    }

    fn system_program(&self) -> AccountInfo<'info> {
        self.system_program.to_account_info()
    }

    fn create_impl(
        &mut self,
        params: &Self::CreateParams,
        nonce: &NonceBytes,
        bumps: &Self::Bumps,
        _remaining_accounts: &'info [AccountInfo<'info>],
    ) -> Result<()> {
        CreateShiftOperation::builder()
            .store(&self.store)
            .owner(self.glv.to_account_info())
            .shift(&self.glv_shift)
            .from_market(&self.from_market)
            .from_market_token_account(&self.from_market_token_vault)
            .to_market(&self.to_market)
            .to_market_token_account(&self.to_market_token_vault)
            .nonce(nonce)
            .bump(bumps.glv_shift)
            .params(params)
            .build()
            .execute()?;

        // Set the funder of the GLV shift.
        {
            self.glv_shift.exit(&crate::ID)?;
            self.glv_shift
                .load_mut()?
                .header_mut()
                .set_rent_receiver(self.authority.key());
        }

        Ok(())
    }
}

impl<'info> internal::Authentication<'info> for CreateGlvShift<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition for [`close_glv_shift`](crate::close_glv_shift) instruction.
#[event_cpi]
#[derive(Accounts)]
pub struct CloseGlvShift<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Funder of the GLV shift.
    /// CHECK: only used to receive funds.
    #[account(mut)]
    pub funder: UncheckedAccount<'info>,
    /// The store.
    pub store: AccountLoader<'info, Store>,
    /// GLV.
    #[account(
        has_one = store,
        constraint = glv.load()?.contains(&from_market_token.key()) @ CoreError::InvalidArgument,
        constraint = glv.load()?.contains(&to_market_token.key()) @ CoreError::InvalidArgument,
    )]
    pub glv: AccountLoader<'info, Glv>,
    /// The GLV shift to close.
    #[account(
        mut,
        constraint = glv_shift.load()?.header().owner == glv.key() @ CoreError::OwnerMismatched,
        constraint = glv_shift.load()?.header().store == store.key() @ CoreError::StoreMismatched,
        // The rent receiver of a GLV shift must be the funder.
        constraint = glv_shift.load()?.header().rent_receiver() == funder.key @ CoreError::RentReceiverMismatched,
        seeds = [GlvShift::SEED, store.key().as_ref(), glv.key().as_ref(), &glv_shift.load()?.header().nonce],
        bump = glv_shift.load()?.header().bump,
    )]
    pub glv_shift: AccountLoader<'info, GlvShift>,
    /// From Market token.
    #[account(
        constraint = glv_shift.load()?.tokens().from_market_token() == from_market_token.key() @ CoreError::MarketTokenMintMismatched
    )]
    pub from_market_token: Box<Account<'info, Mint>>,
    /// To Market token.
    #[account(
        constraint = glv_shift.load()?.tokens().to_market_token() == to_market_token.key() @ CoreError::MarketTokenMintMismatched
    )]
    pub to_market_token: Box<Account<'info, Mint>>,
    /// The system program.
    pub system_program: Program<'info, System>,
    /// The token program.
    pub token_program: Program<'info, Token>,
    /// The associated token program.
    pub associated_token_program: Program<'info, AssociatedToken>,
}

impl<'info> internal::Close<'info, GlvShift> for CloseGlvShift<'info> {
    fn expected_keeper_role(&self) -> &str {
        RoleKey::ORDER_KEEPER
    }

    fn rent_receiver(&self) -> AccountInfo<'info> {
        debug_assert!(
            self.glv_shift.load().unwrap().header().rent_receiver() == self.funder.key,
            "The rent receiver must have been checked to be the owner"
        );
        self.funder.to_account_info()
    }

    fn skip_completion_check_for_keeper(&self) -> bool {
        true
    }

    fn process(&self, _init_if_needed: bool) -> Result<internal::Success> {
        Ok(true)
    }

    fn event_authority(&self, bumps: &Self::Bumps) -> (AccountInfo<'info>, u8) {
        (self.event_authority.clone(), bumps.event_authority)
    }

    fn action(&self) -> &AccountLoader<'info, GlvShift> {
        &self.glv_shift
    }
}

impl<'info> internal::Authentication<'info> for CloseGlvShift<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition for [`execute_glv_shift`](crate::execute_glv_shift) instruction.
///
/// Remaining accounts expected by this instruction:
///
///   - 0..M. `[]` M feed accounts, where M represents the total number of unique tokens
///     of markets.
#[derive(Accounts)]
pub struct ExecuteGlvShift<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    #[account(has_one = token_map)]
    pub store: AccountLoader<'info, Store>,
    /// Token Map.
    #[account(has_one = store)]
    pub token_map: AccountLoader<'info, TokenMapHeader>,
    /// Oracle buffer to use.
    #[account(mut, has_one = store)]
    pub oracle: AccountLoader<'info, Oracle>,
    /// GLV account.
    #[account(
        has_one = store,
        constraint = glv.load()?.contains(&from_market_token.key()) @ CoreError::InvalidArgument,
        constraint = glv.load()?.contains(&to_market_token.key()) @ CoreError::InvalidArgument,
    )]
    pub glv: AccountLoader<'info, Glv>,
    /// From Market.
    #[account(
        mut,
        has_one = store,
        constraint = from_market.load()?.meta().market_token_mint == from_market_token.key() @ CoreError::MarketTokenMintMismatched,
    )]
    pub from_market: AccountLoader<'info, Market>,
    /// To Market.
    #[account(
        mut,
        has_one = store,
        constraint = to_market.load()?.meta().market_token_mint == to_market_token.key() @ CoreError::MarketTokenMintMismatched,
    )]
    pub to_market: AccountLoader<'info, Market>,
    /// The GLV shift to close.
    #[account(
        mut,
        constraint = glv_shift.load()?.header().owner == glv.key() @ CoreError::OwnerMismatched,
        constraint = glv_shift.load()?.header().store == store.key() @ CoreError::StoreMismatched,
        constraint = glv_shift.load()?.tokens().from_market_token_account() == from_market_token_glv_vault.key() @ CoreError::MarketTokenAccountMismatched,
        constraint = glv_shift.load()?.tokens().to_market_token_account() == to_market_token_glv_vault.key() @ CoreError::MarketTokenAccountMismatched,
        seeds = [GlvShift::SEED, store.key().as_ref(), glv.key().as_ref(), &glv_shift.load()?.header().nonce],
        bump = glv_shift.load()?.header().bump,
    )]
    pub glv_shift: AccountLoader<'info, GlvShift>,
    /// From Market token.
    #[account(
        mut,
        constraint = glv_shift.load()?.tokens().from_market_token() == from_market_token.key() @ CoreError::MarketTokenMintMismatched
    )]
    pub from_market_token: Box<Account<'info, Mint>>,
    /// To Market token.
    #[account(
        mut,
        constraint = glv_shift.load()?.tokens().to_market_token() == to_market_token.key() @ CoreError::MarketTokenMintMismatched
    )]
    pub to_market_token: Box<Account<'info, Mint>>,
    /// The escrow account for from market tokens.
    #[account(
        mut,
        associated_token::mint = from_market_token,
        associated_token::authority = glv,
    )]
    pub from_market_token_glv_vault: Box<Account<'info, TokenAccount>>,
    /// The escrow account for to market tokens.
    #[account(
        mut,
        associated_token::mint = to_market_token,
        associated_token::authority = glv,
    )]
    pub to_market_token_glv_vault: Box<Account<'info, TokenAccount>>,
    /// From market token vault.
    #[account(
        mut,
        token::mint = from_market_token,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            from_market_token_vault.mint.as_ref(),
            &[],
        ],
        bump,
    )]
    pub from_market_token_vault: Box<Account<'info, TokenAccount>>,
    /// The token program.
    pub token_program: Program<'info, Token>,
    /// Chainlink Program.
    pub chainlink_program: Option<Program<'info, Chainlink>>,
}

/// Execute GLV shift.
///
/// # CHECK
/// - Only ORDER_KEEPER is allowed to execute shift.
pub fn unchecked_execute_glv_shift<'info>(
    ctx: Context<'_, '_, 'info, 'info, ExecuteGlvShift<'info>>,
    execution_lamports: u64,
    throw_on_execution_error: bool,
) -> Result<()> {
    let accounts = ctx.accounts;
    let remaining_accounts = ctx.remaining_accounts;

    let executed = accounts.perform_execution(remaining_accounts, throw_on_execution_error)?;

    if executed {
        accounts.glv_shift.load_mut()?.header_mut().completed()?;
    } else {
        accounts.glv_shift.load_mut()?.header_mut().cancelled()?;
    }

    // It must be placed at the end to be executed correctly.
    accounts.pay_execution_fee(execution_lamports)?;

    Ok(())
}

impl<'info> internal::Authentication<'info> for ExecuteGlvShift<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

impl<'info> ExecuteGlvShift<'info> {
    #[inline(never)]
    fn pay_execution_fee(&self, execution_fee: u64) -> Result<()> {
        let execution_lamports = self.glv_shift.load()?.execution_lamports(execution_fee);
        PayExecutionFeeOperation::builder()
            .payer(self.glv_shift.to_account_info())
            .receiver(self.authority.to_account_info())
            .execution_lamports(execution_lamports)
            .build()
            .execute()?;
        Ok(())
    }

    #[inline(never)]
    fn ordered_tokens(&self) -> Result<Vec<Pubkey>> {
        let from = *self.from_market.load()?.meta();
        let to = *self.to_market.load()?.meta();

        Ok(ordered_tokens(&from, &to).into_iter().collect())
    }

    fn perform_execution(
        &mut self,
        remaining_accounts: &'info [AccountInfo<'info>],
        throw_on_execution_error: bool,
    ) -> Result<bool> {
        let tokens = self.ordered_tokens()?;

        let builder = ExecuteGlvShiftOperation::builder()
            .glv_shift(&self.glv_shift)
            .token_program(self.token_program.to_account_info())
            .throw_on_execution_error(throw_on_execution_error)
            .store(&self.store)
            .glv(&self.glv)
            .from_market(&self.from_market)
            .from_market_token_mint(&mut self.from_market_token)
            .from_market_token_glv_vault(&self.from_market_token_glv_vault)
            .from_market_token_withdrawal_vault(self.from_market_token_vault.to_account_info())
            .to_market(&self.to_market)
            .to_market_token_mint(&mut self.to_market_token)
            .to_market_token_glv_vault(self.to_market_token_glv_vault.to_account_info());

        self.oracle.load_mut()?.with_prices(
            &self.store,
            &self.token_map,
            &tokens,
            remaining_accounts,
            self.chainlink_program.as_ref(),
            |oracle, _remaining_accounts| builder.oracle(oracle).build().unchecked_execute(),
        )
    }
}
