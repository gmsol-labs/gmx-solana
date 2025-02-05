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
    events::EventEmitter,
    ops::{
        execution_fee::PayExecutionFeeOperation,
        glv::{CreateGlvDepositOperation, CreateGlvDepositParams, ExecuteGlvDepositOperation},
        market::{MarketTransferInOperation, MarketTransferOutOperation},
    },
    states::{
        common::action::{Action, ActionExt, ActionSigner},
        glv::{GlvMarketFlag, SplitAccountsForGlv},
        Chainlink, Glv, GlvDeposit, Market, NonceBytes, Oracle, RoleKey, Seed, Store,
        StoreWalletSigner, TokenMapHeader, TokenMapLoader,
    },
    utils::{
        internal,
        token::{
            is_associated_token_account, is_associated_token_account_or_owner,
            is_associated_token_account_with_program_id,
        },
    },
    CoreError,
};

/// The accounts definition for [`create_glv_deposit`](crate::create_glv_deposit) instruction.
#[derive(Accounts)]
#[instruction(nonce: [u8; 32])]
pub struct CreateGlvDeposit<'info> {
    /// The owner of the deposit.
    #[account(mut)]
    pub owner: Signer<'info>,
    /// The receiver of the output funds.
    /// CHECK: only the address is used.
    pub receiver: UncheckedAccount<'info>,
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
    /// GLV deposit.
    #[account(
        init,
        payer = owner,
        space = 8 + GlvDeposit::INIT_SPACE,
        seeds = [GlvDeposit::SEED, store.key().as_ref(), owner.key().as_ref(), &nonce],
        bump,
    )]
    pub glv_deposit: AccountLoader<'info, GlvDeposit>,
    /// GLV Token.
    pub glv_token: Box<InterfaceAccount<'info, token_interface::Mint>>,
    /// Market token.
    pub market_token: Box<Account<'info, Mint>>,
    /// Initial long token.
    pub initial_long_token: Option<Box<Account<'info, Mint>>>,
    /// initial short token.
    pub initial_short_token: Option<Box<Account<'info, Mint>>>,
    /// The source market token account.
    #[account(mut, token::mint = market_token)]
    pub market_token_source: Option<Box<Account<'info, TokenAccount>>>,
    /// The source initial long token account.
    #[account(mut, token::mint = initial_long_token)]
    pub initial_long_token_source: Option<Box<Account<'info, TokenAccount>>>,
    /// The source initial short token account.
    #[account(mut, token::mint = initial_short_token)]
    pub initial_short_token_source: Option<Box<Account<'info, TokenAccount>>>,
    /// The escrow account for GLV tokens.
    #[account(
        mut,
        associated_token::mint = glv_token,
        associated_token::authority = glv_deposit,
        associated_token::token_program = glv_token_program,
    )]
    pub glv_token_escrow: Box<InterfaceAccount<'info, token_interface::TokenAccount>>,
    /// The escrow account for market tokens.
    #[account(
        mut,
        associated_token::mint = market_token,
        associated_token::authority = glv_deposit,
    )]
    pub market_token_escrow: Box<Account<'info, TokenAccount>>,
    /// The escrow account for initial long tokens.
    #[account(
        mut,
        associated_token::mint = initial_long_token,
        associated_token::authority = glv_deposit,
    )]
    pub initial_long_token_escrow: Option<Box<Account<'info, TokenAccount>>>,
    /// The escrow account for initial short tokens.
    #[account(
        mut,
        associated_token::mint = initial_short_token,
        associated_token::authority = glv_deposit,
    )]
    pub initial_short_token_escrow: Option<Box<Account<'info, TokenAccount>>>,
    /// The system program.
    pub system_program: Program<'info, System>,
    /// The token program.
    pub token_program: Program<'info, Token>,
    /// The token program for GLV token.
    pub glv_token_program: Program<'info, Token2022>,
    /// The associated token program.
    pub associated_token_program: Program<'info, AssociatedToken>,
}

