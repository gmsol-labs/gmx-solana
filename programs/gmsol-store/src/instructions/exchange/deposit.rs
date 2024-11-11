use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
};
use gmsol_utils::InitSpace;

use crate::{
    events::DepositCreated,
    ops::deposit::{CreateDepositOperation, CreateDepositParams},
    states::{common::action::ActionExt, Deposit, Market, NonceBytes, RoleKey, Seed, Store},
    utils::{internal, token::is_associated_token_account},
    CoreError,
};

/// The accounts definition for the [`create_deposit`](crate::gmsol_store::create_deposit)
/// instruction.
///
/// Remaining accounts expected by this instruction:
///
///   - 0..M. `[]` M market accounts, where M represents the length
///     of the swap path for initial long token.
///   - M..M+N. `[]` N market accounts, where N represents the length
///     of the swap path for initial short token.
#[derive(Accounts)]
#[instruction(nonce: [u8; 32])]
pub struct CreateDeposit<'info> {
    /// The owner.
    #[account(mut)]
    pub owner: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// Market.
    #[account(mut, has_one = store)]
    pub market: AccountLoader<'info, Market>,
    /// The deposit to be created.
    #[account(
        init,
        space = 8 + Deposit::INIT_SPACE,
        payer = owner,
        seeds = [Deposit::SEED, store.key().as_ref(), owner.key().as_ref(), &nonce],
        bump,
    )]
    pub deposit: AccountLoader<'info, Deposit>,
    /// Market token.
    #[account(constraint = market.load()?.meta().market_token_mint == market_token.key() @ CoreError::MarketTokenMintMismatched)]
    pub market_token: Box<Account<'info, Mint>>,
    /// Initial long token.
    pub initial_long_token: Option<Box<Account<'info, Mint>>>,
    /// initial short token.
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
    /// The ATA of the owner for receving market tokens.
    #[account(
        init_if_needed,
        payer = owner,
        associated_token::mint = market_token,
        associated_token::authority = owner,
    )]
    pub market_token_ata: Box<Account<'info, TokenAccount>>,
    /// The source initial long token account.
    #[account(mut, token::mint = initial_long_token)]
    pub initial_long_token_source: Option<Box<Account<'info, TokenAccount>>>,
    /// The source initial short token account.
    #[account(mut, token::mint = initial_short_token)]
    pub initial_short_token_source: Option<Box<Account<'info, TokenAccount>>>,
    /// The system program.
    pub system_program: Program<'info, System>,
    /// The token program.
    pub token_program: Program<'info, Token>,
    /// The associated token program.
    pub associated_token_program: Program<'info, AssociatedToken>,
}

impl<'info> internal::Create<'info, Deposit> for CreateDeposit<'info> {
    type CreateParams = CreateDepositParams;

    fn action(&self) -> AccountInfo<'info> {
        self.deposit.to_account_info()
    }

    fn payer(&self) -> AccountInfo<'info> {
        self.owner.to_account_info()
    }

    fn system_program(&self) -> AccountInfo<'info> {
        self.system_program.to_account_info()
    }

    fn create_impl(
        &mut self,
        params: &Self::CreateParams,
        nonce: &NonceBytes,
        bumps: &Self::Bumps,
        remaining_accounts: &'info [AccountInfo<'info>],
    ) -> Result<()> {
        self.transfer_tokens(params)?;
        CreateDepositOperation::builder()
            .deposit(self.deposit.clone())
            .market(self.market.clone())
            .store(self.store.clone())
            .owner(&self.owner)
            .nonce(nonce)
            .bump(bumps.deposit)
            .initial_long_token(self.initial_long_token_escrow.as_deref())
            .initial_short_token(self.initial_short_token_escrow.as_deref())
            .market_token(&self.market_token_escrow)
            .params(params)
            .swap_paths(remaining_accounts)
            .build()
            .execute()?;
        emit!(DepositCreated::new(self.store.key(), self.deposit.key())?);
        Ok(())
    }
}

