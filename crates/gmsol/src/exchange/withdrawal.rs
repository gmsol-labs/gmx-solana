use std::ops::Deref;

use anchor_client::{
    anchor_lang::{system_program, Id},
    solana_sdk::{instruction::AccountMeta, pubkey::Pubkey, signer::Signer},
};
use anchor_spl::associated_token::get_associated_token_address;
use gmsol_solana_utils::{
    bundle_builder::{BundleBuilder, BundleOptions},
    compute_budget::ComputeBudget,
    transaction_builder::TransactionBuilder,
};
use gmsol_store::{
    accounts, instruction,
    ops::withdrawal::CreateWithdrawalParams,
    states::{
        common::{action::Action, swap::SwapActionParams, TokensWithFeed},
        withdrawal::Withdrawal,
        NonceBytes, PriceProviderKind, Pyth, TokenMapAccess,
    },
};

use crate::{
    store::{token::TokenAccountOps, utils::FeedsParser},
    utils::{
        builder::{
            FeedAddressMap, FeedIds, MakeBundleBuilder, PullOraclePriceConsumer, SetExecutionFee,
        },
        fix_optional_account_metas, ZeroCopy,
    },
};

use super::{generate_nonce, get_ata_or_owner, ExchangeOps};

#[cfg(feature = "pyth-pull-oracle")]
use crate::pyth::pull_oracle::Prices;

/// `execute_withdrawal` compute budget.
pub const EXECUTE_WITHDRAWAL_COMPUTE_BUDGET: u32 = 400_000;

/// Create Withdrawal Builder.
pub struct CreateWithdrawalBuilder<'a, C> {
    client: &'a crate::Client<C>,
    store: Pubkey,
    market_token: Pubkey,
    nonce: Option<NonceBytes>,
    execution_fee: u64,
    amount: u64,
    min_long_token_amount: u64,
    min_short_token_amount: u64,
    market_token_account: Option<Pubkey>,
    final_long_token: Option<Pubkey>,
    final_short_token: Option<Pubkey>,
    final_long_token_receiver: Option<Pubkey>,
    final_short_token_receiver: Option<Pubkey>,
    long_token_swap_path: Vec<Pubkey>,
    short_token_swap_path: Vec<Pubkey>,
    token_map: Option<Pubkey>,
    should_unwrap_native_token: bool,
    receiver: Pubkey,
}

