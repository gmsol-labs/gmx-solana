use anchor_lang::prelude::*;
use anchor_spl::token::{transfer_checked, Mint, Token, TokenAccount, TransferChecked};

use crate::{
    constants,
    ops::{
        execution_fee::PayExecutionFeeOperation, market::MarketTransferOutOperation,
        withdrawal::ExecuteWithdrawalOperation,
    },
    states::{
        common::action::{ActionExt, ActionSigner},
        withdrawal::Withdrawal,
        Chainlink, Market, Oracle, Store, TokenMapHeader, TokenMapLoader,
    },
    utils::internal,
    CoreError,
};

/// The accounts deifinition for the [`execute_withdrawal`](crate::gmsol_store::execute_withdrawal)
/// instruction.
///
/// Remaining accounts expected by this instruction:
///
///   - 0..M. `[]` M feed accounts, where M represents the total number of tokens in the
///     swap params.
///   - M..M+N. `[writable]` N market accounts, where N represents the total number of unique
///     markets excluding the current market in the swap params.
#[derive(Accounts)]
pub struct ExecuteWithdrawal<'info> {
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
    /// Market.
    #[account(mut, has_one = store)]
    pub market: AccountLoader<'info, Market>,
    /// The withdrawal to execute.
    #[account(
        mut,
        constraint = withdrawal.load()?.header.market == market.key() @ CoreError::MarketMismatched,
        constraint = withdrawal.load()?.header.store == store.key() @ CoreError::StoreMismatched,
        constraint = withdrawal.load()?.tokens.market_token_account() == market_token_escrow.key() @ CoreError::MarketTokenAccountMismatched,
        constraint = withdrawal.load()?.tokens.final_long_token_account() == final_long_token_escrow.key() @ CoreError::MarketTokenAccountMismatched,
        constraint = withdrawal.load()?.tokens.final_short_token_account() == final_short_token_escrow.key() @ CoreError::MarketTokenAccountMismatched,
    )]
    pub withdrawal: AccountLoader<'info, Withdrawal>,
    /// Market token.
    #[account(
        mut,
        constraint = withdrawal.load()?.tokens.market_token() == market_token.key() @ CoreError::MarketTokenMintMismatched
    )]
    pub market_token: Box<Account<'info, Mint>>,
    /// Final long token.
    #[account(constraint = withdrawal.load()?.tokens.final_long_token() == final_long_token.key() @ CoreError::TokenMintMismatched)]
    pub final_long_token: Box<Account<'info, Mint>>,
    /// Final short token.
    #[account(constraint = withdrawal.load()?.tokens.final_long_token() == final_long_token.key() @ CoreError::TokenMintMismatched)]
    pub final_short_token: Box<Account<'info, Mint>>,
    /// The escrow account for receving market tokens to burn.
    #[account(
        mut,
        associated_token::mint = market_token,
        associated_token::authority = withdrawal,
    )]
    pub market_token_escrow: Box<Account<'info, TokenAccount>>,
    /// The escrow account for receiving withdrawed final long tokens.
    #[account(
        mut,
        associated_token::mint = final_long_token,
        associated_token::authority = withdrawal,
    )]
    pub final_long_token_escrow: Box<Account<'info, TokenAccount>>,
    /// The escrow account for receiving withdrawed final short tokens.
    #[account(
        mut,
        associated_token::mint = final_short_token,
        associated_token::authority = withdrawal,
    )]
    pub final_short_token_escrow: Box<Account<'info, TokenAccount>>,
    /// Market token vault.
    #[account(
        mut,
        token::mint = market_token,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            market_token_vault.mint.as_ref(),
            &[],
        ],
        bump,
    )]
    pub market_token_vault: Box<Account<'info, TokenAccount>>,
    /// Final long token vault.
    #[account(
        mut,
        token::mint = final_long_token,
        token::authority = store,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            final_long_token_vault.mint.as_ref(),
            &[],
        ],
        bump,
    )]
    pub final_long_token_vault: Box<Account<'info, TokenAccount>>,
    /// Final short token vault.
    #[account(
        mut,
        token::mint = final_short_token,
        token::authority = store,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            final_short_token_vault.mint.as_ref(),
            &[],
        ],
        bump,
    )]
    pub final_short_token_vault: Box<Account<'info, TokenAccount>>,
    /// The token program.
    pub token_program: Program<'info, Token>,
    /// The system program.
    pub system_program: Program<'info, System>,
    /// Chainlink Program.
    pub chainlink_program: Option<Program<'info, Chainlink>>,
}