impl<'info> internal::Create<'info, GlvDeposit> for CreateGlvDeposit<'info> {
    type CreateParams = CreateGlvDepositParams;

    fn action(&self) -> AccountInfo<'info> {
        self.glv_deposit.to_account_info()
    }

    fn payer(&self) -> AccountInfo<'info> {
        self.owner.to_account_info()
    }

    fn system_program(&self) -> AccountInfo<'info> {
        self.system_program.to_account_info()
    }

    fn validate(&self, _params: &Self::CreateParams) -> Result<()> {
        let market_token = self.market_token.key();
        let is_deposit_allowed = self
            .glv
            .load()?
            .market_config(&market_token)
            .ok_or_else(|| error!(CoreError::Internal))?
            .get_flag(GlvMarketFlag::IsDepositAllowed);
        require!(is_deposit_allowed, CoreError::GlvDepositIsNotAllowed);
        Ok(())
    }

    fn create_impl(
        &mut self,
        params: &Self::CreateParams,
        nonce: &NonceBytes,
        bumps: &Self::Bumps,
        remaining_accounts: &'info [AccountInfo<'info>],
    ) -> Result<()> {
        self.transfer_tokens(params)?;
        CreateGlvDepositOperation::builder()
            .glv_deposit(self.glv_deposit.clone())
            .market(self.market.clone())
            .store(self.store.clone())
            .owner(&self.owner)
            .receiver(&self.receiver)
            .nonce(nonce)
            .bump(bumps.glv_deposit)
            .initial_long_token(self.initial_long_token_escrow.as_deref())
            .initial_short_token(self.initial_short_token_escrow.as_deref())
            .market_token(&self.market_token_escrow)
            .glv_token(&self.glv_token_escrow)
            .params(params)
            .swap_paths(remaining_accounts)
            .build()
            .unchecked_execute()?;
        Ok(())
    }
}

