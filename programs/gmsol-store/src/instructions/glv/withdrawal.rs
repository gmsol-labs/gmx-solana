use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
    token_2022::Token2022,
    token_interface,
};
use gmsol_utils::InitSpace;

use crate::{
    constants,
    events::RemoveGlvWithdrawalEvent,
    ops::{
        execution_fee::{PayExecutionFeeOperation, TransferExecutionFeeOperation},
        glv::{
            CreateGlvWithdrawalOperation, CreateGlvWithdrawalParams, ExecuteGlvWithdrawalOperation,
        },
        market::MarketTransferOutOperation,
    },
    states::{
        common::action::ActionExt,
        glv::{GlvWithdrawal, SplitAccountsForGlv},
        Glv, Market, NonceBytes, Oracle, PriceProvider, RoleKey, Seed, Store, TokenMapHeader,
        TokenMapLoader,
    },
    utils::{
        internal::{self, Authentication},
        token::{is_associated_token_account, is_associated_token_account_with_program_id},
    },
    CoreError,
};

/// The accounts defintion for [`create_glv_withdrawal`] instruction.
#[derive(Accounts)]
#[instruction(nonce: [u8; 32])]
pub struct CreateGlvWithdrawal<'info> {
    /// Owner.
    #[account(mut)]
    pub owner: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// Market.
    #[account(
        mut,
        has_one = store,
        constraint = market.load()?.meta().market_token_mint == market_token.key() @ CoreError::MarketTokenMintMismatched,
    )]
    pub market: AccountLoader<'info, Market>,
    /// GLV.
    #[account(
        has_one = store,
        constraint = glv.load()?.glv_token == glv_token.key() @ CoreError::TokenMintMismatched,
        constraint = glv.load()?.contains(&market_token.key()) @ CoreError::InvalidArgument,
    )]
    pub glv: AccountLoader<'info, Glv>,
    /// GLV withdrawal.
    #[account(
        init,
        payer = owner,
        space = 8 + GlvWithdrawal::INIT_SPACE,
        seeds = [GlvWithdrawal::SEED, store.key().as_ref(), owner.key().as_ref(), &nonce],
        bump,
    )]
    pub glv_withdrawal: AccountLoader<'info, GlvWithdrawal>,
    /// GLV Token.
    pub glv_token: Box<InterfaceAccount<'info, token_interface::Mint>>,
    /// Market token.
    pub market_token: Box<Account<'info, Mint>>,
    /// Final long token.
    pub final_long_token: Box<Account<'info, Mint>>,
    /// Final short token.
    pub final_short_token: Box<Account<'info, Mint>>,
    /// The source GLV token account.
    #[account(mut, token::mint = glv_token)]
    pub glv_token_source: Box<InterfaceAccount<'info, token_interface::TokenAccount>>,
    /// The escrow account for GLV tokens.
    #[account(
        mut,
        associated_token::mint = glv_token,
        associated_token::authority = glv_withdrawal,
        associated_token::token_program = glv_token_program,
    )]
    pub glv_token_escrow: Box<InterfaceAccount<'info, token_interface::TokenAccount>>,
    /// The escrow account for market tokens.
    #[account(
        mut,
        associated_token::mint = market_token,
        associated_token::authority = glv_withdrawal,
    )]
    pub market_token_escrow: Box<Account<'info, TokenAccount>>,
    /// The escrow account for long tokens.
    #[account(
        mut,
        associated_token::mint = final_long_token,
        associated_token::authority = glv_withdrawal,
    )]
    pub final_long_token_escrow: Box<Account<'info, TokenAccount>>,
    /// The escrow account for short tokens.
    #[account(
        mut,
        associated_token::mint = final_short_token,
        associated_token::authority = glv_withdrawal,
    )]
    pub final_short_token_escrow: Box<Account<'info, TokenAccount>>,
    /// The system program.
    pub system_program: Program<'info, System>,
    /// The token program.
    pub token_program: Program<'info, Token>,
    /// The token program for GLV token.
    pub glv_token_program: Program<'info, Token2022>,
    /// The associated token program.
    pub associated_token_program: Program<'info, AssociatedToken>,
}