impl<'info> CreateDeposit<'info> {
    fn transfer_tokens(&mut self, params: &CreateDepositParams) -> Result<()> {
        use anchor_spl::token::{transfer_checked, TransferChecked};

        let amount = params.initial_long_token_amount;
        if amount != 0 {
            let Some(source) = self.initial_long_token_source.as_ref() else {
                return err!(CoreError::TokenAccountNotProvided);
            };
            let Some(target) = self.initial_long_token_escrow.as_mut() else {
                return err!(CoreError::TokenAccountNotProvided);
            };
            let Some(mint) = self.initial_long_token.as_ref() else {
                return err!(CoreError::MintAccountNotProvided);
            };
            transfer_checked(
                CpiContext::new(
                    self.token_program.to_account_info(),
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
        }

        let amount = params.initial_short_token_amount;
        if amount != 0 {
            let Some(source) = self.initial_short_token_source.as_ref() else {
                return err!(CoreError::TokenAccountNotProvided);
            };
            let Some(target) = self.initial_short_token_escrow.as_mut() else {
                return err!(CoreError::TokenAccountNotProvided);
            };
            let Some(mint) = self.initial_short_token.as_ref() else {
                return err!(CoreError::MintAccountNotProvided);
            };
            transfer_checked(
                CpiContext::new(
                    self.token_program.to_account_info(),
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
        }

        // Make sure the data for escrow accounts is up-to-date.
        for escrow in self
            .initial_long_token_escrow
            .as_mut()
            .into_iter()
            .chain(self.initial_short_token_escrow.as_mut())
        {
            escrow.reload()?;
        }
        Ok(())
    }
}

/// The accounts definition for [`close_deposit`](crate::gmsol_store::close_deposit)
/// instruction.
#[event_cpi]
#[derive(Accounts)]
pub struct CloseDeposit<'info> {
    /// The executor of this instruction.
    pub executor: Signer<'info>,
    /// The store.
    pub store: AccountLoader<'info, Store>,
    /// The owner of the deposit.
    /// CHECK: only use to validate and receive fund.
    #[account(mut)]
    pub owner: UncheckedAccount<'info>,
    /// Market token.
    #[account(
        constraint = deposit.load()?.tokens.market_token.token().expect("must exist") == market_token.key() @ CoreError::MarketTokenMintMismatched
    )]
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
    /// The deposit to close.
    #[account(
        mut,
        constraint = deposit.load()?.header.owner == owner.key() @ CoreError::OwnerMismatched,
        constraint = deposit.load()?.header.store == store.key() @ CoreError::StoreMismatched,
        constraint = deposit.load()?.tokens.market_token.account().expect("must exist") == market_token_escrow.key() @ CoreError::MarketTokenAccountMismatched,
        constraint = deposit.load()?.tokens.initial_long_token.account() == initial_long_token_escrow.as_ref().map(|a| a.key()) @ CoreError::TokenAccountMismatched,
        constraint = deposit.load()?.tokens.initial_short_token.account() == initial_short_token_escrow.as_ref().map(|a| a.key()) @ CoreError::TokenAccountMismatched,
        seeds = [Deposit::SEED, store.key().as_ref(), owner.key().as_ref(), &deposit.load()?.header.nonce],
        bump = deposit.load()?.header.bump,
    )]
    pub deposit: AccountLoader<'info, Deposit>,
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
    /// The ATA for market token of owner.
    /// CHECK: should be checked during the execution.
    #[account(
        mut,
        constraint = is_associated_token_account(market_token_ata.key, owner.key, &market_token.key()) @ CoreError::NotAnATA,
    )]
    pub market_token_ata: UncheckedAccount<'info>,
    /// The ATA for initial long token of owner.
    /// CHECK: should be checked during the execution
    #[account(
        mut,
        constraint = is_associated_token_account(initial_long_token_ata.key, owner.key, &initial_long_token.as_ref().expect("must provided").key()) @ CoreError::NotAnATA,
    )]
    pub initial_long_token_ata: Option<UncheckedAccount<'info>>,
    /// The ATA for initial short token of owner.
    /// CHECK: should be checked during the execution
    #[account(
        mut,
        constraint = is_associated_token_account(initial_short_token_ata.key, owner.key, &initial_short_token.as_ref().expect("must provided").key()) @ CoreError::NotAnATA,
    )]
    pub initial_short_token_ata: Option<UncheckedAccount<'info>>,
    /// The system program.
    pub system_program: Program<'info, System>,
    /// The token program.
    pub token_program: Program<'info, Token>,
    /// The associated token program.
    pub associated_token_program: Program<'info, AssociatedToken>,
}