impl CreateGlvDeposit<'_> {
    fn transfer_tokens(&mut self, params: &CreateGlvDepositParams) -> Result<()> {
        use anchor_spl::token::{transfer_checked, TransferChecked};

        let amount = params.initial_long_token_amount;
        if amount != 0 {
            let Some(source) = self.initial_long_token_source.as_ref() else {
                return err!(CoreError::TokenAccountNotProvided);
            };
            let Some(target) = self.initial_long_token_escrow.as_ref() else {
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
            let Some(target) = self.initial_short_token_escrow.as_ref() else {
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

        let amount = params.market_token_amount;
        if amount != 0 {
            let Some(source) = self.market_token_source.as_ref() else {
                return err!(CoreError::TokenAccountNotProvided);
            };
            let target = &self.market_token_escrow;
            let mint = &self.market_token;
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
            .chain(Some(&mut self.market_token_escrow))
        {
            escrow.reload()?;
        }

        Ok(())
    }
}

/// The accounts definition for [`close_glv_deposit`](crate::gmsol_store::close_glv_deposit) instruction.
#[event_cpi]
#[derive(Accounts)]
pub struct CloseGlvDeposit<'info> {
    /// The executor of this instruction.
    pub executor: Signer<'info>,
    /// The store.
    pub store: AccountLoader<'info, Store>,
    /// The store wallet.
    #[account(mut, seeds = [Store::WALLET_SEED, store.key().as_ref()], bump)]
    pub store_wallet: SystemAccount<'info>,
    /// The owner of the deposit.
    /// CHECK: only use to validate and receive fund.
    #[account(mut)]
    pub owner: UncheckedAccount<'info>,
    /// The recevier of the deposit.
    /// CHECK: only use to validate and receive fund.
    #[account(mut)]
    pub receiver: UncheckedAccount<'info>,
    /// The GLV deposit to close.
    #[account(
        mut,
        constraint = glv_deposit.load()?.header.store == store.key() @ CoreError::StoreMismatched,
        constraint = glv_deposit.load()?.header.owner == owner.key() @ CoreError::OwnerMismatched,
        constraint = glv_deposit.load()?.header.receiver() == receiver.key() @ CoreError::ReceiverMismatched,
        // The rent receiver of a GLV deposit must be the owner.
        constraint = glv_deposit.load()?.header.rent_receiver() == owner.key @ CoreError::RentReceiverMismatched,
        constraint = glv_deposit.load()?.tokens.market_token_account() == market_token_escrow.key() @ CoreError::MarketTokenAccountMismatched,
        constraint = glv_deposit.load()?.tokens.glv_token_account() == glv_token_escrow.key() @ CoreError::MarketTokenAccountMismatched,
        constraint = glv_deposit.load()?.tokens.initial_long_token.account() == initial_long_token_escrow.as_ref().map(|a| a.key()) @ CoreError::TokenAccountMismatched,
        constraint = glv_deposit.load()?.tokens.initial_short_token.account() == initial_short_token_escrow.as_ref().map(|a| a.key()) @ CoreError::TokenAccountMismatched,
        seeds = [GlvDeposit::SEED, store.key().as_ref(), owner.key().as_ref(), &glv_deposit.load()?.header.nonce],
        bump = glv_deposit.load()?.header.bump,
    )]
    pub glv_deposit: AccountLoader<'info, GlvDeposit>,
    /// Market token.
    #[account(
        constraint = glv_deposit.load()?.tokens.market_token() == market_token.key() @ CoreError::MarketTokenMintMismatched
    )]
    pub market_token: Box<Account<'info, Mint>>,
    /// Initial long token.
    #[account(
        constraint = glv_deposit.load()?.tokens.initial_long_token.token().map(|token| initial_long_token.key() == token).unwrap_or(true) @ CoreError::TokenMintMismatched
    )]
    pub initial_long_token: Option<Box<Account<'info, Mint>>>,
    /// Initial short token.
    #[account(
        constraint = glv_deposit.load()?.tokens.initial_short_token.token().map(|token| initial_short_token.key() == token).unwrap_or(true) @ CoreError::TokenMintMismatched
    )]
    pub initial_short_token: Option<Box<Account<'info, Mint>>>,
    /// GLV token.
    #[account(
        constraint = glv_deposit.load()?.tokens.glv_token() == glv_token.key() @ CoreError::TokenMintMismatched
    )]
    pub glv_token: Box<InterfaceAccount<'info, token_interface::Mint>>,
    /// The escrow account for market tokens.
    #[account(
        mut,
        associated_token::mint = market_token,
        associated_token::authority = glv_deposit,
    )]
    pub market_token_escrow: Box<Account<'info, TokenAccount>>,
    /// The escrow account for receiving initial long token for deposit.
    #[account(
        mut,
        associated_token::mint = initial_long_token,
        associated_token::authority = glv_deposit,
    )]
    pub initial_long_token_escrow: Option<Box<Account<'info, TokenAccount>>>,
    /// The escrow account for receiving initial short token for deposit.
    #[account(
        mut,
        associated_token::mint = initial_short_token,
        associated_token::authority = glv_deposit,
    )]
    pub initial_short_token_escrow: Option<Box<Account<'info, TokenAccount>>>,
    /// The escrow account for GLV tokens.
    #[account(
        mut,
        associated_token::mint = glv_token,
        associated_token::authority = glv_deposit,
        associated_token::token_program = glv_token_program,
    )]
    pub glv_token_escrow: Box<InterfaceAccount<'info, token_interface::TokenAccount>>,
    /// The ATA for market token of the owner.
    /// CHECK: should be checked during the execution.
    #[account(
        mut,
        constraint = is_associated_token_account(market_token_ata.key, owner.key, &market_token.key()) @ CoreError::NotAnATA,
    )]
    pub market_token_ata: UncheckedAccount<'info>,
    /// The ATA for initial long token of the owner.
    /// CHECK: should be checked during the execution
    #[account(
        mut,
        constraint = is_associated_token_account_or_owner(initial_long_token_ata.key, owner.key, &initial_long_token.as_ref().expect("must provided").key()) @ CoreError::NotAnATA,
    )]
    pub initial_long_token_ata: Option<UncheckedAccount<'info>>,
    /// The ATA for initial short token of the owner.
    /// CHECK: should be checked during the execution
    #[account(
        mut,
        constraint = is_associated_token_account_or_owner(initial_short_token_ata.key, owner.key, &initial_short_token.as_ref().expect("must provided").key()) @ CoreError::NotAnATA,
    )]
    pub initial_short_token_ata: Option<UncheckedAccount<'info>>,
    /// The ATA for GLV token of the receiver.
    /// CHECK: should be checked during the execution.
    #[account(
        mut,
        constraint = is_associated_token_account_with_program_id(glv_token_ata.key, receiver.key, &glv_token.key(), &glv_token_program.key()) @ CoreError::NotAnATA,
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

