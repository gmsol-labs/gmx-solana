use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};

use crate::{
    constants,
    events::EventEmitter,
    ops::{
        deposit::ExecuteDepositOperation,
        execution_fee::PayExecutionFeeOperation,
        market::{MarketTransferInOperation, MarketTransferOutOperation},
    },
    states::{
        common::{
            action::{ActionExt, ActionSigner},
            swap::SwapActionParamsExt,
        },
        feature::{ActionDisabledFlag, DomainDisabledFlag},
        Chainlink, Deposit, Market, Oracle, Seed, Store, TokenMapHeader, TokenMapLoader,
    },
    utils::internal,
    CoreError,
};

/// The accounts definition for [`execute_deposit`](crate::gmsol_store::execute_deposit) instruction.
///
/// Remaining accounts expected by this instruction:
///
///   - 0..M. `[]` M feed accounts, where M represents the total number of tokens in the
///     swap params.
///   - M..M+N. `[writable]` N market accounts, where N represents the total number of unique
///     markets excluding the current market in the swap params.
#[event_cpi]
#[derive(Accounts)]
pub struct ExecuteDeposit<'info> {
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
    /// Market.
    #[account(mut, has_one = store)]
    pub market: AccountLoader<'info, Market>,
    /// The deposit to execute.
    #[account(
        mut,
        constraint = deposit.load()?.header.market == market.key() @ CoreError::MarketMismatched,
        constraint = deposit.load()?.header.store == store.key() @ CoreError::StoreMismatched,
        constraint = deposit.load()?.tokens.market_token.account().expect("must exist") == market_token_escrow.key() @ CoreError::MarketTokenAccountMismatched,
        constraint = deposit.load()?.tokens.initial_long_token.account() == initial_long_token_escrow.as_ref().map(|a| a.key()) @ CoreError::TokenAccountMismatched,
        constraint = deposit.load()?.tokens.initial_short_token.account() == initial_short_token_escrow.as_ref().map(|a| a.key()) @ CoreError::TokenAccountMismatched,
        seeds = [Deposit::SEED, store.key().as_ref(), deposit.load()?.header.owner.as_ref(), &deposit.load()?.header.nonce],
        bump = deposit.load()?.header.bump,
    )]
    pub deposit: AccountLoader<'info, Deposit>,
    /// Market token mint.
    #[account(mut, constraint = market.load()?.meta().market_token_mint == market_token.key() @ CoreError::MarketTokenMintMismatched)]
    pub market_token: Box<Account<'info, Mint>>,
    /// Initial long token.
    #[account(
        constraint = deposit.load()?.tokens.initial_long_token.token().map(|token| initial_long_token.key() == token).unwrap_or(true) @ CoreError::TokenMintMismatched
    )]
    pub initial_long_token: Option<Box<Account<'info, Mint>>>,
    /// Initial short token.
    #[account(
        constraint = deposit.load()?.tokens.initial_short_token.token().map(|token| initial_short_token.key() == token).unwrap_or(true) @ CoreError::TokenMintMismatched
    )]
    pub initial_short_token: Option<Box<Account<'info, Mint>>>,
    /// The escrow account for receving market tokens.
    #[account(
        mut,
        associated_token::mint = market_token,
        associated_token::authority = deposit,
    )]
    pub market_token_escrow: Box<Account<'info, TokenAccount>>,
    /// The escrow account for receiving initial long token for deposit.
    #[account(
        mut,
        associated_token::mint = initial_long_token,
        associated_token::authority = deposit,
    )]
    pub initial_long_token_escrow: Option<Box<Account<'info, TokenAccount>>>,
    /// The escrow account for receiving initial short token for deposit.
    #[account(
        mut,
        associated_token::mint = initial_short_token,
        associated_token::authority = deposit,
    )]
    pub initial_short_token_escrow: Option<Box<Account<'info, TokenAccount>>>,
    /// Initial long token vault.
    #[account(
        mut,
        token::mint = initial_long_token,
        token::authority = store,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            initial_long_token_vault.mint.as_ref(),
        ],
        bump,
    )]
    pub initial_long_token_vault: Option<Box<Account<'info, TokenAccount>>>,
    /// Initial short token vault.
    #[account(
        mut,
        token::mint = initial_short_token,
        token::authority = store,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            initial_short_token_vault.mint.as_ref(),
        ],
        bump,
    )]
    pub initial_short_token_vault: Option<Box<Account<'info, TokenAccount>>>,
    /// The token program.
    pub token_program: Program<'info, Token>,
    /// The system program.
    pub system_program: Program<'info, System>,
    /// Chainlink Program.
    pub chainlink_program: Option<Program<'info, Chainlink>>,
}