impl<'info> internal::Authentication<'info> for CloseDeposit<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.executor
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

impl<'info> internal::Close<'info, Deposit> for CloseDeposit<'info> {
    fn expected_keeper_role(&self) -> &str {
        RoleKey::ORDER_KEEPER
    }

    fn fund_receiver(&self) -> AccountInfo<'info> {
        self.owner.to_account_info()
    }

    fn process(&self, init_if_needed: bool) -> Result<internal::Success> {
        use crate::utils::token::TransferAllFromEscrowToATA;

        // Prepare signer seeds.
        let signer = self.deposit.load()?.signer();
        let seeds = signer.as_seeds();

        let builder = TransferAllFromEscrowToATA::builder()
            .system_program(self.system_program.to_account_info())
            .token_program(self.token_program.to_account_info())
            .associated_token_program(self.associated_token_program.to_account_info())
            .payer(self.executor.to_account_info())
            .owner(self.owner.to_account_info())
            .escrow_authority(self.deposit.to_account_info())
            .seeds(&seeds)
            .init_if_needed(init_if_needed);

        // Transfer market tokens.
        if !builder
            .clone()
            .mint(self.market_token.to_account_info())
            .decimals(self.market_token.decimals)
            .ata(self.market_token_ata.to_account_info())
            .escrow(self.market_token_escrow.to_account_info())
            .build()
            .execute()?
        {
            return Ok(false);
        }

        // Prevent closing the same token accounts.
        let (initial_long_token_escrow, initial_short_token_escrow) =
            if self.initial_long_token_escrow.as_ref().map(|a| a.key())
                == self.initial_short_token_escrow.as_ref().map(|a| a.key())
            {
                (self.initial_long_token_escrow.as_ref(), None)
            } else {
                (
                    self.initial_long_token_escrow.as_ref(),
                    self.initial_short_token_escrow.as_ref(),
                )
            };

        // Transfer initial long tokens.
        if let Some(escrow) = initial_long_token_escrow.as_ref() {
            let Some(ata) = self.initial_long_token_ata.as_ref() else {
                return err!(CoreError::TokenAccountNotProvided);
            };
            let Some(mint) = self.initial_long_token.as_ref() else {
                return err!(CoreError::MintAccountNotProvided);
            };
            if !builder
                .clone()
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

        // Transfer initial short tokens.
        if let Some(escrow) = initial_short_token_escrow.as_ref() {
            let Some(ata) = self.initial_short_token_ata.as_ref() else {
                return err!(CoreError::TokenAccountNotProvided);
            };
            let Some(mint) = self.initial_short_token.as_ref() else {
                return err!(CoreError::MintAccountNotProvided);
            };
            if !builder
                .clone()
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

    fn event_authority(&self, bumps: &Self::Bumps) -> (AccountInfo<'info>, u8) {
        (
            self.event_authority.to_account_info(),
            bumps.event_authority,
        )
    }

    fn action(&self) -> &AccountLoader<'info, Deposit> {
        &self.deposit
    }
}