pub(crate) fn create_glv_withdrawal<'info>(
    ctx: Context<'_, '_, 'info, 'info, CreateGlvWithdrawal<'info>>,
    nonce: &NonceBytes,
    params: &CreateGlvWithdrawalParams,
) -> Result<()> {
    let accounts = ctx.accounts;
    accounts.transfer_execution_lamports(params)?;
    accounts.transfer_glv_tokens(params)?;
    CreateGlvWithdrawalOperation::builder()
        .glv_withdrawal(accounts.glv_withdrawal.clone())
        .market(accounts.market.clone())
        .store(accounts.store.clone())
        .owner(&accounts.owner)
        .nonce(nonce)
        .bump(ctx.bumps.glv_withdrawal)
        .final_long_token(&accounts.final_long_token_escrow)
        .final_short_token(&accounts.final_short_token_escrow)
        .market_token(&accounts.market_token_escrow)
        .glv_token(&accounts.glv_token_escrow)
        .params(params)
        .swap_paths(ctx.remaining_accounts)
        .build()
        .unchecked_execute()?;
    Ok(())
}

impl<'info> CreateGlvWithdrawal<'info> {
    fn transfer_execution_lamports(&self, params: &CreateGlvWithdrawalParams) -> Result<()> {
        TransferExecutionFeeOperation::builder()
            .payment(self.glv_withdrawal.to_account_info())
            .payer(self.owner.to_account_info())
            .execution_lamports(params.execution_lamports)
            .system_program(self.system_program.to_account_info())
            .build()
            .execute()
    }

    fn transfer_glv_tokens(&mut self, params: &CreateGlvWithdrawalParams) -> Result<()> {
        use anchor_spl::token_interface::{transfer_checked, TransferChecked};

        let amount = params.glv_token_amount;
        require!(amount != 0, CoreError::EmptyGlvWithdrawal);

        let source = &self.glv_token_source;
        let target = &mut self.glv_token_escrow;
        let mint = &self.glv_token;

        transfer_checked(
            CpiContext::new(
                self.glv_token_program.to_account_info(),
                TransferChecked {
                    from: source.to_account_info(),
                    mint: mint.to_account_info(),
                    to: target.to_account_info(),
                    authority: self.owner.to_account_info(),
                },
            ),
            amount,
            mint.decimals,
        )?;

        target.reload()?;

        Ok(())
    }
}

