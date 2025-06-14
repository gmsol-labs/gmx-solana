use std::{collections::BTreeSet, ops::Deref};

use anchor_spl::associated_token::get_associated_token_address;
use gmsol_programs::gmsol_store::{
    accounts::Deposit,
    client::{accounts, args},
    types::CreateDepositParams,
    ID,
};
use gmsol_solana_utils::{
    bundle_builder::{BundleBuilder, BundleOptions},
    compute_budget::ComputeBudget,
    make_bundle_builder::{MakeBundleBuilder, SetExecutionFee},
    transaction_builder::TransactionBuilder,
};
use gmsol_utils::{
    action::ActionFlag,
    oracle::PriceProviderKind,
    pubkey::optional_address,
    swap::SwapActionParams,
    token_config::{TokenMapAccess, TokensWithFeed},
};
use solana_sdk::{instruction::AccountMeta, pubkey::Pubkey, signer::Signer, system_program};

use crate::{
    builders::utils::{generate_nonce, get_ata_or_owner},
    client::{
        feeds_parser::{FeedAddressMap, FeedsParser},
        ops::token_account::TokenAccountOps,
        pull_oracle::{FeedIds, PullOraclePriceConsumer},
        Client,
    },
    pda::NonceBytes,
    utils::optional::fix_optional_account_metas,
};

use super::{ExchangeOps, VirtualInventoryCollector};

/// Compute unit limit for `execute_deposit`
pub const EXECUTE_DEPOSIT_COMPUTE_BUDGET: u32 = 400_000;

/// Min execution lamports for deposit.
pub const MIN_EXECUTION_LAMPORTS: u64 = 200_000;

/// Create Deposit Builder.
pub struct CreateDepositBuilder<'a, C> {
    client: &'a Client<C>,
    store: Pubkey,
    market_token: Pubkey,
    execution_fee: u64,
    long_token_swap_path: Vec<Pubkey>,
    short_token_swap_path: Vec<Pubkey>,
    initial_long_token: Option<Pubkey>,
    initial_short_token: Option<Pubkey>,
    initial_long_token_account: Option<Pubkey>,
    initial_short_token_account: Option<Pubkey>,
    initial_long_token_amount: u64,
    initial_short_token_amount: u64,
    min_market_token: u64,
    receiver: Option<Pubkey>,
    nonce: Option<NonceBytes>,
    should_unwrap_native_token: bool,
}

