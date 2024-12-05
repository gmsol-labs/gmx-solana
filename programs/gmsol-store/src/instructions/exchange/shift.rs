use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{transfer_checked, Mint, Token, TokenAccount, TransferChecked},
};
use gmsol_utils::InitSpace;

use crate::{
    ops::shift::{CreateShiftOperation, CreateShiftParams},
    states::{common::action::ActionExt, Market, NonceBytes, RoleKey, Seed, Shift, Store},
    utils::{internal, token::is_associated_token_account},
    CoreError,
};

/// The accounts definition for the [`create_shift`](crate::gmsol_store::create_shift) instruction.
#[derive(Accounts)]
#[instruction(nonce: [u8; 32])]
pub struct CreateShift<'info> {
    /// The owner.
    #[account(mut)]
    pub owner: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// From market.
    #[account(mut, has_one = store)]
    pub from_market: AccountLoader<'info, Market>,
    /// To market.
    #[account(
        has_one = store,
        constraint = from_market.load()?.validate_shiftable(&*to_market.load()?).is_ok() @ CoreError::TokenMintMismatched,
    )]
    pub to_market: AccountLoader<'info, Market>,
    /// Shift.
    #[account(
        init,
        space = 8 + Shift::INIT_SPACE,
        payer = owner,
        seeds = [Shift::SEED, store.key().as_ref(), owner.key().as_ref(), &nonce],
        bump,
    )]
    pub shift: AccountLoader<'info, Shift>,
    /// From market token.
    #[account(constraint = from_market.load()?.meta().market_token_mint == from_market_token.key() @ CoreError::MarketTokenMintMismatched)]
    pub from_market_token: Box<Account<'info, Mint>>,
    /// To market token.
    #[account(constraint = from_market.load()?.meta().market_token_mint == from_market_token.key() @ CoreError::MarketTokenMintMismatched)]
    pub to_market_token: Box<Account<'info, Mint>>,
    /// The escrow account for the from market tokens.
    #[account(
        mut,
        associated_token::mint = from_market_token,
        associated_token::authority = shift,
    )]
    pub from_market_token_escrow: Box<Account<'info, TokenAccount>>,
    /// The escrow account for the to market tokens.
    #[account(
        mut,
        associated_token::mint = to_market_token,
        associated_token::authority = shift,
    )]
    pub to_market_token_escrow: Box<Account<'info, TokenAccount>>,
    /// The source from market token account.
    #[account(
        mut,
        token::mint = from_market_token,
    )]
    pub from_market_token_source: Box<Account<'info, TokenAccount>>,
    /// The ATA for receiving to market tokens.
    #[account(
        associated_token::mint = to_market_token,
        associated_token::authority = owner,
    )]
    pub to_market_token_ata: Box<Account<'info, TokenAccount>>,
    /// The system program.
    pub system_program: Program<'info, System>,
    /// The token program.
    pub token_program: Program<'info, Token>,
    /// The associated token program.
    pub associated_token_program: Program<'info, AssociatedToken>,
}

impl<'info> internal::Create<'info, Shift> for CreateShift<'info> {
    type CreateParams = CreateShiftParams;

    fn action(&self) -> AccountInfo<'info> {
        self.shift.to_account_info()
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
        _remaining_accounts: &'info [AccountInfo<'info>],
    ) -> Result<()> {
        self.transfer_tokens(params)?;
        CreateShiftOperation::builder()
            .store(&self.store)
            .owner(self.owner.to_account_info())
            .shift(&self.shift)
            .from_market(&self.from_market)
            .from_market_token_account(&self.from_market_token_escrow)
            .to_market(&self.to_market)
            .to_market_token_account(&self.to_market_token_escrow)
            .nonce(nonce)
            .bump(bumps.shift)
            .params(params)
            .build()
            .execute()?;
        Ok(())
    }
}

impl<'info> CreateShift<'info> {
    fn transfer_tokens(&mut self, params: &CreateShiftParams) -> Result<()> {
        let amount = params.from_market_token_amount;
        let source = &self.from_market_token_source;
        let target = &mut self.from_market_token_escrow;
        let mint = &self.from_market_token;
        if amount != 0 {
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
            target.reload()?;
        }
        Ok(())
    }
}