/// The accounts defintion for `close_glv_withdrawal` instruction.
#[event_cpi]
#[derive(Accounts)]
pub struct CloseGlvWithdrawal<'info> {
    /// The executor of this instruction.
    pub executor: Signer<'info>,
    /// The store.
    pub store: AccountLoader<'info, Store>,
    /// The owner of the deposit.
    /// CHECK: only use to validate and receive fund.
    #[account(mut)]
    pub owner: UncheckedAccount<'info>,
    /// The GLV withdrawal to close.
    #[account(
        mut,
        constraint = glv_withdrawal.load()?.header.owner == owner.key() @ CoreError::OwnerMismatched,
        constraint = glv_withdrawal.load()?.header.store == store.key() @ CoreError::StoreMismatched,
        constraint = glv_withdrawal.load()?.tokens.market_token_account() == market_token_escrow.key() @ CoreError::MarketTokenAccountMismatched,
        constraint = glv_withdrawal.load()?.tokens.glv_token_account() == glv_token_escrow.key() @ CoreError::MarketTokenAccountMismatched,
        constraint = glv_withdrawal.load()?.tokens.final_long_token_account() == final_long_token_escrow.key() @ CoreError::TokenAccountMismatched,
        constraint = glv_withdrawal.load()?.tokens.final_short_token_account() == final_short_token_escrow.key() @ CoreError::TokenAccountMismatched,
        seeds = [GlvWithdrawal::SEED, store.key().as_ref(), owner.key().as_ref(), &glv_withdrawal.load()?.header.nonce],
        bump = glv_withdrawal.load()?.header.bump,
    )]
    pub glv_withdrawal: AccountLoader<'info, GlvWithdrawal>,
    /// Market token.
    #[account(
        constraint = glv_withdrawal.load()?.tokens.market_token() == market_token.key() @ CoreError::MarketTokenMintMismatched
    )]
    pub market_token: Box<Account<'info, Mint>>,
    /// Final long token.
    #[account(
        constraint = glv_withdrawal.load()?.tokens.final_long_token() == final_long_token.key() @ CoreError::TokenMintMismatched
    )]
    pub final_long_token: Box<Account<'info, Mint>>,
    /// Final short token.
    #[account(
        constraint = glv_withdrawal.load()?.tokens.final_short_token() == final_short_token.key() @ CoreError::TokenMintMismatched
    )]
    pub final_short_token: Box<Account<'info, Mint>>,
    /// GLV token.
    #[account(
        constraint = glv_withdrawal.load()?.tokens.glv_token() == glv_token.key() @ CoreError::TokenMintMismatched
    )]
    pub glv_token: Box<InterfaceAccount<'info, token_interface::Mint>>,
    /// The escrow account for market tokens.
    #[account(
        mut,
        associated_token::mint = market_token,
        associated_token::authority = glv_withdrawal,
    )]
    pub market_token_escrow: Box<Account<'info, TokenAccount>>,
    /// The escrow account for receiving initial long token for deposit.
    #[account(
        mut,
        associated_token::mint = final_long_token,
        associated_token::authority = glv_withdrawal,
    )]
    pub final_long_token_escrow: Box<Account<'info, TokenAccount>>,
    /// The escrow account for receiving final short token for deposit.
    #[account(
        mut,
        associated_token::mint = final_short_token,
        associated_token::authority = glv_withdrawal,
    )]
    pub final_short_token_escrow: Box<Account<'info, TokenAccount>>,
    /// The ATA for market token of owner.
    /// CHECK: should be checked during the execution.
    #[account(
        mut,
        constraint = is_associated_token_account(market_token_ata.key, owner.key, &market_token.key()) @ CoreError::NotAnATA,
    )]
    pub market_token_ata: UncheckedAccount<'info>,
    /// The ATA for final long token of owner.
    /// CHECK: should be checked during the execution
    #[account(
        mut,
        constraint = is_associated_token_account(final_long_token_ata.key, owner.key, &final_long_token.as_ref().key()) @ CoreError::NotAnATA,
    )]
    pub final_long_token_ata: UncheckedAccount<'info>,
    /// The ATA for final short token of owner.
    /// CHECK: should be checked during the execution
    #[account(
        mut,
        constraint = is_associated_token_account(final_short_token_ata.key, owner.key, &final_short_token.as_ref().key()) @ CoreError::NotAnATA,
    )]
    pub final_short_token_ata: UncheckedAccount<'info>,
    /// The escrow account for GLV tokens.
    #[account(
        mut,
        associated_token::mint = glv_token,
        associated_token::authority = glv_withdrawal,
        associated_token::token_program = glv_token_program,
    )]
    pub glv_token_escrow: Box<InterfaceAccount<'info, token_interface::TokenAccount>>,
    /// The ATA for GLV token of owner.
    /// CHECK: should be checked during the execution.
    #[account(
        mut,
        constraint = is_associated_token_account_with_program_id(glv_token_ata.key, owner.key, &glv_token.key(), &glv_token_program.key()) @ CoreError::NotAnATA,
    )]
    pub glv_token_ata: UncheckedAccount<'info>,
    /// The system program.
    pub system_program: Program<'info, System>,
    /// The token program.
    pub token_program: Program<'info, Token>,
    /// Token program for GLV token.
    pub glv_token_program: Program<'info, Token2022>,
    /// The associated token program.
    pub associated_token_program: Program<'info, AssociatedToken>,
}

pub(crate) fn close_glv_withdrawal(ctx: Context<CloseGlvWithdrawal>, reason: &str) -> Result<()> {
    let accounts = &ctx.accounts;
    let should_continue_when_atas_are_missing = accounts.preprocess()?;
    if accounts.transfer_to_atas(should_continue_when_atas_are_missing)? {
        {
            let glv_withdrawal_address = accounts.glv_withdrawal.key();
            let glv_withdrawal = accounts.glv_withdrawal.load()?;
            emit_cpi!(RemoveGlvWithdrawalEvent::new(
                glv_withdrawal.header.id,
                glv_withdrawal.header.store,
                glv_withdrawal_address,
                glv_withdrawal.tokens.market_token(),
                glv_withdrawal.tokens.glv_token(),
                glv_withdrawal.header.owner,
                glv_withdrawal.header.action_state()?,
                reason,
            )?);
        }
        accounts.close()?;
    } else {
        msg!("Some ATAs are not initilaized, skip the close");
    }
    Ok(())
}

impl<'info> internal::Authentication<'info> for CloseGlvWithdrawal<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.executor
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

type ShouldContinueWhenATAsAreMissing = bool;
type Success = bool;

