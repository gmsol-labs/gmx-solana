use std::ops::Deref;

use anchor_client::{
    anchor_lang::{system_program, Id},
    solana_sdk::{instruction::AccountMeta, pubkey::Pubkey, signer::Signer},
};
use anchor_spl::associated_token::get_associated_token_address;
use gmsol_store::{
    accounts, instruction,
    ops::deposit::CreateDepositParams,
    states::{
        common::{swap::SwapParamsV2, TokensWithFeed},
        DepositV2, NonceBytes, Pyth, TokenMapAccess,
    },
};

use crate::{
    exchange::ExchangeOps,
    store::utils::{read_market, FeedsParser},
    utils::{ComputeBudget, RpcBuilder, ZeroCopy},
};

use super::generate_nonce;

#[cfg(feature = "pyth-pull-oracle")]
use crate::pyth::pull_oracle::Prices;

/// `execute_deposit` compute budget.
pub const EXECUTE_DEPOSIT_COMPUTE_BUDGET: u32 = 400_000;

/// Create Deposit Builder.
pub struct CreateDepositBuilder<'a, C> {
    client: &'a crate::Client<C>,
    store: Pubkey,
    market_token: Pubkey,
    receiver: Option<Pubkey>,
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
    nonce: Option<NonceBytes>,
    token_map: Option<Pubkey>,
}