impl<'info> internal::Close<'info, GlvDeposit> for CloseGlvDeposit<'info> {
    fn expected_keeper_role(&self) -> &str {
        RoleKey::ORDER_KEEPER
    }

    fn rent_receiver(&self) -> AccountInfo<'info> {
        debug_assert!(
            self.glv_deposit.load().unwrap().header.rent_receiver() == self.owner.key,
            "The rent receiver must have been checked to be the owner"
        );
        self.owner.to_account_info()
    }

    fn store_wallet_bump(&self, bumps: &Self::Bumps) -> u8 {
        bumps.store_wallet
    }

    fn process(
        &self,
        init_if_needed: bool,
        store_wallet_signer: &StoreWalletSigner,
        _event_emitter: &EventEmitter<'_, 'info>,
    ) -> Result<internal::Success> {
        use crate::utils::token::TransferAllFromEscrowToATA;

        // Prepare signer seeds.
        let signer = self.glv_deposit.load()?.signer();
        let seeds = signer.as_seeds();

        let builder = TransferAllFromEscrowToATA::builder()
            .store_wallet(self.store_wallet.to_account_info())
            .store_wallet_signer(store_wallet_signer)
            .system_program(self.system_program.to_account_info())
            .associated_token_program(self.associated_token_program.to_account_info())
            .payer(self.executor.to_account_info())
            .escrow_authority(self.glv_deposit.to_account_info())
            .escrow_authority_seeds(&seeds)
            .init_if_needed(init_if_needed)
            .rent_receiver(self.rent_receiver())
            .should_unwrap_native(
                self.glv_deposit
                    .load()?
                    .header()
                    .should_unwrap_native_token(),
            );

        // Transfer market tokens.
        if !builder
            .clone()
            .token_program(self.token_program.to_account_info())
            .mint(self.market_token.to_account_info())
            .decimals(self.market_token.decimals)
            .ata(self.market_token_ata.to_account_info())
            .escrow(self.market_token_escrow.to_account_info())
            .owner(self.owner.to_account_info())
            .build()
            .unchecked_execute()?
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
            .owner(self.receiver.to_account_info())
            .build()
            .unchecked_execute()?
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
                .token_program(self.token_program.to_account_info())
                .mint(mint.to_account_info())
                .decimals(mint.decimals)
                .ata(ata.to_account_info())
                .escrow(escrow.to_account_info())
                .owner(self.owner.to_account_info())
                .build()
                .unchecked_execute()?
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
                .token_program(self.token_program.to_account_info())
                .mint(mint.to_account_info())
                .decimals(mint.decimals)
                .ata(ata.to_account_info())
                .escrow(escrow.to_account_info())
                .owner(self.owner.to_account_info())
                .build()
                .unchecked_execute()?
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

    fn action(&self) -> &AccountLoader<'info, GlvDeposit> {
        &self.glv_deposit
    }
}