impl<'info> CloseGlvWithdrawal<'info> {
    fn preprocess(&self) -> Result<ShouldContinueWhenATAsAreMissing> {
        if self.executor.key == self.owner.key {
            Ok(true)
        } else {
            self.only_role(RoleKey::ORDER_KEEPER)?;
            {
                let glv_withdrawal = self.glv_withdrawal.load()?;
                if glv_withdrawal
                    .header
                    .action_state()?
                    .is_completed_or_cancelled()
                {
                    Ok(false)
                } else {
                    err!(CoreError::PermissionDenied)
                }
            }
        }
    }

    fn close(&self) -> Result<()> {
        self.glv_withdrawal.close(self.owner.to_account_info())?;
        Ok(())
    }

    fn transfer_to_atas(&self, init_if_needed: bool) -> Result<Success> {
        use crate::utils::token::TransferAllFromEscrowToATA;

        // Prepare signer seeds.
        let signer = self.glv_withdrawal.load()?.signer();
        let seeds = signer.as_seeds();

        let builder = TransferAllFromEscrowToATA::builder()
            .system_program(self.system_program.to_account_info())
            .associated_token_program(self.associated_token_program.to_account_info())
            .payer(self.executor.to_account_info())
            .owner(self.owner.to_account_info())
            .escrow_authority(self.glv_withdrawal.to_account_info())
            .seeds(&seeds)
            .init_if_needed(init_if_needed);

        // Transfer market tokens.
        if !builder
            .clone()
            .token_program(self.token_program.to_account_info())
            .mint(self.market_token.to_account_info())
            .decimals(self.market_token.decimals)
            .ata(self.market_token_ata.to_account_info())
            .escrow(self.market_token_escrow.to_account_info())
            .build()
            .execute()?
        {
            return Ok(false);
        }

        // Transfer GLV tokens.
        if !builder
            .clone()
            .token_program(self.glv_token_program.to_account_info())
            .mint(self.glv_token.to_account_info())
            .decimals(self.glv_token.decimals)
            .ata(self.glv_token_ata.to_account_info())
            .escrow(self.glv_token_escrow.to_account_info())
            .build()
            .execute()?
        {
            return Ok(false);
        }

        // Prevent closing the same token accounts.
        let (final_long_token_escrow, final_short_token_escrow) =
            if self.final_long_token_escrow.key() == self.final_short_token_escrow.key() {
                (Some(&self.final_long_token_escrow), None)
            } else {
                (
                    Some(&self.final_long_token_escrow),
                    Some(&self.final_short_token_escrow),
                )
            };

        // Transfer final long tokens.
        if let Some(escrow) = final_long_token_escrow.as_ref() {
            let ata = &self.final_long_token_ata;
            let mint = &self.final_long_token;
            if !builder
                .clone()
                .token_program(self.token_program.to_account_info())
                .mint(mint.to_account_info())
                .decimals(mint.decimals)
                .ata(ata.to_account_info())
                .escrow(escrow.to_account_info())
                .build()
                .execute()?
            {
                return Ok(false);
            }
        }

        // Transfer final short tokens.
        if let Some(escrow) = final_short_token_escrow.as_ref() {
            let ata = &self.final_short_token_ata;
            let mint = &self.final_short_token;
            if !builder
                .clone()
                .token_program(self.token_program.to_account_info())
                .mint(mint.to_account_info())
                .decimals(mint.decimals)
                .ata(ata.to_account_info())
                .escrow(escrow.to_account_info())
                .build()
                .execute()?
            {
                return Ok(false);
            }
        }

        Ok(true)
    }
}