impl<'a, C, S> CreateDepositBuilder<'a, C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    pub(super) fn new(client: &'a crate::Client<C>, store: Pubkey, market_token: Pubkey) -> Self {
        Self {
            client,
            store,
            nonce: None,
            market_token,
            receiver: None,
            execution_fee: DepositV2::MIN_EXECUTION_LAMPORTS,
            long_token_swap_path: vec![],
            short_token_swap_path: vec![],
            initial_long_token: None,
            initial_short_token: None,
            initial_long_token_account: None,
            initial_short_token_account: None,
            initial_long_token_amount: 0,
            initial_short_token_amount: 0,
            min_market_token: 0,
            token_map: None,
        }
    }

    /// Set the token account for receiving minted market tokens.
    ///
    /// Defaults to use associated token account.
    pub fn receiver(&mut self, token_account: &Pubkey) -> &mut Self {
        self.receiver = Some(*token_account);
        self
    }

    /// Set min market token to mint.
    pub fn min_market_token(&mut self, amount: u64) -> &mut Self {
        self.min_market_token = amount;
        self
    }

    /// Set exectuion fee allowed to use.
    ///
    /// Defaults to min execution fee.
    pub fn execution_fee(&mut self, fee: u64) -> &mut Self {
        self.execution_fee = fee;
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
                return Err(crate::Error::EmptyDeposit);
            }
            (None, 0, Some(short_token), _) => (None, Some(short_token)),
            (Some(long_token), _, None, 0) => (Some(long_token), None),
            (mut long_token, long_amount, mut short_token, short_amount) => {
                debug_assert!(
                    (long_token.is_none() && long_amount != 0)
                        || (short_token.is_none() && short_amount != 0)
                );
                let market = read_market(&self.client.data_store().async_rpc(), market).await?;
                if long_amount != 0 && long_token.is_none() {
                    long_token = Some(market.meta().long_token_mint);
                }
                if short_amount != 0 && short_token.is_none() {
                    short_token = Some(market.meta().short_token_mint);
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

    /// Set token map address.
    pub fn token_map(&mut self, address: Pubkey) -> &mut Self {
        self.token_map = Some(address);
        self
    }

    /// Build a [`RpcBuilder`] and return deposit address.
    pub async fn build_with_address(&self) -> crate::Result<(RpcBuilder<'a, C>, Pubkey)> {
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
            ..
        } = self;
        let nonce = nonce.unwrap_or_else(generate_nonce);
        let payer = client.payer();
        let deposit = client.find_deposit_address(store, &payer, &nonce);
        let market = client.find_market_address(store, market_token);
        let (long_token, short_token) = self.get_or_fetch_initial_tokens(&market).await?;
        let initial_long_token_account =
            self.get_or_find_associated_initial_long_token_account(long_token.as_ref());
        let initial_short_token_account =
            self.get_or_find_associated_initial_short_token_account(short_token.as_ref());
        let market_token_ata = get_associated_token_address(&payer, market_token);
        let market_token_escrow = get_associated_token_address(&deposit, market_token);
        let initial_long_token_escrow = long_token
            .as_ref()
            .map(|mint| get_associated_token_address(&deposit, mint));
        let initial_short_token_escrow = short_token
            .as_ref()
            .map(|mint| get_associated_token_address(&deposit, mint));
        let prepare = client
            .data_store_rpc()
            .accounts(crate::utils::fix_optional_account_metas(
                accounts::PrepareDepositEscrow {
                    owner: payer,
                    store: *store,
                    deposit,
                    market_token: *market_token,
                    initial_long_token: long_token,
                    initial_short_token: short_token,
                    market_token_escrow,
                    initial_long_token_escrow,
                    initial_short_token_escrow,
                    system_program: system_program::ID,
                    token_program: anchor_spl::token::ID,
                    associated_token_program: anchor_spl::associated_token::ID,
                },
                &gmsol_store::id(),
                &client.store_program_id(),
            ))
            .args(instruction::PrepareDepositEscrow { nonce });
        let create = client
            .data_store_rpc()
            .accounts(crate::utils::fix_optional_account_metas(
                accounts::CreateDeposit {
                    owner: payer,
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
                    token_program: anchor_spl::token::ID,
                    associated_token_program: anchor_spl::associated_token::ID,
                },
                &gmsol_store::id(),
                &client.store_program_id(),
            ))
            .args(instruction::CreateDeposit {
                nonce,
                params: CreateDepositParams {
                    execution_fee: *execution_fee,
                    long_token_swap_length: long_token_swap_path
                        .len()
                        .try_into()
                        .map_err(|_| crate::Error::NumberOutOfRange)?,
                    short_token_swap_length: short_token_swap_path
                        .len()
                        .try_into()
                        .map_err(|_| crate::Error::NumberOutOfRange)?,
                    initial_long_token_amount: *initial_long_token_amount,
                    initial_short_token_amount: *initial_short_token_amount,
                    min_market_token: *min_market_token,
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
    client: &'a crate::Client<C>,
    store: Pubkey,
    deposit: Pubkey,
    reason: String,
    hint: Option<CloseDepositHint>,
}

/// Close Deposit Hint.
#[derive(Debug, Clone, Copy)]
pub struct CloseDepositHint {
    owner: Pubkey,
    market_token: Pubkey,
    market_token_account: Pubkey,
    initial_long_token: Option<Pubkey>,
    initial_short_token: Option<Pubkey>,
    initial_long_token_account: Option<Pubkey>,
    initial_short_token_account: Option<Pubkey>,
}

impl<'a> CloseDepositHint {
    /// Create from deposit.
    pub fn new(deposit: &'a DepositV2) -> Self {
        Self {
            owner: *deposit.header().owner(),
            market_token: deposit.tokens().market_token(),
            market_token_account: deposit.tokens().market_token_account(),
            initial_long_token: deposit.tokens().initial_long_token.token(),
            initial_short_token: deposit.tokens().initial_short_token.token(),
            initial_long_token_account: deposit.tokens().initial_long_token.account(),
            initial_short_token_account: deposit.tokens().initial_short_token.account(),
        }
    }
}

impl<'a, S, C> CloseDepositBuilder<'a, C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    pub(super) fn new(client: &'a crate::Client<C>, store: &Pubkey, deposit: &Pubkey) -> Self {
        Self {
            client,
            store: *store,
            deposit: *deposit,
            reason: "cancelled".to_string(),
            hint: None,
        }
    }

    /// Set hint with the given deposit.
    pub fn hint_with_deposit(&mut self, deposit: &DepositV2) -> &mut Self {
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
                let deposit: ZeroCopy<DepositV2> =
                    self.client.data_store().account(self.deposit).await?;
                Ok(CloseDepositHint::new(&deposit.0))
            }
        }
    }

    /// Build a [`RpcBuilder`] for `cancel_deposit` instruction.
    pub async fn build(&self) -> crate::Result<RpcBuilder<'a, C>> {
        let executor = self.client.payer();
        let hint = self.get_or_fetch_deposit_info().await?;
        let Self {
            client,
            store,
            deposit,
            ..
        } = self;
        let owner = hint.owner;
        let market_token_ata = get_associated_token_address(&owner, &hint.market_token);
        let initial_long_token_ata = hint
            .initial_long_token
            .as_ref()
            .map(|mint| get_associated_token_address(&owner, mint));
        let initial_short_token_ata = hint
            .initial_short_token
            .as_ref()
            .map(|mint| get_associated_token_address(&owner, mint));
        Ok(client
            .data_store_rpc()
            .accounts(crate::utils::fix_optional_account_metas(
                accounts::CloseDeposit {
                    executor,
                    store: *store,
                    owner,
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
                    event_authority: client.data_store_event_authority(),
                    program: client.store_program_id(),
                },
                &gmsol_store::id(),
                &client.store_program_id(),
            ))
            .args(instruction::CloseDeposit {
                reason: "cancelled".to_string(),
            }))
    }
}

