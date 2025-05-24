use anchor_lang::prelude::*;
use anchor_spl::token::{transfer_checked, Mint, Token, TokenAccount, TransferChecked};
use gmsol_utils::market::ordered_tokens;

use crate::{
    constants,
    ops::{execution_fee::PayExecutionFeeOperation, shift::ExecuteShiftOperation},
    states::{
        common::action::{ActionExt, ActionSigner},
        feature::{ActionDisabledFlag, DomainDisabledFlag},
        Chainlink, Market, Oracle, Shift, Store, TokenMapHeader,
    },
    utils::internal,
    CoreError,
};

/// The accounts definition for the [`execute_shift`](crate::gmsol_store::execute_shift) instruction.
///
/// Remaining accounts expected by this instruction:
///
///   - 0..N. `[]` N feed accounts, where N represents the total number of unique tokens
///     in the markets.
#[event_cpi]
#[derive(Accounts)]
pub struct ExecuteShift<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    #[account(has_one = token_map)]
    pub store: AccountLoader<'info, Store>,
    /// Token map.
    #[account(has_one = store)]
    pub token_map: AccountLoader<'info, TokenMapHeader>,
    /// Oracle buffer to use.
    #[account(mut, has_one = store)]
    pub oracle: AccountLoader<'info, Oracle>,
    /// From market.
    #[account(
        mut,
        has_one = store,
        constraint = from_market.load()?.meta().market_token_mint == from_market_token.key() @ CoreError::MarketTokenMintMismatched,
        constraint = from_market.load()?.meta().long_token_mint == shift.load()?.tokens.long_token @ CoreError::TokenMintMismatched,
        constraint = from_market.load()?.meta().short_token_mint== shift.load()?.tokens.short_token @ CoreError::TokenMintMismatched,
    )]
    pub from_market: AccountLoader<'info, Market>,
    /// To market.
    #[account(
        mut,
        has_one = store,
        constraint = to_market.load()?.meta().market_token_mint == to_market_token.key() @ CoreError::MarketTokenMintMismatched,
        constraint = to_market.load()?.meta().long_token_mint == shift.load()?.tokens.long_token @ CoreError::TokenMintMismatched,
        constraint = to_market.load()?.meta().short_token_mint== shift.load()?.tokens.short_token @ CoreError::TokenMintMismatched,
    )]
    pub to_market: AccountLoader<'info, Market>,
    /// The shift to execute.
    #[account(
        mut,
        constraint = shift.load()?.header.store == store.key() @ CoreError::StoreMismatched,
        constraint = shift.load()?.header.market == from_market.key() @ CoreError::MarketMismatched,
        constraint = shift.load()?.tokens.from_market_token_account() == from_market_token_escrow.key() @ CoreError::MarketTokenAccountMismatched,
        constraint = shift.load()?.tokens.to_market_token_account() == to_market_token_escrow.key() @ CoreError::MarketTokenAccountMismatched,
    )]
    pub shift: AccountLoader<'info, Shift>,
    /// From market token.
    #[account(
        mut,
        constraint = shift.load()?.tokens.from_market_token() == from_market_token.key() @ CoreError::MarketTokenMintMismatched,
    )]
    pub from_market_token: Box<Account<'info, Mint>>,
    /// To market token.
    #[account(
        mut,
        constraint = shift.load()?.tokens.to_market_token() == to_market_token.key() @ CoreError::MarketTokenMintMismatched,
    )]
    pub to_market_token: Box<Account<'info, Mint>>,
    /// The escrow account for from market tokens.
    #[account(
        mut,
        associated_token::mint = from_market_token,
        associated_token::authority = shift,
    )]
    pub from_market_token_escrow: Box<Account<'info, TokenAccount>>,
    /// The escrow account for to market tokens.
    #[account(
        mut,
        associated_token::mint = to_market_token,
        associated_token::authority = shift,
    )]
    pub to_market_token_escrow: Box<Account<'info, TokenAccount>>,
    /// From market token vault.
    #[account(
        mut,
        token::mint = from_market_token,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            from_market_token_vault.mint.as_ref(),
        ],
        bump,
    )]
    pub from_market_token_vault: Box<Account<'info, TokenAccount>>,
    /// The token program.
    pub token_program: Program<'info, Token>,
    /// Chainlink Program.
    pub chainlink_program: Option<Program<'info, Chainlink>>,
}