impl<'a, C, S> CreateWithdrawalBuilder<'a, C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    pub(super) fn new(
        client: &'a crate::Client<C>,
        store: Pubkey,
        market_token: Pubkey,
        amount: u64,
    ) -> Self {
        Self {
            client,
            store,
            market_token,
            nonce: None,
            execution_fee: Withdrawal::MIN_EXECUTION_LAMPORTS,
            amount,
            min_long_token_amount: 0,
            min_short_token_amount: 0,
            market_token_account: None,
            final_long_token: None,
            final_short_token: None,
            final_long_token_receiver: None,
            final_short_token_receiver: None,
            long_token_swap_path: vec![],
            short_token_swap_path: vec![],
            token_map: None,
            should_unwrap_native_token: true,
            receiver: client.payer(),
        }
    }

    /// Set the nonce.
    pub fn nonce(&mut self, nonce: NonceBytes) -> &mut Self {
        self.nonce = Some(nonce);
        self
    }

    /// Set extra exectuion fee allowed to use.
    ///
    /// Defaults to `0` means only allowed to use at most `rent-exempt` amount of fee.
    pub fn execution_fee(&mut self, amount: u64) -> &mut Self {
        self.execution_fee = amount;
        self
    }

    /// Set min final long token amount.
    pub fn min_final_long_token_amount(&mut self, amount: u64) -> &mut Self {
        self.min_long_token_amount = amount;
        self
    }

    /// Set min final short token amount.
    pub fn min_final_short_token_amount(&mut self, amount: u64) -> &mut Self {
        self.min_short_token_amount = amount;
        self
    }

    /// Set market token source account to the given.
    pub fn market_token_account(&mut self, account: &Pubkey) -> &mut Self {
        self.market_token_account = Some(*account);
        self
    }

    /// Set final long token params.
    pub fn final_long_token(
        &mut self,
        token: &Pubkey,
        token_account: Option<&Pubkey>,
    ) -> &mut Self {
        self.final_long_token = Some(*token);
        self.final_long_token_receiver = token_account.copied();
        self
    }

    /// Set final short token params.
    pub fn final_short_token(
        &mut self,
        token: &Pubkey,
        token_account: Option<&Pubkey>,
    ) -> &mut Self {
        self.final_short_token = Some(*token);
        self.final_short_token_receiver = token_account.copied();
        self
    }

    /// Set long swap path.
    pub fn long_token_swap_path(&mut self, market_tokens: Vec<Pubkey>) -> &mut Self {
        self.long_token_swap_path = market_tokens;
        self
    }

    /// Set short swap path.
    pub fn short_token_swap_path(&mut self, market_tokens: Vec<Pubkey>) -> &mut Self {
        self.short_token_swap_path = market_tokens;
        self
    }

    /// Set whether to unwrap native token.
    /// Defaults to should unwrap.
    pub fn should_unwrap_native_token(&mut self, should_unwrap: bool) -> &mut Self {
        self.should_unwrap_native_token = should_unwrap;
        self
    }

    /// Set receiver.
    /// Defaults to the payer.
    pub fn receiver(&mut self, receiver: Pubkey) -> &mut Self {
        self.receiver = receiver;
        self
    }

    fn get_or_find_associated_market_token_account(&self) -> Pubkey {
        match self.market_token_account {
            Some(account) => account,
            None => get_associated_token_address(&self.client.payer(), &self.market_token),
        }
    }

    async fn get_or_fetch_final_tokens(&self, market: &Pubkey) -> crate::Result<(Pubkey, Pubkey)> {
        if let (Some(long_token), Some(short_token)) =
            (self.final_long_token, self.final_short_token)
        {
            return Ok((long_token, short_token));
        }
        let market = self.client.market(market).await?;
        Ok((
            self.final_long_token
                .unwrap_or_else(|| market.meta().long_token_mint),
            self.final_short_token
                .unwrap_or_else(|| market.meta().short_token_mint),
        ))
    }

    /// Set token map.
    pub fn token_map(&mut self, address: Pubkey) -> &mut Self {
        self.token_map = Some(address);
        self
    }

    /// Create the [`TransactionBuilder`] and return withdrawal address.
    pub async fn build_with_address(&self) -> crate::Result<(TransactionBuilder<'a, C>, Pubkey)> {
        let token_program_id = anchor_spl::token::ID;

        let owner = self.client.payer();
        let receiver = self.receiver;
        let nonce = self.nonce.unwrap_or_else(generate_nonce);
        let withdrawal = self
            .client
            .find_withdrawal_address(&self.store, &owner, &nonce);
        let market = self
            .client
            .find_market_address(&self.store, &self.market_token);
        let (long_token, short_token) = self.get_or_fetch_final_tokens(&market).await?;
        let market_token_escrow = get_associated_token_address(&withdrawal, &self.market_token);
        let final_long_token_escrow = get_associated_token_address(&withdrawal, &long_token);
        let final_short_token_escrow = get_associated_token_address(&withdrawal, &short_token);
        let final_long_token_ata = get_associated_token_address(&receiver, &long_token);
        let final_short_token_ata = get_associated_token_address(&receiver, &short_token);
        let prepare_escrows = self
            .client
            .prepare_associated_token_account(&long_token, &token_program_id, Some(&withdrawal))
            .merge(self.client.prepare_associated_token_account(
                &short_token,
                &token_program_id,
                Some(&withdrawal),
            ))
            .merge(self.client.prepare_associated_token_account(
                &self.market_token,
                &token_program_id,
                Some(&withdrawal),
            ));
        let prepare_final_long_token_ata = self
            .client
            .store_transaction()
            .anchor_accounts(accounts::PrepareAssociatedTokenAccount {
                payer: owner,
                owner: receiver,
                mint: long_token,
                account: final_long_token_ata,
                system_program: system_program::ID,
                token_program: anchor_spl::token::ID,
                associated_token_program: anchor_spl::associated_token::ID,
            })
            .anchor_args(instruction::PrepareAssociatedTokenAccount {});
        let prepare_final_short_token_ata = self
            .client
            .store_transaction()
            .anchor_accounts(accounts::PrepareAssociatedTokenAccount {
                payer: owner,
                owner: receiver,
                mint: short_token,
                account: final_short_token_ata,
                system_program: system_program::ID,
                token_program: anchor_spl::token::ID,
                associated_token_program: anchor_spl::associated_token::ID,
            })
            .anchor_args(instruction::PrepareAssociatedTokenAccount {});
        let create = self
            .client
            .store_transaction()
            .anchor_accounts(accounts::CreateWithdrawal {
                store: self.store,
                token_program: anchor_spl::token::ID,
                system_program: system_program::ID,
                associated_token_program: anchor_spl::associated_token::ID,
                market,
                withdrawal,
                owner,
                receiver,
                market_token: self.market_token,
                final_long_token: long_token,
                final_short_token: short_token,
                market_token_escrow,
                final_long_token_escrow,
                final_short_token_escrow,
                market_token_source: self.get_or_find_associated_market_token_account(),
            })
            .anchor_args(instruction::CreateWithdrawal {
                nonce,
                params: CreateWithdrawalParams {
                    market_token_amount: self.amount,
                    execution_lamports: self.execution_fee,
                    min_long_token_amount: self.min_long_token_amount,
                    min_short_token_amount: self.min_short_token_amount,
                    long_token_swap_path_length: self
                        .long_token_swap_path
                        .len()
                        .try_into()
                        .map_err(|_| crate::Error::NumberOutOfRange)?,
                    short_token_swap_path_length: self
                        .short_token_swap_path
                        .len()
                        .try_into()
                        .map_err(|_| crate::Error::NumberOutOfRange)?,
                    should_unwrap_native_token: self.should_unwrap_native_token,
                },
            })
            .accounts(
                self.long_token_swap_path
                    .iter()
                    .chain(self.short_token_swap_path.iter())
                    .map(|mint| AccountMeta {
                        pubkey: self.client.find_market_address(&self.store, mint),
                        is_signer: false,
                        is_writable: false,
                    })
                    .collect::<Vec<_>>(),
            );

        Ok((
            prepare_escrows
                .merge(prepare_final_long_token_ata)
                .merge(prepare_final_short_token_ata)
                .merge(create),
            withdrawal,
        ))
    }
}