/// CHECK only ORDER_KEEPER can invoke this instruction.
pub(crate) fn unchecked_execute_withdrawal<'info>(
    ctx: Context<'_, '_, 'info, 'info, ExecuteWithdrawal<'info>>,
    execution_fee: u64,
    throw_on_execution_error: bool,
) -> Result<()> {
    let accounts = ctx.accounts;
    let remaining_accounts = ctx.remaining_accounts;
    let signer = accounts.withdrawal.load()?.signer();

    accounts.transfer_market_tokens_in(&signer)?;

    let executed = accounts.perform_execution(remaining_accounts, throw_on_execution_error)?;

    match executed {
        Some((final_long_token_amount, final_short_token_amount)) => {
            accounts.withdrawal.load_mut()?.header.completed()?;
            accounts.transfer_tokens_out(
                remaining_accounts,
                final_long_token_amount,
                final_short_token_amount,
            )?;
        }
        None => {
            accounts.withdrawal.load_mut()?.header.cancelled()?;
            accounts.transfer_market_tokens_out()?;
        }
    }

    // Is must be placed at the end to be executed correctly.
    accounts.pay_execution_fee(execution_fee)?;

    Ok(())
}

impl<'info> internal::Authentication<'info> for ExecuteWithdrawal<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

impl<'info> ExecuteWithdrawal<'info> {
    fn perform_execution(
        &mut self,
        remaining_accounts: &'info [AccountInfo<'info>],
        throw_on_execution_error: bool,
    ) -> Result<Option<(u64, u64)>> {
        // FIXME: We only need the tokens here, the feeds are not necessary.
        let feeds = self
            .withdrawal
            .load()?
            .swap()
            .to_feeds(&self.token_map.load_token_map()?)?;

        let op = ExecuteWithdrawalOperation::builder()
            .store(&self.store)
            .market(&self.market)
            .withdrawal(&self.withdrawal)
            .market_token_mint(&mut self.market_token)
            .market_token_vault(self.market_token_vault.to_account_info())
            .token_program(self.token_program.to_account_info())
            .throw_on_execution_error(throw_on_execution_error);

        let executed = self.oracle.load_mut()?.with_prices(
            &self.store,
            &self.token_map,
            &feeds.tokens,
            remaining_accounts,
            self.chainlink_program.as_ref(),
            |oracle, remaining_accounts| {
                op.oracle(oracle)
                    .remaining_accounts(remaining_accounts)
                    .build()
                    .execute()
            },
        )?;

        Ok(executed)
    }

    fn transfer_market_tokens_in(&self, signer: &ActionSigner) -> Result<()> {
        let seeds = signer.as_seeds();

        transfer_checked(
            CpiContext::new(
                self.token_program.to_account_info(),
                TransferChecked {
                    from: self.market_token_escrow.to_account_info(),
                    mint: self.market_token.to_account_info(),
                    to: self.market_token_vault.to_account_info(),
                    authority: self.withdrawal.to_account_info(),
                },
            )
            .with_signer(&[&seeds]),
            self.withdrawal.load()?.params.market_token_amount,
            self.market_token.decimals,
        )?;

        Ok(())
    }

    fn transfer_market_tokens_out(&self) -> Result<()> {
        use crate::internal::TransferUtils;

        let amount = self.withdrawal.load()?.params.market_token_amount;
        TransferUtils::new(
            self.token_program.to_account_info(),
            &self.store,
            self.market_token.to_account_info(),
        )
        .transfer_out(
            self.market_token_vault.to_account_info(),
            self.market_token_escrow.to_account_info(),
            amount,
            self.market_token.decimals,
        )?;

        Ok(())
    }

    fn transfer_tokens_out(
        &self,
        remaining_accounts: &'info [AccountInfo<'info>],
        final_long_token_amount: u64,
        final_short_token_amount: u64,
    ) -> Result<()> {
        let builder = MarketTransferOutOperation::builder()
            .store(&self.store)
            .token_program(self.token_program.to_account_info());
        let store = &self.store.key();

        if final_long_token_amount != 0 {
            let market = self
                .withdrawal
                .load()?
                .swap
                .find_and_unpack_last_market(store, true, remaining_accounts)?
                .unwrap_or(self.market.clone());
            let vault = &self.final_long_token_vault;
            let escrow = &self.final_long_token_escrow;
            let token = &self.final_long_token;
            builder
                .clone()
                .market(&market)
                .to(escrow.to_account_info())
                .vault(vault.to_account_info())
                .amount(final_long_token_amount)
                .decimals(token.decimals)
                .token_mint(token.to_account_info())
                .build()
                .execute()?;
        }

        if final_short_token_amount != 0 {
            let market = self
                .withdrawal
                .load()?
                .swap
                .find_and_unpack_last_market(store, false, remaining_accounts)?
                .unwrap_or(self.market.clone());
            let vault = &self.final_short_token_vault;
            let escrow = &self.final_short_token_escrow;
            let token = &self.final_short_token;
            builder
                .market(&market)
                .to(escrow.to_account_info())
                .vault(vault.to_account_info())
                .amount(final_short_token_amount)
                .decimals(token.decimals)
                .token_mint(token.to_account_info())
                .build()
                .execute()?;
        }
        Ok(())
    }

    fn pay_execution_fee(&self, execution_fee: u64) -> Result<()> {
        let execution_lamports = self.withdrawal.load()?.execution_lamports(execution_fee);
        PayExecutionFeeOperation::builder()
            .payer(self.withdrawal.to_account_info())
            .receiver(self.authority.to_account_info())
            .execution_lamports(execution_lamports)
            .build()
            .execute()?;
        Ok(())
    }
}