/// CHECK: only ORDER_KEEPER is allowed to execute shift.
pub fn unchecked_execute_shift<'info>(
    ctx: Context<'_, '_, 'info, 'info, ExecuteShift<'info>>,
    execution_lamports: u64,
    throw_on_execution_error: bool,
) -> Result<()> {
    let accounts = ctx.accounts;
    let remaining_accounts = ctx.remaining_accounts;

    // Validate feature enabled.
    accounts
        .store
        .load()?
        .validate_feature_enabled(DomainDisabledFlag::Shift, ActionDisabledFlag::Execute)?;

    let signer = accounts.shift.load()?.signer();

    accounts.transfer_from_market_tokens_in(&signer)?;

    let executed = accounts.perform_execution(
        remaining_accounts,
        throw_on_execution_error,
        ctx.bumps.event_authority,
    )?;

    if executed {
        accounts.shift.load_mut()?.header.completed()?;
    } else {
        accounts.shift.load_mut()?.header.cancelled()?;
        accounts.transfer_from_market_tokens_out()?;
    }

    // Is must be placed at the end to be executed correctly.
    accounts.pay_execution_fee(execution_lamports)?;

    Ok(())
}

impl<'info> internal::Authentication<'info> for ExecuteShift<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

impl<'info> ExecuteShift<'info> {
    fn transfer_from_market_tokens_in(&mut self, signer: &ActionSigner) -> Result<()> {
        let seeds = signer.as_seeds();

        transfer_checked(
            CpiContext::new(
                self.token_program.to_account_info(),
                TransferChecked {
                    from: self.from_market_token_escrow.to_account_info(),
                    mint: self.from_market_token.to_account_info(),
                    to: self.from_market_token_vault.to_account_info(),
                    authority: self.shift.to_account_info(),
                },
            )
            .with_signer(&[&seeds]),
            self.shift.load()?.params.from_market_token_amount(),
            self.from_market_token.decimals,
        )?;

        Ok(())
    }

    fn transfer_from_market_tokens_out(&self) -> Result<()> {
        use crate::internal::TransferUtils;

        let amount = self.shift.load()?.params.from_market_token_amount();
        TransferUtils::new(
            self.token_program.to_account_info(),
            &self.store,
            self.from_market_token.to_account_info(),
        )
        .transfer_out(
            self.from_market_token_vault.to_account_info(),
            self.from_market_token_escrow.to_account_info(),
            amount,
            self.from_market_token.decimals,
        )?;

        Ok(())
    }

    #[inline(never)]
    fn ordered_tokens(&self) -> Result<Vec<Pubkey>> {
        let from = *self.from_market.load()?.meta();
        let to = *self.to_market.load()?.meta();

        Ok(ordered_tokens(&from, &to).into_iter().collect())
    }

    #[inline(never)]
    fn perform_execution(
        &mut self,
        remaining_accounts: &'info [AccountInfo<'info>],
        throw_on_execution_error: bool,
        event_authority_bump: u8,
    ) -> Result<bool> {
        let tokens = self.ordered_tokens()?;

        let ops = ExecuteShiftOperation::builder()
            .store(&self.store)
            .shift(&self.shift)
            .from_market(&self.from_market)
            .to_market(&self.to_market)
            .from_market_token_mint(&mut self.from_market_token)
            .to_market_token_mint(&mut self.to_market_token)
            .from_market_token_vault(self.from_market_token_vault.to_account_info())
            .to_market_token_account(self.to_market_token_escrow.to_account_info())
            .throw_on_execution_error(throw_on_execution_error)
            .token_program(self.token_program.to_account_info())
            .event_emitter((&self.event_authority, event_authority_bump));

        let executed = self.oracle.load_mut()?.with_prices(
            &self.store,
            &self.token_map,
            &tokens,
            remaining_accounts,
            self.chainlink_program.as_ref(),
            |oracle, _remaining_accounts| ops.oracle(oracle).build().execute(),
        )?;

        Ok(executed)
    }

    fn pay_execution_fee(&self, execution_lamports: u64) -> Result<()> {
        let execution_lamports = self.shift.load()?.execution_lamports(execution_lamports);
        PayExecutionFeeOperation::builder()
            .payer(self.shift.to_account_info())
            .receiver(self.authority.to_account_info())
            .execution_lamports(execution_lamports)
            .build()
            .execute()?;
        Ok(())
    }
}