/// Execute Deposit Builder.
pub struct ExecuteDepositBuilder<'a, C> {
    client: &'a crate::Client<C>,
    store: Pubkey,
    oracle: Pubkey,
    deposit: Pubkey,
    execution_fee: u64,
    price_provider: Pubkey,
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
    market_token_escrow: Pubkey,
    market_token_mint: Pubkey,
    /// Feeds.
    pub feeds: TokensWithFeed,
    swap: SwapParamsV2,
    initial_long_token_escrow: Option<Pubkey>,
    initial_short_token_escrow: Option<Pubkey>,
    initial_long_token: Option<Pubkey>,
    initial_short_token: Option<Pubkey>,
}

impl ExecuteDepositHint {
    /// Create a new hint for the deposit.
    pub fn new(deposit: &DepositV2, map: &impl TokenMapAccess) -> crate::Result<Self> {
        Ok(Self {
            owner: *deposit.header().owner(),
            market_token_escrow: deposit.tokens().market_token_account(),
            market_token_mint: deposit.tokens().market_token(),
            feeds: deposit.swap().to_feeds(map)?,
            swap: *deposit.swap(),
            initial_long_token: deposit.tokens().initial_long_token.token(),
            initial_short_token: deposit.tokens().initial_short_token.token(),
            initial_long_token_escrow: deposit.tokens().initial_long_token.account(),
            initial_short_token_escrow: deposit.tokens().initial_short_token.account(),
        })
    }
}