/// The accounts definition for `execute_glv_withdrawal` instruction.
///
/// Remaining accounts expected by this instruction:
///
///   - 0..N. `[]` N market accounts, where N represents the total number of markets managed
///     by the given GLV.
///   - N..2N. `[]` N market token accounts (see above for the definition of N).
///   - 2N..3N. `[]` N market token vault accounts (see above for the definition of N).
///   - 3N..3N+M. `[]` M feed accounts, where M represents the total number of tokens in the
///     swap params.
///   - 3N+M..3N+M+L. `[writable]` L market accounts, where L represents the total number of unique
///     markets excluding the current market in the swap params.
#[derive(Accounts)]
pub struct ExecuteGlvWithdrawal<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    #[account(has_one = token_map)]
    pub store: AccountLoader<'info, Store>,
    /// Token Map.
    #[account(has_one = store)]
    pub token_map: AccountLoader<'info, TokenMapHeader>,
    /// Price Provider.
    pub price_provider: Interface<'info, PriceProvider>,
    /// Oracle buffer to use.
    #[account(has_one = store)]
    pub oracle: Box<Account<'info, Oracle>>,
    /// GLV account.
    #[account(
        has_one = store,
        constraint = glv.load()?.contains(&market_token.key()) @ CoreError::InvalidArgument,
    )]
    pub glv: AccountLoader<'info, Glv>,
    /// Market.
    #[account(mut, has_one = store)]
    pub market: AccountLoader<'info, Market>,
    /// The GLV withdrawal to execute.
    #[account(
        mut,
        constraint = glv_withdrawal.load()?.header.store == store.key() @ CoreError::StoreMismatched,
        constraint = glv_withdrawal.load()?.header.market == market.key() @ CoreError::MarketMismatched,
        constraint = glv_withdrawal.load()?.tokens.glv_token() == glv_token.key() @ CoreError::TokenMintMismatched,
        constraint = glv_withdrawal.load()?.tokens.market_token() == market_token.key() @ CoreError::MarketTokenMintMismatched,
        constraint = glv_withdrawal.load()?.tokens.market_token_account() == market_token_escrow.key() @ CoreError::MarketTokenAccountMismatched,
        constraint = glv_withdrawal.load()?.tokens.glv_token_account() == glv_token_escrow.key() @ CoreError::MarketTokenAccountMismatched,
        constraint = glv_withdrawal.load()?.tokens.final_long_token_account() == final_long_token_escrow.key() @ CoreError::TokenAccountMismatched,
        constraint = glv_withdrawal.load()?.tokens.final_short_token_account() == final_short_token_escrow.key() @ CoreError::TokenAccountMismatched,
        seeds = [GlvWithdrawal::SEED, store.key().as_ref(), glv_withdrawal.load()?.header.owner.as_ref(), &glv_withdrawal.load()?.header.nonce],
        bump = glv_withdrawal.load()?.header.bump,
    )]
    pub glv_withdrawal: AccountLoader<'info, GlvWithdrawal>,
    /// GLV token mint.
    #[account(mut, constraint = glv.load()?.glv_token == glv_token.key() @ CoreError::TokenMintMismatched)]
    pub glv_token: Box<InterfaceAccount<'info, token_interface::Mint>>,
    /// Market token mint.
    #[account(mut, constraint = market.load()?.meta().market_token_mint == market_token.key() @ CoreError::MarketTokenMintMismatched)]
    pub market_token: Box<Account<'info, Mint>>,
    /// Final long token.
    #[account(
        constraint = glv_withdrawal.load()?.tokens.final_long_token() == final_long_token.key() @ CoreError::TokenMintMismatched
    )]
    pub final_long_token: Box<Account<'info, Mint>>,
    /// Final short token.
    #[account(
        constraint = glv_withdrawal.load()?.tokens.final_short_token() == final_short_token.key() @ CoreError::TokenMintMismatched
    )]
    pub final_short_token: Box<Account<'info, Mint>>,
    /// The escrow account for GLV tokens.
    #[account(
        mut,
        associated_token::mint = glv_token,
        associated_token::authority = glv_withdrawal,
        associated_token::token_program = glv_token_program,
    )]
    pub glv_token_escrow: Box<InterfaceAccount<'info, token_interface::TokenAccount>>,
    /// The escrow account for market tokens.
    #[account(
        mut,
        associated_token::mint = market_token,
        associated_token::authority = glv_withdrawal,
    )]
    pub market_token_escrow: Box<Account<'info, TokenAccount>>,
    /// The escrow account for receiving final long token for withdrawal.
    #[account(
        mut,
        associated_token::mint = final_long_token,
        associated_token::authority = glv_withdrawal,
    )]
    pub final_long_token_escrow: Box<Account<'info, TokenAccount>>,
    /// The escrow account for receiving final short token for withdrawal.
    #[account(
        mut,
        associated_token::mint = final_short_token,
        associated_token::authority = glv_withdrawal,
    )]
    pub final_short_token_escrow: Box<Account<'info, TokenAccount>>,
    /// Market token wihtdrawal vault.
    #[account(
        mut,
        token::mint = market_token,
        token::authority = store,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            market_token_withdrawal_vault.mint.as_ref(),
            &[],
        ],
        bump,
    )]
    pub market_token_withdrawal_vault: Box<Account<'info, TokenAccount>>,
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
    /// Market token vault for the GLV.
    #[account(
        mut,
        associated_token::mint = market_token,
        associated_token::authority = glv,
    )]
    pub market_token_vault: Box<Account<'info, TokenAccount>>,
    /// The token program.
    pub token_program: Program<'info, Token>,
    /// The token program for GLV token.
    pub glv_token_program: Program<'info, Token2022>,
    /// The system program.
    pub system_program: Program<'info, System>,
}