impl<'info> internal::Authentication<'info> for CloseGlvDeposit<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.executor
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition for [`execute_glv_deposit`](crate::gmsol_store::execute_glv_deposit) instruction.
///
/// Remaining accounts expected by this instruction:
///
///   - 0..N. `[]` N market accounts, where N represents the total number of markets managed
///     by the given GLV.
///   - N..2N. `[]` N market token accounts (see above for the definition of N).
///   - 2N..2N+M. `[]` M feed accounts, where M represents the total number of tokens in the
///     swap params.
///   - 2N+M..2N+M+L. `[writable]` L market accounts, where L represents the total number of unique
///     markets excluding the current market in the swap params.
#[event_cpi]
#[derive(Accounts)]
pub struct ExecuteGlvDeposit<'info> {
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
        mut,
        has_one = store,
        constraint = glv.load()?.contains(&market_token.key()) @ CoreError::InvalidArgument,
    )]
    pub glv: AccountLoader<'info, Glv>,
    /// Market.
    #[account(mut, has_one = store)]
    pub market: AccountLoader<'info, Market>,
    /// The GLV deposit to execute.
    #[account(
        mut,
        constraint = glv_deposit.load()?.header.store == store.key() @ CoreError::StoreMismatched,
        constraint = glv_deposit.load()?.header.market == market.key() @ CoreError::MarketMismatched,
        constraint = glv_deposit.load()?.tokens.glv_token() == glv_token.key() @ CoreError::TokenMintMismatched,
        constraint = glv_deposit.load()?.tokens.market_token() == market_token.key() @ CoreError::MarketTokenMintMismatched,
        constraint = glv_deposit.load()?.tokens.market_token_account() == market_token_escrow.key() @ CoreError::MarketTokenAccountMismatched,
        constraint = glv_deposit.load()?.tokens.glv_token_account() == glv_token_escrow.key() @ CoreError::MarketTokenAccountMismatched,
        constraint = glv_deposit.load()?.tokens.initial_long_token.account() == initial_long_token_escrow.as_ref().map(|a| a.key()) @ CoreError::TokenAccountMismatched,
        constraint = glv_deposit.load()?.tokens.initial_short_token.account() == initial_short_token_escrow.as_ref().map(|a| a.key()) @ CoreError::TokenAccountMismatched,
        seeds = [GlvDeposit::SEED, store.key().as_ref(), glv_deposit.load()?.header.owner.as_ref(), &glv_deposit.load()?.header.nonce],
        bump = glv_deposit.load()?.header.bump,
    )]
    pub glv_deposit: AccountLoader<'info, GlvDeposit>,
    /// GLV token mint.
    #[account(mut, constraint = glv.load()?.glv_token == glv_token.key() @ CoreError::TokenMintMismatched)]
    pub glv_token: Box<InterfaceAccount<'info, token_interface::Mint>>,
    /// Market token mint.
    #[account(mut, constraint = market.load()?.meta().market_token_mint == market_token.key() @ CoreError::MarketTokenMintMismatched)]
    pub market_token: Box<Account<'info, Mint>>,
    /// Initial long token.
    #[account(
        constraint = glv_deposit.load()?.tokens.initial_long_token.token().map(|token| initial_long_token.key() == token).unwrap_or(true) @ CoreError::TokenMintMismatched
    )]
    pub initial_long_token: Option<Box<Account<'info, Mint>>>,
    /// Initial short token.
    #[account(
        constraint = glv_deposit.load()?.tokens.initial_short_token.token().map(|token| initial_short_token.key() == token).unwrap_or(true) @ CoreError::TokenMintMismatched
    )]
    pub initial_short_token: Option<Box<Account<'info, Mint>>>,
    /// The escrow account for GLV tokens.
    #[account(
        mut,
        associated_token::mint = glv_token,
        associated_token::authority = glv_deposit,
        associated_token::token_program = glv_token_program,
    )]
    pub glv_token_escrow: Box<InterfaceAccount<'info, token_interface::TokenAccount>>,
    /// The escrow account for market tokens.
    #[account(
        mut,
        associated_token::mint = market_token,
        associated_token::authority = glv_deposit,
    )]
    pub market_token_escrow: Box<Account<'info, TokenAccount>>,
    /// The escrow account for receiving initial long token for deposit.
    #[account(
        mut,
        associated_token::mint = initial_long_token,
        associated_token::authority = glv_deposit,
    )]
    pub initial_long_token_escrow: Option<Box<Account<'info, TokenAccount>>>,
    /// The escrow account for receiving initial short token for deposit.
    #[account(
        mut,
        associated_token::mint = initial_short_token,
        associated_token::authority = glv_deposit,
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
    /// Chainlink Program.
    pub chainlink_program: Option<Program<'info, Chainlink>>,
}