impl<'a, S, C> ExecuteDepositBuilder<'a, C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    pub(super) fn new(
        client: &'a crate::Client<C>,
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
            price_provider: Pyth::id(),
            hint: None,
            feeds_parser: Default::default(),
            token_map: None,
            cancel_on_execution_error,
            close: true,
        }
    }

    /// Set execution fee.
    pub fn execution_fee(&mut self, fee: u64) -> &mut Self {
        self.execution_fee = fee;
        self
    }

    /// Set whether to close the deposit after execution.
    pub fn close(&mut self, close: bool) -> &mut Self {
        self.close = close;
        self
    }

    /// Set price provider to the given.
    pub fn price_provider(&mut self, program: Pubkey) -> &mut Self {
        self.price_provider = program;
        self
    }

    /// Set hint with the given deposit.
    pub fn hint(
        &mut self,
        deposit: &DepositV2,
        map: &impl TokenMapAccess,
    ) -> crate::Result<&mut Self> {
        self.hint = Some(ExecuteDepositHint::new(deposit, map)?);
        Ok(self)
    }

    /// Parse feeds with the given price udpates map.
    #[cfg(feature = "pyth-pull-oracle")]
    pub fn parse_with_pyth_price_updates(&mut self, price_updates: Prices) -> &mut Self {
        self.feeds_parser.with_pyth_price_updates(price_updates);
        self
    }

    /// Prepare [`ExecuteDepositHint`].
    pub async fn prepare_hint(&mut self) -> crate::Result<ExecuteDepositHint> {
        match &self.hint {
            Some(hint) => Ok(hint.clone()),
            None => {
                let map = self.client.authorized_token_map(&self.store).await?;
                let deposit: ZeroCopy<DepositV2> =
                    self.client.data_store().account(self.deposit).await?;
                let hint = ExecuteDepositHint::new(&deposit.0, &map)?;
                self.hint = Some(hint.clone());
                Ok(hint)
            }
        }
    }

    async fn get_token_map(&self) -> crate::Result<Pubkey> {
        if let Some(address) = self.token_map {
            Ok(address)
        } else {
            crate::store::utils::token_map(self.client.data_store(), &self.store).await
        }
    }

    /// Set token map.
    pub fn token_map(&mut self, address: Pubkey) -> &mut Self {
        self.token_map = Some(address);
        self
    }

    /// Build [`RpcBuilder`] for executing the deposit.
    pub async fn build(&mut self) -> crate::Result<RpcBuilder<'a, C>> {
        let token_map = self.get_token_map().await?;
        let hint = self.prepare_hint().await?;
        let Self {
            client,
            store,
            oracle,
            deposit,
            execution_fee,
            price_provider,
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
        tracing::debug!(%price_provider, "constructing `execute_deposit` ix...");

        // Execution.
        let execute = client
            .data_store_rpc()
            .accounts(crate::utils::fix_optional_account_metas(
                accounts::ExecuteDepositV2 {
                    authority,
                    store: *store,
                    price_provider: *price_provider,
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
                },
                &gmsol_store::ID,
                &self.client.store_program_id(),
            ))
            .args(instruction::ExecuteDepositV2 {
                execution_fee: *execution_fee,
                throw_on_execution_error: !*cancel_on_execution_error,
            })
            .accounts(feeds.into_iter().chain(markets).collect::<Vec<_>>())
            .compute_budget(ComputeBudget::default().with_limit(EXECUTE_DEPOSIT_COMPUTE_BUDGET));

        if self.close {
            let close = self
                .client
                .close_deposit(store, deposit)
                .hint(CloseDepositHint {
                    owner: hint.owner,
                    market_token: hint.market_token_mint,
                    market_token_account: hint.market_token_escrow,
                    initial_long_token: hint.initial_long_token,
                    initial_short_token: hint.initial_short_token,
                    initial_long_token_account: hint.initial_long_token_escrow,
                    initial_short_token_account: hint.initial_short_token_escrow,
                })
                .build()
                .await?;
            Ok(execute.merge(close))
        } else {
            Ok(execute)
        }
    }
}

#[cfg(feature = "pyth-pull-oracle")]
mod pyth {
    use crate::pyth::{pull_oracle::ExecuteWithPythPrices, PythPullOracleContext};

    use super::*;

    impl<'a, C: Deref<Target = impl Signer> + Clone> ExecuteWithPythPrices<'a, C>
        for ExecuteDepositBuilder<'a, C>
    {
        fn set_execution_fee(&mut self, lamports: u64) {
            self.execution_fee(lamports);
        }

        async fn context(&mut self) -> crate::Result<PythPullOracleContext> {
            let hint = self.prepare_hint().await?;
            let ctx = PythPullOracleContext::try_from_feeds(&hint.feeds)?;
            Ok(ctx)
        }

        async fn build_rpc_with_price_updates(
            &mut self,
            price_updates: Prices,
        ) -> crate::Result<Vec<crate::utils::RpcBuilder<'a, C, ()>>> {
            let rpc = self
                .parse_with_pyth_price_updates(price_updates)
                .build()
                .await?;
            Ok(vec![rpc])
        }
    }
}