/// Execute GLV withdrawal.
///
/// # CHECK
/// - Only ORDER_KEEPER is allowed to call this function.
pub(crate) fn unchecked_execute_glv_withdrawal<'info>(
    ctx: Context<'_, '_, 'info, 'info, ExecuteGlvWithdrawal<'info>>,
    execution_lamports: u64,
    throw_on_execution_error: bool,
) -> Result<()> {
    let accounts = ctx.accounts;
    let remaining_accounts = ctx.remaining_accounts;

    let glv_address = accounts.glv.key();

    let splitted = {
        let glv_withdrawal = accounts.glv_withdrawal.load()?;
        let token_map = accounts.token_map.load_token_map()?;
        accounts.glv.load()?.validate_and_split_remaining_accounts(
            &glv_address,
            &accounts.store.key(),
            accounts.token_program.key,
            remaining_accounts,
            Some(&*glv_withdrawal),
            &token_map,
        )?
    };

    let executed = accounts.perform_execution(&splitted, throw_on_execution_error)?;

    match executed {
        Some((final_long_token_amount, final_short_token_amount)) => {
            accounts.glv_withdrawal.load_mut()?.header.completed()?;
            accounts.transfer_tokens_out(
                splitted.remaining_accounts,
                final_long_token_amount,
                final_short_token_amount,
            )?;
        }
        None => {
            accounts.glv_withdrawal.load_mut()?.header.cancelled()?;
        }
    }

    // It must be placed at the end to be executed correctly.
    accounts.pay_execution_fee(execution_lamports)?;

    Ok(())
}

impl<'info> internal::Authentication<'info> for ExecuteGlvWithdrawal<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

impl<'info> ExecuteGlvWithdrawal<'info> {
    #[inline(never)]
    fn pay_execution_fee(&self, execution_fee: u64) -> Result<()> {
        let execution_lamports = self
            .glv_withdrawal
            .load()?
            .execution_lamports(execution_fee);
        PayExecutionFeeOperation::builder()
            .payer(self.glv_withdrawal.to_account_info())
            .receiver(self.authority.to_account_info())
            .execution_lamports(execution_lamports)
            .build()
            .execute()?;
        Ok(())
    }

    #[inline(never)]
    fn perform_execution(
        &mut self,
        splitted: &SplitAccountsForGlv<'info>,
        throw_on_execution_error: bool,
    ) -> Result<Option<(u64, u64)>> {
        let builder = ExecuteGlvWithdrawalOperation::builder()
            .glv_withdrawal(self.glv_withdrawal.clone())
            .token_program(self.token_program.to_account_info())
            .glv_token_program(self.glv_token_program.to_account_info())
            .throw_on_execution_error(throw_on_execution_error)
            .store(self.store.clone())
            .glv(&self.glv)
            .glv_token_mint(&mut self.glv_token)
            .glv_token_account(self.glv_token_escrow.to_account_info())
            .market(self.market.clone())
            .market_token_mint(&mut self.market_token)
            .market_token_glv_vault(&self.market_token_vault)
            .market_token_withdrawal_vault(self.market_token_withdrawal_vault.to_account_info())
            .markets(splitted.markets)
            .market_tokens(splitted.market_tokens)
            .market_token_vaults(splitted.market_token_vaults);

        self.oracle.with_prices(
            &self.store,
            &self.price_provider,
            &self.token_map,
            &splitted.tokens,
            splitted.remaining_accounts,
            |oracle, remaining_accounts| {
                builder
                    .oracle(oracle)
                    .remaining_accounts(remaining_accounts)
                    .build()
                    .unchecked_execute()
            },
        )
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
                .glv_withdrawal
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
                .glv_withdrawal
                .load()?
                .swap
                .find_and_unpack_last_market(store, true, remaining_accounts)?
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
}