impl<C> CreateDepositBuilder<'_, C> {
    /// Set the nonce.
    pub fn nonce(&mut self, nonce: NonceBytes) -> &mut Self {
        self.nonce = Some(nonce);
        self
    }

    /// Set execution fee. Defaults to min execution fee.
    pub fn execution_fee(&mut self, fee: u64) -> &mut Self {
        self.execution_fee = fee;
        self
    }

    /// Set min market token to mint.
    pub fn min_market_token(&mut self, amount: u64) -> &mut Self {
        self.min_market_token = amount;
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

    /// Set recevier.
    /// Defaults to the payer.
    pub fn receiver(&mut self, receiver: Option<Pubkey>) -> &mut Self {
        self.receiver = receiver;
        self
    }

    /// Set whether to unwrap native token.
    /// Defaults to should unwrap.
    pub fn should_unwrap_native_token(&mut self, should_unwrap: bool) -> &mut Self {
        self.should_unwrap_native_token = should_unwrap;
        self
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> CreateDepositBuilder<'a, C> {
    pub(super) fn new(client: &'a Client<C>, store: Pubkey, market_token: Pubkey) -> Self {
        Self {
            client,
            store,
            market_token,
            execution_fee: MIN_EXECUTION_LAMPORTS,
            long_token_swap_path: vec![],
            short_token_swap_path: vec![],
            initial_long_token: None,
            initial_short_token: None,
            initial_long_token_account: None,
            initial_short_token_account: None,
            initial_long_token_amount: 0,
            initial_short_token_amount: 0,
            min_market_token: 0,
            receiver: None,
            nonce: None,
            should_unwrap_native_token: true,
        }
    }

    fn get_receiver(&self) -> Pubkey {
        self.receiver.unwrap_or(self.client.payer())
    }

    fn get_or_find_associated_initial_long_token_account(
        &self,
        token: Option<&Pubkey>,
    ) -> Option<Pubkey> {
        let token = token?;
        match self.initial_long_token_account {
            Some(account) => Some(account),
            None => Some(get_associated_token_address(&self.client.payer(), token)),
        }
    }

    fn get_or_find_associated_initial_short_token_account(
        &self,
        token: Option<&Pubkey>,
    ) -> Option<Pubkey> {
        let token = token?;
        match self.initial_short_token_account {
            Some(account) => Some(account),
            None => Some(get_associated_token_address(&self.client.payer(), token)),
        }
    }

    async fn get_or_fetch_initial_tokens(
        &self,
        market: &Pubkey,
    ) -> crate::Result<(Option<Pubkey>, Option<Pubkey>)> {
        let res = match (
            self.initial_long_token,
            self.initial_long_token_amount,
            self.initial_short_token,
            self.initial_short_token_amount,
        ) {
            (Some(long_token), _, Some(short_token), _) => (Some(long_token), Some(short_token)),
            (_, 0, _, 0) => {
                return Err(crate::Error::custom("empty deposit"));
            }
            (None, 0, Some(short_token), _) => (None, Some(short_token)),
            (Some(long_token), _, None, 0) => (Some(long_token), None),
            (mut long_token, long_amount, mut short_token, short_amount) => {
                debug_assert!(
                    (long_token.is_none() && long_amount != 0)
                        || (short_token.is_none() && short_amount != 0)
                );
                let market = self.client.market(market).await?;
                if long_amount != 0 && long_token.is_none() {
                    long_token = Some(market.meta.long_token_mint);
                }
                if short_amount != 0 && short_token.is_none() {
                    short_token = Some(market.meta.short_token_mint);
                }
                (long_token, short_token)
            }
        };
        Ok(res)
    }

    /// Set the initial long token params for deposit.
    pub fn long_token(
        &mut self,
        amount: u64,
        token: Option<&Pubkey>,
        token_account: Option<&Pubkey>,
    ) -> &mut Self {
        self.initial_long_token = token.copied();
        self.initial_long_token_amount = amount;
        self.initial_long_token_account = token_account.copied();
        self
    }

    /// Set the initial short token params for deposit.
    pub fn short_token(
        &mut self,
        amount: u64,
        token: Option<&Pubkey>,
        token_account: Option<&Pubkey>,
    ) -> &mut Self {
        self.initial_short_token = token.cloned();
        self.initial_short_token_amount = amount;
        self.initial_short_token_account = token_account.copied();
        self
    }

    /// Build a [`TransactionBuilder`] and return deposit address.
    pub async fn build_with_address(&self) -> crate::Result<(TransactionBuilder<'a, C>, Pubkey)> {
        let token_program_id = anchor_spl::token::ID;
        let Self {
            client,
            store,
            nonce,
            market_token,
            execution_fee,
            long_token_swap_path,
            short_token_swap_path,
            initial_long_token_amount,
            initial_short_token_amount,
            min_market_token,
            should_unwrap_native_token,
            ..
        } = self;
        let nonce = nonce.unwrap_or_else(|| generate_nonce().to_bytes());
        let owner = client.payer();
        let receiver = self.get_receiver();
        let deposit = client.find_deposit_address(store, &owner, &nonce);
        let market = client.find_market_address(store, market_token);

        let (long_token, short_token) = self.get_or_fetch_initial_tokens(&market).await?;

        let initial_long_token_account =
            self.get_or_find_associated_initial_long_token_account(long_token.as_ref());
        let initial_short_token_account =
            self.get_or_find_associated_initial_short_token_account(short_token.as_ref());
        let market_token_ata = get_associated_token_address(&receiver, market_token);

        let market_token_escrow = get_associated_token_address(&deposit, market_token);
        let initial_long_token_escrow = long_token
            .as_ref()
            .map(|mint| get_associated_token_address(&deposit, mint));
        let initial_short_token_escrow = short_token
            .as_ref()
            .map(|mint| get_associated_token_address(&deposit, mint));

        let mut prepare = client.prepare_associated_token_account(
            market_token,
            &token_program_id,
            Some(&deposit),
        );

        for token in long_token.iter().chain(short_token.iter()) {
            prepare = prepare.merge(client.prepare_associated_token_account(
                token,
                &token_program_id,
                Some(&deposit),
            ));
        }

        let create = client
            .store_transaction()
            .accounts(fix_optional_account_metas(
                accounts::CreateDeposit {
                    owner,
                    receiver,
                    store: *store,
                    market,
                    deposit,
                    market_token: *market_token,
                    initial_long_token: long_token,
                    initial_short_token: short_token,
                    market_token_ata,
                    market_token_escrow,
                    initial_long_token_escrow,
                    initial_short_token_escrow,
                    initial_long_token_source: initial_long_token_account,
                    initial_short_token_source: initial_short_token_account,
                    system_program: system_program::ID,
                    token_program: token_program_id,
                    associated_token_program: anchor_spl::associated_token::ID,
                },
                &ID,
                client.store_program_id(),
            ))
            .anchor_args(args::CreateDeposit {
                nonce,
                params: CreateDepositParams {
                    execution_lamports: *execution_fee,
                    long_token_swap_length: long_token_swap_path
                        .len()
                        .try_into()
                        .map_err(|_| crate::Error::custom("number out of range"))?,
                    short_token_swap_length: short_token_swap_path
                        .len()
                        .try_into()
                        .map_err(|_| crate::Error::custom("number out of range"))?,
                    initial_long_token_amount: *initial_long_token_amount,
                    initial_short_token_amount: *initial_short_token_amount,
                    min_market_token_amount: *min_market_token,
                    should_unwrap_native_token: *should_unwrap_native_token,
                },
            })
            .accounts(
                long_token_swap_path
                    .iter()
                    .enumerate()
                    .map(|(idx, mint)| AccountMeta {
                        pubkey: client.find_market_address(store, mint),
                        is_signer: false,
                        is_writable: idx == 0,
                    })
                    .chain(short_token_swap_path.iter().enumerate().map(|(idx, mint)| {
                        AccountMeta {
                            pubkey: client.find_market_address(store, mint),
                            is_signer: false,
                            is_writable: idx == 0,
                        }
                    }))
                    .collect::<Vec<_>>(),
            );
        let builder = prepare.merge(create);
        Ok((builder, deposit))
    }
}

/// Close Deposit Builder.
pub struct CloseDepositBuilder<'a, C> {
    client: &'a Client<C>,
    store: Pubkey,
    deposit: Pubkey,
    reason: String,
    hint: Option<CloseDepositHint>,
}