/// CHECK: only ORDER_KEEPER can invoke this instruction.
#[inline(never)]
pub(crate) fn unchecked_execute_deposit<'info>(
    ctx: Context<'_, '_, 'info, 'info, ExecuteDeposit<'info>>,
    execution_fee: u64,
    throw_on_execution_error: bool,
) -> Result<()> {
    let accounts = ctx.accounts;
    let remaining_accounts = ctx.remaining_accounts;

    // Validate feature enabled.
    accounts
        .store
        .load()?
        .validate_feature_enabled(DomainDisabledFlag::Deposit, ActionDisabledFlag::Execute)?;

    let signer = accounts.deposit.load()?.signer();

    let event_authority = accounts.event_authority.clone();
    let event_emitter = EventEmitter::new(&event_authority, ctx.bumps.event_authority);

    accounts.transfer_tokens_in(&signer, remaining_accounts, &event_emitter)?;

    let executed =
        accounts.perform_execution(remaining_accounts, throw_on_execution_error, &event_emitter)?;

    if executed {
        accounts.deposit.load_mut()?.header.completed()?;
    } else {
        accounts.deposit.load_mut()?.header.cancelled()?;
        accounts.transfer_tokens_out(remaining_accounts, &event_emitter)?;
    }

    // It must be placed at the end to be executed correctly.
    accounts.pay_execution_fee(execution_fee)?;

    Ok(())
}

impl<'info> internal::Authentication<'info> for ExecuteDeposit<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

impl<'info> ExecuteDeposit<'info> {
    #[inline(never)]
    fn pay_execution_fee(&self, execution_fee: u64) -> Result<()> {
        let execution_lamports = self.deposit.load()?.execution_lamports(execution_fee);
        PayExecutionFeeOperation::builder()
            .payer(self.deposit.to_account_info())
            .receiver(self.authority.to_account_info())
            .execution_lamports(execution_lamports)
            .build()
            .execute()?;
        Ok(())
    }