/// CHECK: only ORDER_KEEPER is allowed to call this function.
pub(crate) fn unchecked_execute_glv_deposit<'info>(
    ctx: Context<'_, '_, 'info, 'info, ExecuteGlvDeposit<'info>>,
    execution_lamports: u64,
    throw_on_execution_error: bool,
) -> Result<()> {
    let accounts = ctx.accounts;
    let remaining_accounts = ctx.remaining_accounts;

    let SplitAccountsForGlv {
        markets,
        market_tokens,
        remaining_accounts,
        tokens,
    } = {
        let glv_deposit = accounts.glv_deposit.load()?;
        let token_map = accounts.token_map.load_token_map()?;
        accounts.glv.load()?.validate_and_split_remaining_accounts(
            &accounts.store.key(),
            remaining_accounts,
            Some(&*glv_deposit),
            &token_map,
        )?
    };

    let event_authority = accounts.event_authority.clone();
    let event_emitter = EventEmitter::new(&event_authority, ctx.bumps.event_authority);

    let signer = accounts.glv_deposit.load()?.signer();
    accounts.transfer_tokens_in(&signer, remaining_accounts, &event_emitter)?;

    let executed = accounts.perform_execution(
        markets,
        market_tokens,
        &tokens,
        remaining_accounts,
        throw_on_execution_error,
        &event_emitter,
    )?;

    if executed {
        accounts.glv_deposit.load_mut()?.header.completed()?;
    } else {
        accounts.glv_deposit.load_mut()?.header.cancelled()?;
        accounts.transfer_tokens_out(remaining_accounts, &event_emitter)?;
    }

    // It must be placed at the end to be executed correctly.
    accounts.pay_execution_fee(execution_lamports)?;
    Ok(())
}

impl<'info> internal::Authentication<'info> for ExecuteGlvDeposit<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

impl<'info> ExecuteGlvDeposit<'info> {
    #[inline(never)]
    fn pay_execution_fee(&self, execution_fee: u64) -> Result<()> {
        let execution_lamports = self.glv_deposit.load()?.execution_lamports(execution_fee);
        PayExecutionFeeOperation::builder()
            .payer(self.glv_deposit.to_account_info())
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
        // self.transfer_market_tokens_in(signer)?;
        self.transfer_initial_tokens_in(signer, remaining_accounts, event_emitter)?;
        Ok(())
    }