/// Close Deposit Hint.
#[derive(Debug, Clone, Copy)]
pub struct CloseDepositHint {
    owner: Pubkey,
    receiver: Pubkey,
    market_token: Pubkey,
    market_token_account: Pubkey,
    initial_long_token: Option<Pubkey>,
    initial_short_token: Option<Pubkey>,
    initial_long_token_account: Option<Pubkey>,
    initial_short_token_account: Option<Pubkey>,
    should_unwrap_native_token: bool,
}

impl CloseDepositHint {
    /// Create from deposit.
    pub fn new(deposit: &Deposit) -> Self {
        Self {
            owner: deposit.header.owner,
            receiver: deposit.header.receiver,
            market_token: deposit.tokens.market_token.token,
            market_token_account: deposit.tokens.market_token.account,
            initial_long_token: optional_address(&deposit.tokens.initial_long_token.token).copied(),
            initial_short_token: optional_address(&deposit.tokens.initial_short_token.token)
                .copied(),
            initial_long_token_account: optional_address(
                &deposit.tokens.initial_long_token.account,
            )
            .copied(),
            initial_short_token_account: optional_address(
                &deposit.tokens.initial_short_token.account,
            )
            .copied(),
            should_unwrap_native_token: deposit
                .header
                .flags
                .get_flag(ActionFlag::ShouldUnwrapNativeToken),
        }
    }
}