/// The accounts definition for the [`close_shift`](crate::gmsol_store::close_shift) instruction.
#[event_cpi]
#[derive(Accounts)]
pub struct CloseShift<'info> {
    /// The executor of this instruction.
    pub executor: Signer<'info>,
    /// The store.
    pub store: AccountLoader<'info, Store>,
    /// The owenr of the shift.
    /// CHECK: only use to validate and receive fund.
    #[account(mut)]
    pub owner: UncheckedAccount<'info>,
    /// The shift to close.
    #[account(
        mut,
        constraint = shift.load()?.header.store == store.key() @ CoreError::StoreMismatched,
        constraint = shift.load()?.header.owner == owner.key() @ CoreError::OwnerMismatched,
        // The rent receiver of a shift must be the owner.
        constraint = shift.load()?.header.rent_receiver() == owner.key @ CoreError::RentReceiverMismatched,
        constraint = shift.load()?.tokens.from_market_token_account() == from_market_token_escrow.key() @ CoreError::MarketTokenAccountMismatched,
        constraint = shift.load()?.tokens.to_market_token_account() == to_market_token_escrow.key() @ CoreError::MarketTokenAccountMismatched,
    )]
    pub shift: AccountLoader<'info, Shift>,
    /// From market token.
    #[account(constraint = shift.load()?.tokens.from_market_token() == from_market_token.key() @ CoreError::MarketTokenMintMismatched)]
    pub from_market_token: Box<Account<'info, Mint>>,
    /// To market token.
    #[account(constraint = shift.load()?.tokens.to_market_token() == to_market_token.key() @ CoreError::MarketTokenMintMismatched)]
    pub to_market_token: Box<Account<'info, Mint>>,
    /// The escrow account for the from market tokens.
    #[account(
        mut,
        associated_token::mint = from_market_token,
        associated_token::authority = shift,
    )]
    pub from_market_token_escrow: Box<Account<'info, TokenAccount>>,
    /// The escrow account for the to market tokens.
    #[account(
        mut,
        associated_token::mint = to_market_token,
        associated_token::authority = shift,
    )]
    pub to_market_token_escrow: Box<Account<'info, TokenAccount>>,
    /// The ATA for from market token of owner.
    /// CHECK: should be checked during the execution.
    #[account(
        mut,
        constraint = is_associated_token_account(from_market_token_ata.key, owner.key, &from_market_token.key()) @ CoreError::NotAnATA,
    )]
    pub from_market_token_ata: UncheckedAccount<'info>,
    /// The ATA for to market token of owner.
    /// CHECK: should be checked during the execution.
    #[account(
        mut,
        constraint = is_associated_token_account(to_market_token_ata.key, owner.key, &to_market_token.key()) @ CoreError::NotAnATA,
    )]
    pub to_market_token_ata: UncheckedAccount<'info>,
    /// The system program.
    pub system_program: Program<'info, System>,
    /// The token program.
    pub token_program: Program<'info, Token>,
    /// The associated token program.
    pub associated_token_program: Program<'info, AssociatedToken>,
}

impl<'info> internal::Authentication<'info> for CloseShift<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.executor
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

impl<'info> internal::Close<'info, Shift> for CloseShift<'info> {
    fn expected_keeper_role(&self) -> &str {
        RoleKey::ORDER_KEEPER
    }

    fn rent_receiver(&self) -> AccountInfo<'info> {
        debug_assert!(
            self.shift.load().unwrap().header.rent_receiver() == self.owner.key,
            "The rent receiver must have been checked to be the owner"
        );
        self.owner.to_account_info()
    }

    fn process(&self, init_if_needed: bool) -> Result<internal::Success> {
        use crate::utils::token::TransferAllFromEscrowToATA;

        let signer = self.shift.load()?.signer();
        let seeds = signer.as_seeds();

        let builder = TransferAllFromEscrowToATA::builder()
            .system_program(self.system_program.to_account_info())
            .token_program(self.token_program.to_account_info())
            .associated_token_program(self.associated_token_program.to_account_info())
            .payer(self.executor.to_account_info())
            .owner(self.owner.to_account_info())
            .escrow_authority(self.shift.to_account_info())
            .seeds(&seeds)
            .init_if_needed(init_if_needed)
            .rent_receiver(self.rent_receiver());

        // Transfer from market tokens.
        if !builder
            .clone()
            .mint(self.from_market_token.to_account_info())
            .decimals(self.from_market_token.decimals)
            .ata(self.from_market_token_ata.to_account_info())
            .escrow(self.from_market_token_escrow.to_account_info())
            .build()
            .execute()?
        {
            return Ok(false);
        }

        // Transfer to market tokens.
        if !builder
            .clone()
            .mint(self.to_market_token.to_account_info())
            .decimals(self.to_market_token.decimals)
            .ata(self.to_market_token_ata.to_account_info())
            .escrow(self.to_market_token_escrow.to_account_info())
            .build()
            .execute()?
        {
            return Ok(false);
        }

        Ok(true)
    }

    fn event_authority(&self, bumps: &Self::Bumps) -> (AccountInfo<'info>, u8) {
        (
            self.event_authority.to_account_info(),
            bumps.event_authority,
        )
    }

    fn action(&self) -> &AccountLoader<'info, Shift> {
        &self.shift
    }
}