/// Close Withdrawal Builder.
pub struct CloseWithdrawalBuilder<'a, C> {
    client: &'a crate::Client<C>,
    store: Pubkey,
    withdrawal: Pubkey,
    reason: String,
    hint: Option<CloseWithdrawalHint>,
}

#[derive(Clone, Copy)]
pub struct CloseWithdrawalHint {
    owner: Pubkey,
    receiver: Pubkey,
    market_token: Pubkey,
    final_long_token: Pubkey,
    final_short_token: Pubkey,
    market_token_account: Pubkey,
    final_long_token_account: Pubkey,
    final_short_token_account: Pubkey,
    should_unwrap_native_token: bool,
}

impl<'a> From<&'a Withdrawal> for CloseWithdrawalHint {
    fn from(withdrawal: &'a Withdrawal) -> Self {
        let tokens = withdrawal.tokens();
        Self {
            owner: *withdrawal.header().owner(),
            receiver: withdrawal.header().receiver(),
            market_token: tokens.market_token(),
            final_long_token: tokens.final_long_token(),
            final_short_token: tokens.final_short_token(),
            market_token_account: tokens.market_token_account(),
            final_long_token_account: tokens.final_long_token_account(),
            final_short_token_account: tokens.final_short_token_account(),
            should_unwrap_native_token: withdrawal.header().should_unwrap_native_token(),
        }
    }
}