    #[inline(never)]
    fn transfer_tokens_out(
        &self,
        remaining_accounts: &'info [AccountInfo<'info>],
        event_emitter: &EventEmitter<'_, 'info>,
    ) -> Result<()> {
        // self.transfer_market_tokens_out()?;
        self.transfer_initial_tokens_out(remaining_accounts, event_emitter)?;
        Ok(())
    }

    fn transfer_initial_tokens_in(
        &self,
        sigenr: &ActionSigner,
        remaining_accounts: &'info [AccountInfo<'info>],
        event_emitter: &EventEmitter<'_, 'info>,
    ) -> Result<()> {
        let seeds = sigenr.as_seeds();
        let builder = MarketTransferInOperation::builder()
            .store(&self.store)
            .from_authority(self.glv_deposit.to_account_info())
            .token_program(self.token_program.to_account_info())
            .signer_seeds(&seeds)
            .event_emitter(*event_emitter);
        let store = &self.store.key();

        for is_primary in [true, false] {
            let (amount, escrow, vault) = if is_primary {
                (
                    self.glv_deposit
                        .load()?
                        .params
                        .deposit
                        .initial_long_token_amount,
                    self.initial_long_token_escrow.as_ref(),
                    self.initial_long_token_vault.as_ref(),
                )
            } else {
                (
                    self.glv_deposit
                        .load()?
                        .params
                        .deposit
                        .initial_short_token_amount,
                    self.initial_short_token_escrow.as_ref(),
                    self.initial_short_token_vault.as_ref(),
                )
            };

            if amount == 0 {
                continue;
            }

            let escrow = escrow.ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
            let market = self
                .glv_deposit
                .load()?
                .swap
                .find_and_unpack_first_market(store, is_primary, remaining_accounts)?
                .unwrap_or(self.market.clone());
            let vault = vault.ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
            builder
                .clone()
                .market(&market)
                .from(escrow.to_account_info())
                .vault(vault)
                .amount(amount)
                .build()
                .execute()?;
        }

        Ok(())
    }

    fn transfer_initial_tokens_out(
        &self,
        remaining_accounts: &'info [AccountInfo<'info>],
        event_emitter: &EventEmitter<'_, 'info>,
    ) -> Result<()> {
        let builder = MarketTransferOutOperation::builder()
            .store(&self.store)
            .token_program(self.token_program.to_account_info())
            .event_emitter(*event_emitter);

        let store = &self.store.key();

        for is_primary in [true, false] {
            let (amount, token, escrow, vault) = if is_primary {
                (
                    self.glv_deposit
                        .load()?
                        .params
                        .deposit
                        .initial_long_token_amount,
                    self.initial_long_token.as_ref(),
                    self.initial_long_token_escrow.as_ref(),
                    self.initial_long_token_vault.as_ref(),
                )
            } else {
                (
                    self.glv_deposit
                        .load()?
                        .params
                        .deposit
                        .initial_short_token_amount,
                    self.initial_short_token.as_ref(),
                    self.initial_short_token_escrow.as_ref(),
                    self.initial_short_token_vault.as_ref(),
                )
            };

            let Some(escrow) = escrow else {
                continue;
            };

            let market = self
                .glv_deposit
                .load()?
                .swap
                .find_and_unpack_first_market(store, is_primary, remaining_accounts)?
                .unwrap_or(self.market.clone());
            let token = token.ok_or_else(|| error!(CoreError::TokenMintNotProvided))?;
            let vault = vault.ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
            builder
                .clone()
                .market(&market)
                .to(escrow.to_account_info())
                .vault(vault.to_account_info())
                .amount(amount)
                .decimals(token.decimals)
                .token_mint(token.to_account_info())
                .build()
                .execute()?;
        }

        Ok(())
    }

    fn perform_execution(
        &mut self,
        markets: &'info [AccountInfo<'info>],
        market_tokens: &'info [AccountInfo<'info>],
        tokens: &[Pubkey],
        remaining_accounts: &'info [AccountInfo<'info>],
        throw_on_execution_error: bool,
        event_emitter: &EventEmitter<'_, 'info>,
    ) -> Result<bool> {
        let builder = ExecuteGlvDepositOperation::builder()
            .glv_deposit(self.glv_deposit.clone())
            .token_program(self.token_program.to_account_info())
            .glv_token_program(self.glv_token_program.to_account_info())
            .throw_on_execution_error(throw_on_execution_error)
            .store(self.store.clone())
            .glv(self.glv.clone())
            .glv_token_mint(&mut self.glv_token)
            .glv_token_receiver(self.glv_token_escrow.to_account_info())
            .market(self.market.clone())
            .market_token_source(&self.market_token_escrow)
            .market_token_mint(&mut self.market_token)
            .market_token_vault(self.market_token_vault.to_account_info())
            .markets(markets)
            .market_tokens(market_tokens)
            .event_emitter(*event_emitter);

        self.oracle.load_mut()?.with_prices(
            &self.store,
            &self.token_map,
            tokens,
            remaining_accounts,
            self.chainlink_program.as_ref(),
            |oracle, remaining_accounts| {
                builder
                    .oracle(oracle)
                    .remaining_accounts(remaining_accounts)
                    .build()
                    .unchecked_execute()
            },
        )
    }
}