impl<'a, S, C> CloseDepositBuilder<'a, C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    pub(super) fn new(client: &'a Client<C>, store: &Pubkey, deposit: &Pubkey) -> Self {
        Self {
            client,
            store: *store,
            deposit: *deposit,
            reason: "cancelled".to_string(),
            hint: None,
        }
    }

    /// Set hint with the given deposit.
    pub fn hint_with_deposit(&mut self, deposit: &Deposit) -> &mut Self {
        self.hint(CloseDepositHint::new(deposit))
    }

    /// Set hint.
    pub fn hint(&mut self, hint: CloseDepositHint) -> &mut Self {
        self.hint = Some(hint);
        self
    }

    /// Set the close reason.
    pub fn reason(&mut self, reason: impl ToString) -> &mut Self {
        self.reason = reason.to_string();
        self
    }

    async fn get_or_fetch_deposit_info(&self) -> crate::Result<CloseDepositHint> {
        match &self.hint {
            Some(hint) => Ok(*hint),
            None => {
                let deposit = self.client.deposit(&self.deposit).await?;
                Ok(CloseDepositHint::new(&deposit))
            }
        }
    }

    /// Build a [`TransactionBuilder`] for `cancel_deposit` instruction.
    pub async fn build(&self) -> crate::Result<TransactionBuilder<'a, C>> {
        let executor = self.client.payer();
        let hint = self.get_or_fetch_deposit_info().await?;
        let Self {
            client,
            store,
            deposit,
            ..
        } = self;
        let owner = hint.owner;
        let receiver = hint.receiver;
        let market_token_ata = get_associated_token_address(&receiver, &hint.market_token);
        let should_unwrap_native_token = hint.should_unwrap_native_token;
        let initial_long_token_ata = hint
            .initial_long_token
            .as_ref()
            .map(|mint| get_ata_or_owner(&owner, mint, should_unwrap_native_token));
        let initial_short_token_ata = hint
            .initial_short_token
            .as_ref()
            .map(|mint| get_ata_or_owner(&owner, mint, should_unwrap_native_token));
        Ok(client
            .store_transaction()
            .accounts(fix_optional_account_metas(
                accounts::CloseDeposit {
                    executor,
                    store: *store,
                    store_wallet: client.find_store_wallet_address(store),
                    owner,
                    receiver,
                    market_token: hint.market_token,
                    initial_long_token: hint.initial_long_token,
                    initial_short_token: hint.initial_short_token,
                    deposit: *deposit,
                    market_token_escrow: hint.market_token_account,
                    initial_long_token_escrow: hint.initial_long_token_account,
                    initial_short_token_escrow: hint.initial_short_token_account,
                    market_token_ata,
                    initial_long_token_ata,
                    initial_short_token_ata,
                    associated_token_program: anchor_spl::associated_token::ID,
                    token_program: anchor_spl::token::ID,
                    system_program: system_program::ID,
                    event_authority: client.store_event_authority(),
                    program: *client.store_program_id(),
                },
                &ID,
                client.store_program_id(),
            ))
            .anchor_args(args::CloseDeposit {
                reason: self.reason.clone(),
            }))
    }
}

/// Execute Deposit Builder.
pub struct ExecuteDepositBuilder<'a, C> {
    client: &'a Client<C>,
    store: Pubkey,
    oracle: Pubkey,
    deposit: Pubkey,
    execution_fee: u64,
    feeds_parser: FeedsParser,
    hint: Option<ExecuteDepositHint>,
    token_map: Option<Pubkey>,
    cancel_on_execution_error: bool,
    close: bool,
}

/// Hint for executing deposit.
#[derive(Clone, Debug)]
pub struct ExecuteDepositHint {
    owner: Pubkey,
    receiver: Pubkey,
    market_token_escrow: Pubkey,
    market_token_mint: Pubkey,
    /// Feeds.
    pub feeds: TokensWithFeed,
    swap: SwapActionParams,
    initial_long_token_escrow: Option<Pubkey>,
    initial_short_token_escrow: Option<Pubkey>,
    initial_long_token: Option<Pubkey>,
    initial_short_token: Option<Pubkey>,
    should_unwrap_native_token: bool,
    virtual_inventories: BTreeSet<Pubkey>,
}

impl ExecuteDepositHint {
    /// Create a new hint for the deposit.
    pub fn new(
        deposit: &Deposit,
        map: &impl TokenMapAccess,
        virtual_inventories: BTreeSet<Pubkey>,
    ) -> crate::Result<Self> {
        let CloseDepositHint {
            owner,
            receiver,
            market_token,
            market_token_account,
            initial_long_token,
            initial_short_token,
            initial_long_token_account,
            initial_short_token_account,
            should_unwrap_native_token,
        } = CloseDepositHint::new(deposit);
        let swap: SwapActionParams = deposit.swap.into();
        Ok(Self {
            owner,
            receiver,
            market_token_escrow: market_token_account,
            market_token_mint: market_token,
            feeds: swap.to_feeds(map).map_err(crate::Error::custom)?,
            swap,
            initial_long_token,
            initial_short_token,
            initial_long_token_escrow: initial_long_token_account,
            initial_short_token_escrow: initial_short_token_account,
            should_unwrap_native_token,
            virtual_inventories,
        })
    }
}