impl<'a, S, C> CloseWithdrawalBuilder<'a, C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    pub(super) fn new(client: &'a crate::Client<C>, store: &Pubkey, withdrawal: &Pubkey) -> Self {
        Self {
            client,
            store: *store,
            withdrawal: *withdrawal,
            reason: "cancelled".to_string(),
            hint: None,
        }
    }

    /// Set hint.
    pub fn hint(&mut self, hint: CloseWithdrawalHint) -> &mut Self {
        self.hint = Some(hint);
        self
    }

    async fn get_or_fetch_withdrawal_hint(&self) -> crate::Result<CloseWithdrawalHint> {
        match &self.hint {
            Some(hint) => Ok(*hint),
            None => {
                let withdrawal: ZeroCopy<Withdrawal> = self
                    .client
                    .account(&self.withdrawal)
                    .await?
                    .ok_or(crate::Error::NotFound)?;
                Ok((&withdrawal.0).into())
            }
        }
    }

    /// Set close reason.
    pub fn reason(&mut self, reason: impl ToString) -> &mut Self {
        self.reason = reason.to_string();
        self
    }

    /// Build a [`TransactionBuilder`] for `close_withdrawal` instruction.
    pub async fn build(&self) -> crate::Result<TransactionBuilder<'a, C>> {
        let payer = self.client.payer();
        let hint = self.get_or_fetch_withdrawal_hint().await?;
        let market_token_ata = get_associated_token_address(&hint.owner, &hint.market_token);
        let final_long_token_ata = get_ata_or_owner(
            &hint.receiver,
            &hint.final_long_token,
            hint.should_unwrap_native_token,
        );
        let final_short_token_ata = get_ata_or_owner(
            &hint.receiver,
            &hint.final_short_token,
            hint.should_unwrap_native_token,
        );
        Ok(self
            .client
            .store_transaction()
            .anchor_accounts(accounts::CloseWithdrawal {
                store: self.store,
                store_wallet: self.client.find_store_wallet_address(&self.store),
                withdrawal: self.withdrawal,
                market_token: hint.market_token,
                token_program: anchor_spl::token::ID,
                system_program: system_program::ID,
                event_authority: self.client.store_event_authority(),
                executor: payer,
                owner: hint.owner,
                receiver: hint.receiver,
                final_long_token: hint.final_long_token,
                final_short_token: hint.final_short_token,
                market_token_escrow: hint.market_token_account,
                final_long_token_escrow: hint.final_long_token_account,
                final_short_token_escrow: hint.final_short_token_account,
                market_token_ata,
                final_long_token_ata,
                final_short_token_ata,
                associated_token_program: anchor_spl::associated_token::ID,
                program: *self.client.store_program_id(),
            })
            .anchor_args(instruction::CloseWithdrawal {
                reason: self.reason.clone(),
            }))
    }
}

/// Execute Withdrawal Builder.
pub struct ExecuteWithdrawalBuilder<'a, C> {
    client: &'a crate::Client<C>,
    store: Pubkey,
    oracle: Pubkey,
    withdrawal: Pubkey,
    execution_fee: u64,
    price_provider: Pubkey,
    hint: Option<ExecuteWithdrawalHint>,
    feeds_parser: FeedsParser,
    token_map: Option<Pubkey>,
    cancel_on_execution_error: bool,
    close: bool,
}

/// Hint for withdrawal execution.
#[derive(Clone)]
pub struct ExecuteWithdrawalHint {
    owner: Pubkey,
    receiver: Pubkey,
    market_token: Pubkey,
    market_token_escrow: Pubkey,
    final_long_token_escrow: Pubkey,
    final_short_token_escrow: Pubkey,
    final_long_token: Pubkey,
    final_short_token: Pubkey,
    /// Feeds.
    pub feeds: TokensWithFeed,
    swap: SwapActionParams,
    should_unwrap_native_token: bool,
}