    #[inline(never)]
    fn transfer_tokens_in(
        &self,
        signer: &ActionSigner,
        remaining_accounts: &'info [AccountInfo<'info>],
        event_emitter: &EventEmitter<'_, 'info>,
    ) -> Result<()> {
        let seeds = signer.as_seeds();

        let builder = MarketTransferInOperation::builder()
            .store(&self.store)
            .from_authority(self.deposit.to_account_info())
            .token_program(self.token_program.to_account_info())
            .signer_seeds(&seeds)
            .event_emitter(*event_emitter);

        let store = &self.store.key();

        if let Some(escrow) = self.initial_long_token_escrow.as_ref() {
            let market = self
                .deposit
                .load()?
                .swap
                .find_and_unpack_first_market(store, true, remaining_accounts)?
                .unwrap_or(self.market.clone());
            let vault = self
                .initial_long_token_vault
                .as_ref()
                .ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
            builder
                .clone()
                .market(&market)
                .from(escrow.to_account_info())
                .vault(vault)
                .amount(self.deposit.load()?.params.initial_long_token_amount)
                .build()
                .execute()?;
        }

        if let Some(escrow) = self.initial_short_token_escrow.as_ref() {
            let market = self
                .deposit
                .load()?
                .swap
                .find_and_unpack_first_market(store, false, remaining_accounts)?
                .unwrap_or(self.market.clone());
            let vault = self
                .initial_short_token_vault
                .as_ref()
                .ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
            builder
                .clone()
                .market(&market)
                .from(escrow.to_account_info())
                .vault(vault)
                .amount(self.deposit.load()?.params.initial_short_token_amount)
                .build()
                .execute()?;
        }

        Ok(())
    }

    #[inline(never)]
    fn transfer_tokens_out(
        &self,
        remaining_accounts: &'info [AccountInfo<'info>],
        event_emitter: &EventEmitter<'_, 'info>,
    ) -> Result<()> {
        let builder = MarketTransferOutOperation::builder()
            .store(&self.store)
            .token_program(self.token_program.to_account_info())
            .event_emitter(*event_emitter);

        let store = &self.store.key();

        if let Some(escrow) = self.initial_long_token_escrow.as_ref() {
            let market = self
                .deposit
                .load()?
                .swap
                .find_and_unpack_first_market(store, true, remaining_accounts)?
                .unwrap_or(self.market.clone());
            let vault = self
                .initial_long_token_vault
                .as_ref()
                .ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
            let token = self
                .initial_long_token
                .as_ref()
                .ok_or_else(|| error!(CoreError::TokenMintNotProvided))?;
            builder
                .clone()
                .market(&market)
                .to(escrow.to_account_info())
                .vault(vault.to_account_info())
                .amount(self.deposit.load()?.params.initial_long_token_amount)
                .decimals(token.decimals)
                .token_mint(token.to_account_info())
                .build()
                .execute()?;
        }

        if let Some(escrow) = self.initial_short_token_escrow.as_ref() {
            let market = self
                .deposit
                .load()?
                .swap
                .find_and_unpack_first_market(store, false, remaining_accounts)?
                .unwrap_or(self.market.clone());
            let vault = self
                .initial_short_token_vault
                .as_ref()
                .ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
            let token = self
                .initial_short_token
                .as_ref()
                .ok_or_else(|| error!(CoreError::TokenMintNotProvided))?;
            builder
                .market(&market)
                .to(escrow.to_account_info())
                .vault(vault.to_account_info())
                .amount(self.deposit.load()?.params.initial_short_token_amount)
                .decimals(token.decimals)
                .token_mint(token.to_account_info())
                .build()
                .execute()?;
        }

        Ok(())
    }

    #[inline(never)]
    fn perform_execution(
        &mut self,
        remaining_accounts: &'info [AccountInfo<'info>],
        throw_on_execution_error: bool,
        event_emitter: &EventEmitter<'_, 'info>,
    ) -> Result<bool> {
        // Note: We only need the tokens here, the feeds are not necessary.
        let feeds = self
            .deposit
            .load()?
            .swap()
            .to_feeds(&self.token_map.load_token_map()?)
            .map_err(CoreError::from)?;
        let ops = ExecuteDepositOperation::builder()
            .store(&self.store)
            .market(&self.market)
            .deposit(&self.deposit)
            .market_token_mint(&mut self.market_token)
            .market_token_receiver(self.market_token_escrow.to_account_info())
            .token_program(self.token_program.to_account_info())
            .throw_on_execution_error(throw_on_execution_error)
            .event_emitter(*event_emitter);

        let executed = self.oracle.load_mut()?.with_prices(
            &self.store,
            &self.token_map,
            &feeds.tokens,
            remaining_accounts,
            self.chainlink_program.as_ref(),
            |oracle, remaining_accounts| {
                ops.oracle(oracle)
                    .remaining_accounts(remaining_accounts)
                    .build()
                    .execute()
            },
        )?;

        Ok(executed)
    }
}