impl<'a, S, C> ExecuteDepositBuilder<'a, C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    pub(super) fn new(
        client: &'a Client<C>,
        store: &Pubkey,
        oracle: &Pubkey,
        deposit: &Pubkey,
        cancel_on_execution_error: bool,
    ) -> Self {
        Self {
            client,
            store: *store,
            oracle: *oracle,
            deposit: *deposit,
            execution_fee: 0,
            hint: None,
            feeds_parser: Default::default(),
            token_map: None,
            cancel_on_execution_error,
            close: true,
        }
    }

    /// Set whether to close the deposit after execution.
    pub fn close(&mut self, close: bool) -> &mut Self {
        self.close = close;
        self
    }

    /// Set hint with the given deposit.
    pub fn hint(
        &mut self,
        deposit: &Deposit,
        map: &impl TokenMapAccess,
        virtual_inventories: BTreeSet<Pubkey>,
    ) -> crate::Result<&mut Self> {
        self.hint = Some(ExecuteDepositHint::new(deposit, map, virtual_inventories)?);
        Ok(self)
    }

    /// Prepare [`ExecuteDepositHint`].
    pub async fn prepare_hint(&mut self) -> crate::Result<ExecuteDepositHint> {
        match &self.hint {
            Some(hint) => Ok(hint.clone()),
            None => {
                let map = self.client.authorized_token_map(&self.store).await?;
                let deposit = self.client.deposit(&self.deposit).await?;
                let swap = deposit.swap.into();
                let virtual_inventories = VirtualInventoryCollector::from_swap(&swap)
                    .collect(self.client, &self.store)
                    .await?;
                let hint = ExecuteDepositHint::new(&deposit, &map, virtual_inventories)?;
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
        let token_map = self.get_token_map().await?;
        let hint = self.prepare_hint().await?;
        let Self {
            client,
            store,
            oracle,
            deposit,
            execution_fee,
            cancel_on_execution_error,
            ..
        } = &self;
        let authority = client.payer();
        let feeds = self
            .feeds_parser
            .parse(&hint.feeds)
            .collect::<Result<Vec<_>, _>>()?;
        let markets = hint
            .swap
            .unique_market_tokens_excluding_current(&hint.market_token_mint)
            .map(|mint| AccountMeta {
                pubkey: client.find_market_address(store, mint),
                is_signer: false,
                is_writable: true,
            });
        let virtual_inventories = hint
            .virtual_inventories
            .iter()
            .map(|pubkey| AccountMeta::new(*pubkey, false));

        // Execution.
        let execute = client
            .store_transaction()
            .accounts(fix_optional_account_metas(
                accounts::ExecuteDeposit {
                    authority,
                    store: *store,
                    oracle: *oracle,
                    token_map,
                    deposit: *deposit,
                    market: client.find_market_address(store, &hint.market_token_mint),
                    market_token: hint.market_token_mint,
                    initial_long_token_vault: hint
                        .initial_long_token
                        .as_ref()
                        .map(|token| client.find_market_vault_address(store, token)),
                    initial_short_token_vault: hint
                        .initial_short_token
                        .as_ref()
                        .map(|token| client.find_market_vault_address(store, token)),
                    token_program: anchor_spl::token::ID,
                    system_program: system_program::ID,
                    initial_long_token: hint.initial_long_token,
                    initial_short_token: hint.initial_short_token,
                    market_token_escrow: hint.market_token_escrow,
                    initial_long_token_escrow: hint.initial_long_token_escrow,
                    initial_short_token_escrow: hint.initial_short_token_escrow,
                    chainlink_program: None,
                    event_authority: client.store_event_authority(),
                    program: *client.store_program_id(),
                },
                &ID,
                self.client.store_program_id(),
            ))
            .anchor_args(args::ExecuteDeposit {
                execution_fee: *execution_fee,
                throw_on_execution_error: !*cancel_on_execution_error,
            })
            .accounts(
                feeds
                    .into_iter()
                    .chain(markets)
                    .chain(virtual_inventories)
                    .collect::<Vec<_>>(),
            )
            .compute_budget(ComputeBudget::default().with_limit(EXECUTE_DEPOSIT_COMPUTE_BUDGET));

        let rpc = if self.close {
            let close = self
                .client
                .close_deposit(store, deposit)
                .hint(CloseDepositHint {
                    owner: hint.owner,
                    receiver: hint.receiver,
                    market_token: hint.market_token_mint,
                    market_token_account: hint.market_token_escrow,
                    initial_long_token: hint.initial_long_token,
                    initial_short_token: hint.initial_short_token,
                    initial_long_token_account: hint.initial_long_token_escrow,
                    initial_short_token_account: hint.initial_short_token_escrow,
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
    for ExecuteDepositBuilder<'a, C>
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
    for ExecuteDepositBuilder<'_, C>
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

impl<C> SetExecutionFee for ExecuteDepositBuilder<'_, C> {
    fn set_execution_fee(&mut self, lamports: u64) -> &mut Self {
        self.execution_fee = lamports;
        self
    }
}