impl ExecuteWithdrawalHint {
    /// Create a new hint for the execution.
    pub fn new(withdrawal: &Withdrawal, map: &impl TokenMapAccess) -> crate::Result<Self> {
        let tokens = withdrawal.tokens();
        let swap = withdrawal.swap();
        Ok(Self {
            owner: *withdrawal.header().owner(),
            receiver: withdrawal.header().receiver(),
            market_token: tokens.market_token(),
            market_token_escrow: tokens.market_token_account(),
            final_long_token_escrow: tokens.final_long_token_account(),
            final_short_token_escrow: tokens.final_short_token_account(),
            final_long_token: tokens.final_long_token(),
            final_short_token: tokens.final_short_token(),
            feeds: swap.to_feeds(map)?,
            swap: *swap,
            should_unwrap_native_token: withdrawal.header().should_unwrap_native_token(),
        })
    }
}

impl<'a, S, C> ExecuteWithdrawalBuilder<'a, C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    pub(super) fn new(
        client: &'a crate::Client<C>,
        store: &Pubkey,
        oracle: &Pubkey,
        withdrawal: &Pubkey,
        cancel_on_execution_error: bool,
    ) -> Self {
        Self {
            client,
            store: *store,
            oracle: *oracle,
            withdrawal: *withdrawal,
            execution_fee: 0,
            price_provider: Pyth::id(),
            hint: None,
            feeds_parser: Default::default(),
            token_map: None,
            cancel_on_execution_error,
            close: true,
        }
    }

    /// Set price provider to the given.
    pub fn price_provider(&mut self, program: Pubkey) -> &mut Self {
        self.price_provider = program;
        self
    }

    /// Set whether to close the withdrawal after execution.
    pub fn close(&mut self, close: bool) -> &mut Self {
        self.close = close;
        self
    }

    /// Set hint with the given withdrawal.
    pub fn hint(
        &mut self,
        withdrawal: &Withdrawal,
        map: &impl TokenMapAccess,
    ) -> crate::Result<&mut Self> {
        self.hint = Some(ExecuteWithdrawalHint::new(withdrawal, map)?);
        Ok(self)
    }

    /// Parse feeds with the given price updates map.
    #[cfg(feature = "pyth-pull-oracle")]
    pub fn parse_with_pyth_price_updates(&mut self, price_updates: Prices) -> &mut Self {
        self.feeds_parser.with_pyth_price_updates(price_updates);
        self
    }

    /// Prepare [`ExecuteWithdrawalHint`].
    pub async fn prepare_hint(&mut self) -> crate::Result<ExecuteWithdrawalHint> {
        match &self.hint {
            Some(hint) => Ok(hint.clone()),
            None => {
                let map = self.client.authorized_token_map(&self.store).await?;
                let withdrawal: ZeroCopy<Withdrawal> = self
                    .client
                    .account(&self.withdrawal)
                    .await?
                    .ok_or(crate::Error::NotFound)?;
                let hint = ExecuteWithdrawalHint::new(&withdrawal.0, &map)?;
                self.hint = Some(hint.clone());
                Ok(hint)
            }
        }
    }

    async fn get_token_map(&self) -> crate::Result<Pubkey> {
        if let Some(address) = self.token_map {
            Ok(address)
        } else {
            Ok(self
                .client
                .authorized_token_map_address(&self.store)
                .await?
                .ok_or(crate::Error::NotFound)?)
        }
    }

    /// Set token map.
    pub fn token_map(&mut self, address: Pubkey) -> &mut Self {
        self.token_map = Some(address);
        self
    }

    async fn build_txn(&mut self) -> crate::Result<TransactionBuilder<'a, C>> {
        let authority = self.client.payer();
        let hint = self.prepare_hint().await?;
        let feeds = self
            .feeds_parser
            .parse(&hint.feeds)
            .collect::<Result<Vec<_>, _>>()?;
        let swap_path_markets = hint
            .swap
            .unique_market_tokens_excluding_current(&hint.market_token)
            .map(|mint| AccountMeta {
                pubkey: self.client.find_market_address(&self.store, mint),
                is_signer: false,
                is_writable: true,
            });
        let execute = self
            .client
            .store_transaction()
            .accounts(fix_optional_account_metas(
                accounts::ExecuteWithdrawal {
                    authority,
                    store: self.store,
                    token_program: anchor_spl::token::ID,
                    system_program: system_program::ID,
                    oracle: self.oracle,
                    token_map: self.get_token_map().await?,
                    withdrawal: self.withdrawal,
                    market: self
                        .client
                        .find_market_address(&self.store, &hint.market_token),
                    final_long_token_vault: self
                        .client
                        .find_market_vault_address(&self.store, &hint.final_long_token),
                    final_short_token_vault: self
                        .client
                        .find_market_vault_address(&self.store, &hint.final_short_token),
                    market_token: hint.market_token,
                    final_long_token: hint.final_long_token,
                    final_short_token: hint.final_short_token,
                    market_token_escrow: hint.market_token_escrow,
                    final_long_token_escrow: hint.final_long_token_escrow,
                    final_short_token_escrow: hint.final_short_token_escrow,
                    market_token_vault: self
                        .client
                        .find_market_vault_address(&self.store, &hint.market_token),
                    chainlink_program: None,
                    event_authority: self.client.store_event_authority(),
                    program: *self.client.store_program_id(),
                },
                &crate::program_ids::DEFAULT_GMSOL_STORE_ID,
                self.client.store_program_id(),
            ))
            .anchor_args(instruction::ExecuteWithdrawal {
                execution_fee: self.execution_fee,
                throw_on_execution_error: !self.cancel_on_execution_error,
            })
            .accounts(
                feeds
                    .into_iter()
                    .chain(swap_path_markets)
                    .collect::<Vec<_>>(),
            )
            .compute_budget(ComputeBudget::default().with_limit(EXECUTE_WITHDRAWAL_COMPUTE_BUDGET));
        let rpc = if self.close {
            let close = self
                .client
                .close_withdrawal(&self.store, &self.withdrawal)
                .hint(CloseWithdrawalHint {
                    owner: hint.owner,
                    receiver: hint.receiver,
                    market_token: hint.market_token,
                    final_long_token: hint.final_long_token,
                    final_short_token: hint.final_short_token,
                    market_token_account: hint.market_token_escrow,
                    final_long_token_account: hint.final_long_token_escrow,
                    final_short_token_account: hint.final_short_token_escrow,
                    should_unwrap_native_token: hint.should_unwrap_native_token,
                })
                .reason("executed")
                .build()
                .await?;
            execute.merge(close)
        } else {
            execute
        };

        Ok(rpc)
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> MakeBundleBuilder<'a, C>
    for ExecuteWithdrawalBuilder<'a, C>
{
    async fn build_with_options(
        &mut self,
        options: BundleOptions,
    ) -> gmsol_solana_utils::Result<BundleBuilder<'a, C>> {
        let mut tx = self.client.bundle_with_options(options);

        tx.try_push(
            self.build_txn()
                .await
                .map_err(gmsol_solana_utils::Error::custom)?,
        )?;

        Ok(tx)
    }
}

impl<C: Deref<Target = impl Signer> + Clone> PullOraclePriceConsumer
    for ExecuteWithdrawalBuilder<'_, C>
{
    async fn feed_ids(&mut self) -> crate::Result<FeedIds> {
        let hint = self.prepare_hint().await?;
        Ok(FeedIds::new(self.store, hint.feeds))
    }

    fn process_feeds(
        &mut self,
        provider: PriceProviderKind,
        map: FeedAddressMap,
    ) -> crate::Result<()> {
        self.feeds_parser
            .insert_pull_oracle_feed_parser(provider, map);
        Ok(())
    }
}

impl<C> SetExecutionFee for ExecuteWithdrawalBuilder<'_, C> {
    fn set_execution_fee(&mut self, lamports: u64) -> &mut Self {
        self.execution_fee = lamports;
        self
    }
}
