use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
};

use anchor_client::{
    anchor_lang::{system_program, Id},
    solana_sdk::{
        address_lookup_table::AddressLookupTableAccount, instruction::AccountMeta, pubkey::Pubkey,
        signer::Signer,
    },
};
use anchor_spl::associated_token::get_associated_token_address;
use gmsol_store::{
    accounts, instruction,
    ops::order::CreateOrderParams,
    states::{
        common::{action::Action, swap::SwapParams, TokensWithFeed},
        order::{Order, OrderKind},
        position::PositionKind,
        user::UserHeader,
        Market, MarketMeta, NonceBytes, Pyth, Store, TokenMapAccess,
    },
};

use crate::{
    store::{
        token::TokenAccountOps,
        utils::{read_market, read_store, FeedsParser},
    },
    utils::{
        fix_optional_account_metas, ComputeBudget, RpcBuilder, TokenAccountParams,
        TransactionBuilder, ZeroCopy,
    },
};

use super::{generate_nonce, ExchangeOps};

#[cfg(feature = "pyth-pull-oracle")]
use crate::pyth::pull_oracle::{ExecuteWithPythPrices, Prices, PythPullOracleContext};

/// `execute_order` compute budget.
pub const EXECUTE_ORDER_COMPUTE_BUDGET: u32 = 400_000;

/// Order Params.
pub struct OrderParams {
    /// Order kind.
    pub kind: OrderKind,
    /// Minimum amount or value for output tokens.
    ///
    /// - Amount for swap orders.
    /// - Value for decrease position orders.
    pub min_output_amount: u128,
    /// Size delta usd.
    pub size_delta_usd: u128,
    /// Initial collateral delta amount.
    pub initial_collateral_delta_amount: u64,
    /// Trigger price (unit price).
    pub trigger_price: Option<u128>,
    /// Acceptable price (unit price).
    pub acceptable_price: Option<u128>,
    /// Whether the order is for a long or short position.
    pub is_long: bool,
}

impl OrderParams {
    /// Get position kind.
    pub fn to_position_kind(&self) -> crate::Result<PositionKind> {
        if self.kind.is_swap() {
            Err(crate::Error::invalid_argument("position is not required"))
        } else {
            Ok(if self.is_long {
                PositionKind::Long
            } else {
                PositionKind::Short
            })
        }
    }
}

/// Create Order Builder.
pub struct CreateOrderBuilder<'a, C> {
    client: &'a crate::Client<C>,
    store: Pubkey,
    market_token: Pubkey,
    is_output_token_long: bool,
    nonce: Option<NonceBytes>,
    execution_fee: u64,
    params: OrderParams,
    swap_path: Vec<Pubkey>,
    hint: Option<CreateOrderHint>,
    initial_token: TokenAccountParams,
    final_token: Option<Pubkey>,
    long_token_account: Option<Pubkey>,
    short_token_account: Option<Pubkey>,
}

#[derive(Clone, Copy)]
struct CreateOrderHint {
    long_token: Pubkey,
    short_token: Pubkey,
}

impl<'a, C, S> CreateOrderBuilder<'a, C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    pub(super) fn new(
        client: &'a crate::Client<C>,
        store: &Pubkey,
        market_token: &Pubkey,
        params: OrderParams,
        is_output_token_long: bool,
    ) -> Self {
        Self {
            client,
            store: *store,
            market_token: *market_token,
            nonce: None,
            execution_fee: Order::MIN_EXECUTION_LAMPORTS,
            params,
            swap_path: vec![],
            is_output_token_long,
            hint: None,
            initial_token: Default::default(),
            final_token: Default::default(),
            long_token_account: None,
            short_token_account: None,
        }
    }

    /// Set extra exectuion fee allowed to use.
    ///
    /// Defaults to `0` means only allowed to use at most `rent-exempt` amount of fee.
    pub fn execution_fee(&mut self, amount: u64) -> &mut Self {
        self.execution_fee = amount;
        self
    }

    /// Setup hint with the given market meta.
    pub fn hint(&mut self, meta: &MarketMeta) -> &mut Self {
        self.hint = Some(CreateOrderHint {
            long_token: meta.long_token_mint,
            short_token: meta.short_token_mint,
        });
        self
    }

    /// Set swap path.
    pub fn swap_path(&mut self, market_tokens: Vec<Pubkey>) -> &mut Self {
        self.swap_path = market_tokens;
        self
    }

    /// Set initial collateral token (or swap-in token) params.
    pub fn initial_collateral_token(
        &mut self,
        token: &Pubkey,
        token_account: Option<&Pubkey>,
    ) -> &mut Self {
        self.initial_token.set_token(*token);
        if let Some(account) = token_account {
            self.initial_token.set_token_account(*account);
        }
        self
    }

    /// Set final output token params (position order only).
    pub fn final_output_token(&mut self, token: &Pubkey) -> &mut Self {
        self.final_token = Some(*token);
        self
    }

    /// Set long token account.
    pub fn long_token_account(&mut self, account: &Pubkey) -> &mut Self {
        self.long_token_account = Some(*account);
        self
    }

    /// Set short token account.
    pub fn short_token_account(&mut self, account: &Pubkey) -> &mut Self {
        self.short_token_account = Some(*account);
        self
    }

    /// Set min output amount.
    pub fn min_output_amount(&mut self, amount: u128) -> &mut Self {
        self.params.min_output_amount = amount;
        self
    }

    /// Set acceptable price.
    pub fn acceptable_price(&mut self, unit_price: u128) -> &mut Self {
        self.params.acceptable_price = Some(unit_price);
        self
    }

    fn market(&self) -> Pubkey {
        self.client
            .find_market_address(&self.store, &self.market_token)
    }

    async fn prepare_hint(&mut self) -> crate::Result<CreateOrderHint> {
        loop {
            if let Some(hint) = self.hint {
                return Ok(hint);
            }
            let market =
                read_market(&self.client.data_store().solana_rpc(), &self.market()).await?;
            self.hint(market.meta());
        }
    }

    async fn output_token(&mut self) -> crate::Result<Pubkey> {
        let hint = self.prepare_hint().await?;
        let output_token = if self.is_output_token_long {
            hint.long_token
        } else {
            hint.short_token
        };
        Ok(output_token)
    }

    async fn position(&mut self) -> crate::Result<Option<Pubkey>> {
        let output_token = self.output_token().await?;
        match &self.params.kind {
            OrderKind::MarketIncrease
            | OrderKind::MarketDecrease
            | OrderKind::Liquidation
            | OrderKind::LimitIncrease
            | OrderKind::LimitDecrease
            | OrderKind::StopLossDecrease => {
                let position = self.client.find_position_address(
                    &self.store,
                    &self.client.payer(),
                    &self.market_token,
                    &output_token,
                    self.params.to_position_kind()?,
                )?;
                Ok(Some(position))
            }
            OrderKind::MarketSwap | OrderKind::LimitSwap => Ok(None),
            kind => Err(crate::Error::invalid_argument(format!(
                "unsupported order kind: {kind:?}"
            ))),
        }
    }

    /// Get initial collateral token account and vault.
    ///
    /// Returns `(initial_collateral_token, initial_collateral_token_account)`.
    async fn initial_collateral_accounts(&mut self) -> crate::Result<Option<(Pubkey, Pubkey)>> {
        match &self.params.kind {
            OrderKind::MarketIncrease
            | OrderKind::MarketSwap
            | OrderKind::LimitIncrease
            | OrderKind::LimitSwap => {
                if self.initial_token.is_empty() {
                    let output_token = self.output_token().await?;
                    self.initial_token.set_token(output_token);
                }
                let Some((token, account)) = self
                    .initial_token
                    .get_or_fetch_token_and_token_account(self.client, Some(&self.client.payer()))
                    .await?
                else {
                    return Err(crate::Error::invalid_argument(
                        "missing initial collateral token parameters",
                    ));
                };
                Ok(Some((token, account)))
            }
            OrderKind::MarketDecrease
            | OrderKind::Liquidation
            | OrderKind::LimitDecrease
            | OrderKind::StopLossDecrease => Ok(None),
            kind => Err(crate::Error::invalid_argument(format!(
                "unsupported order kind: {kind:?}"
            ))),
        }
    }

    async fn get_final_output_token(&mut self) -> crate::Result<Pubkey> {
        match &self.params.kind {
            OrderKind::MarketDecrease | OrderKind::LimitDecrease | OrderKind::StopLossDecrease => {
                if self.final_token.is_none() {
                    let output_token = self.output_token().await?;
                    self.final_token = Some(output_token);
                }
                Ok(self.final_token.unwrap())
            }
            OrderKind::MarketIncrease
            | OrderKind::MarketSwap
            | OrderKind::LimitIncrease
            | OrderKind::LimitSwap => Ok(self.output_token().await?),
            kind => Err(crate::Error::invalid_argument(format!(
                "unsupported order kind: {kind:?}"
            ))),
        }
    }

    /// Create [`RpcBuilder`] and return order address.
    pub async fn build_with_address(&mut self) -> crate::Result<(RpcBuilder<'a, C>, Pubkey)> {
        let (rpc, order, _) = self.build_with_addresses().await?;
        Ok((rpc, order))
    }

    /// Create [`RpcBuilder`] and return order address and optional position address.
    pub async fn build_with_addresses(
        &mut self,
    ) -> crate::Result<(RpcBuilder<'a, C>, Pubkey, Option<Pubkey>)> {
        let token_program_id = anchor_spl::token::ID;

        let nonce = self.nonce.unwrap_or_else(generate_nonce);
        let payer = &self.client.payer();
        let order = self.client.find_order_address(&self.store, payer, &nonce);
        let (initial_collateral_token, initial_collateral_token_account) =
            self.initial_collateral_accounts().await?.unzip();
        let final_output_token = self.get_final_output_token().await?;
        let hint = self.prepare_hint().await?;
        let (long_token, short_token) = if self.params.kind.is_swap() {
            (None, None)
        } else {
            (Some(hint.long_token), Some(hint.short_token))
        };

        let initial_collateral_token_escrow = initial_collateral_token
            .as_ref()
            .map(|token| get_associated_token_address(&order, token));
        let long_token_accounts = long_token.as_ref().map(|token| {
            let escrow = get_associated_token_address(&order, token);
            let ata = get_associated_token_address(payer, token);
            (escrow, ata)
        });
        let short_token_accounts = short_token.as_ref().map(|token| {
            let escrow = get_associated_token_address(&order, token);
            let ata = get_associated_token_address(payer, token);
            (escrow, ata)
        });
        let final_output_token_accounts =
            if self.params.kind.is_swap() || self.params.kind.is_decrease_position() {
                let escrow = get_associated_token_address(&order, &final_output_token);
                let ata = get_associated_token_address(payer, &final_output_token);
                Some((escrow, ata))
            } else {
                None
            };
        let position = self.position().await?;
        let user = self.client.find_user_address(&self.store, payer);

        let kind = self.params.kind;
        let params = CreateOrderParams {
            execution_lamports: self.execution_fee,
            swap_path_length: self
                .swap_path
                .len()
                .try_into()
                .map_err(|_| crate::Error::NumberOutOfRange)?,
            kind,
            initial_collateral_delta_amount: self.params.initial_collateral_delta_amount,
            size_delta_value: self.params.size_delta_usd,
            is_long: self.params.is_long,
            is_collateral_long: self.is_output_token_long,
            min_output: Some(self.params.min_output_amount),
            trigger_price: self.params.trigger_price,
            acceptable_price: self.params.acceptable_price,
        };

        let prepare = match kind {
            OrderKind::MarketSwap | OrderKind::LimitSwap => {
                let swap_in_token = initial_collateral_token.ok_or(
                    crate::Error::invalid_argument("swap in token is not provided"),
                )?;
                let escrow = self
                    .client
                    .prepare_associated_token_account(
                        &swap_in_token,
                        &token_program_id,
                        Some(&order),
                    )
                    .merge(self.client.prepare_associated_token_account(
                        &final_output_token,
                        &token_program_id,
                        Some(&order),
                    ));
                let ata = self.client.prepare_associated_token_account(
                    &final_output_token,
                    &token_program_id,
                    None,
                );
                escrow.merge(ata)
            }
            OrderKind::MarketIncrease | OrderKind::LimitIncrease => {
                let initial_collateral_token = initial_collateral_token.ok_or(
                    crate::Error::invalid_argument("initial collateral token is not provided"),
                )?;
                let long_token = long_token
                    .ok_or(crate::Error::invalid_argument("long token is not provided"))?;
                let short_token = short_token.ok_or(crate::Error::invalid_argument(
                    "short token is not provided",
                ))?;

                let escrow = self
                    .client
                    .prepare_associated_token_account(
                        &initial_collateral_token,
                        &token_program_id,
                        Some(&order),
                    )
                    .merge(self.client.prepare_associated_token_account(
                        &long_token,
                        &token_program_id,
                        Some(&order),
                    ))
                    .merge(self.client.prepare_associated_token_account(
                        &short_token,
                        &token_program_id,
                        Some(&order),
                    ));
                let long_token_ata = self.client.prepare_associated_token_account(
                    &long_token,
                    &token_program_id,
                    None,
                );
                let short_token_ata = self.client.prepare_associated_token_account(
                    &short_token,
                    &token_program_id,
                    None,
                );

                let prepare_position = self
                    .client
                    .store_rpc()
                    .accounts(accounts::PreparePosition {
                        owner: *payer,
                        store: self.store,
                        market: self.market(),
                        position: position.expect("must provided"),
                        system_program: system_program::ID,
                    })
                    .args(instruction::PreparePosition {
                        params: params.clone(),
                    });

                escrow
                    .merge(long_token_ata)
                    .merge(short_token_ata)
                    .merge(prepare_position)
            }
            OrderKind::MarketDecrease | OrderKind::LimitDecrease | OrderKind::StopLossDecrease => {
                let long_token = long_token
                    .ok_or(crate::Error::invalid_argument("long token is not provided"))?;
                let short_token = short_token.ok_or(crate::Error::invalid_argument(
                    "short token is not provided",
                ))?;

                let escrow = self
                    .client
                    .prepare_associated_token_account(
                        &final_output_token,
                        &token_program_id,
                        Some(&order),
                    )
                    .merge(self.client.prepare_associated_token_account(
                        &long_token,
                        &token_program_id,
                        Some(&order),
                    ))
                    .merge(self.client.prepare_associated_token_account(
                        &short_token,
                        &token_program_id,
                        Some(&order),
                    ));

                let long_token_ata = self.client.prepare_associated_token_account(
                    &long_token,
                    &token_program_id,
                    None,
                );
                let short_token_ata = self.client.prepare_associated_token_account(
                    &short_token,
                    &token_program_id,
                    None,
                );
                let final_output_token_ata = self.client.prepare_associated_token_account(
                    &final_output_token,
                    &token_program_id,
                    None,
                );

                escrow
                    .merge(long_token_ata)
                    .merge(short_token_ata)
                    .merge(final_output_token_ata)
            }
            _ => {
                return Err(crate::Error::invalid_argument("unsupported order kind"));
            }
        };

        let prepare_user = self
            .client
            .store_rpc()
            .accounts(accounts::PrepareUser {
                owner: *payer,
                store: self.store,
                user,
                system_program: system_program::ID,
            })
            .args(instruction::PrepareUser {});

        let create = self
            .client
            .store_rpc()
            .accounts(crate::utils::fix_optional_account_metas(
                accounts::CreateOrder {
                    store: self.store,
                    order,
                    position,
                    market: self.market(),
                    owner: *payer,
                    user,
                    initial_collateral_token,
                    final_output_token,
                    long_token,
                    short_token,
                    initial_collateral_token_escrow,
                    final_output_token_escrow: final_output_token_accounts
                        .map(|(escrow, _)| escrow),
                    long_token_escrow: long_token_accounts.map(|(escrow, _)| escrow),
                    short_token_escrow: short_token_accounts.map(|(escrow, _)| escrow),
                    initial_collateral_token_source: initial_collateral_token_account,
                    final_output_token_ata: final_output_token_accounts.map(|(_, ata)| ata),
                    long_token_ata: long_token_accounts.map(|(_, ata)| ata),
                    short_token_ata: short_token_accounts.map(|(_, ata)| ata),
                    system_program: system_program::ID,
                    token_program: anchor_spl::token::ID,
                    associated_token_program: anchor_spl::associated_token::ID,
                },
                &gmsol_store::id(),
                self.client.store_program_id(),
            ))
            .args(instruction::CreateOrder { nonce, params })
            .accounts(
                self.swap_path
                    .iter()
                    .map(|mint| AccountMeta {
                        pubkey: self.client.find_market_address(&self.store, mint),
                        is_signer: false,
                        is_writable: false,
                    })
                    .collect::<Vec<_>>(),
            );

        Ok((prepare.merge(prepare_user).merge(create), order, position))
    }
}

/// Execute Order Builder.
pub struct ExecuteOrderBuilder<'a, C> {
    client: &'a crate::Client<C>,
    store: Pubkey,
    oracle: Pubkey,
    order: Pubkey,
    execution_fee: u64,
    price_provider: Pubkey,
    feeds_parser: FeedsParser,
    recent_timestamp: i64,
    hint: Option<ExecuteOrderHint>,
    token_map: Option<Pubkey>,
    cancel_on_execution_error: bool,
    close: bool,
    event_buffer_index: u8,
    alts: HashMap<Pubkey, Vec<Pubkey>>,
}

/// Hint for executing order.
#[derive(Clone)]
pub struct ExecuteOrderHint {
    kind: OrderKind,
    store_program_id: Pubkey,
    store: Store,
    market_token: Pubkey,
    position: Option<Pubkey>,
    owner: Pubkey,
    user: Pubkey,
    referrer: Option<Pubkey>,
    initial_collateral_token_and_account: Option<(Pubkey, Pubkey)>,
    final_output_token_and_account: Option<(Pubkey, Pubkey)>,
    long_token_and_account: Option<(Pubkey, Pubkey)>,
    short_token_and_account: Option<(Pubkey, Pubkey)>,
    long_token_mint: Pubkey,
    short_token_mint: Pubkey,
    pnl_token: Pubkey,
    /// Feeds.
    pub feeds: TokensWithFeed,
    swap: SwapParams,
}

impl ExecuteOrderHint {
    fn long_token_vault(&self, store: &Pubkey) -> Option<Pubkey> {
        let token = self.long_token_and_account.as_ref()?.0;
        Some(crate::pda::find_market_vault_address(store, &token, &self.store_program_id).0)
    }

    fn short_token_vault(&self, store: &Pubkey) -> Option<Pubkey> {
        let token = self.short_token_and_account.as_ref()?.0;
        Some(crate::pda::find_market_vault_address(store, &token, &self.store_program_id).0)
    }

    fn claimable_long_token_account(
        &self,
        store: &Pubkey,
        timestamp: i64,
    ) -> crate::Result<Pubkey> {
        Ok(crate::pda::find_claimable_account_pda(
            store,
            &self.long_token_mint,
            &self.owner,
            &self.store.claimable_time_key(timestamp)?,
            &self.store_program_id,
        )
        .0)
    }

    fn claimable_short_token_account(
        &self,
        store: &Pubkey,
        timestamp: i64,
    ) -> crate::Result<Pubkey> {
        Ok(crate::pda::find_claimable_account_pda(
            store,
            &self.short_token_mint,
            &self.owner,
            &self.store.claimable_time_key(timestamp)?,
            &self.store_program_id,
        )
        .0)
    }

    fn claimable_pnl_token_account_for_holding(
        &self,
        store: &Pubkey,
        timestamp: i64,
    ) -> crate::Result<Pubkey> {
        Ok(crate::pda::find_claimable_account_pda(
            store,
            &self.pnl_token,
            self.store.holding(),
            &self.store.claimable_time_key(timestamp)?,
            &self.store_program_id,
        )
        .0)
    }
}

impl<'a, S, C> ExecuteOrderBuilder<'a, C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    pub(super) fn try_new(
        client: &'a crate::Client<C>,
        store: &Pubkey,
        oracle: &Pubkey,
        order: &Pubkey,
        cancel_on_execution_error: bool,
    ) -> crate::Result<Self> {
        Ok(Self {
            client,
            store: *store,
            oracle: *oracle,
            order: *order,
            execution_fee: 0,
            price_provider: Pyth::id(),
            feeds_parser: Default::default(),
            recent_timestamp: recent_timestamp()?,
            hint: None,
            token_map: None,
            cancel_on_execution_error,
            close: true,
            event_buffer_index: 0,
            alts: Default::default(),
        })
    }

    /// Set execution fee.
    pub fn execution_fee(&mut self, fee: u64) -> &mut Self {
        self.execution_fee = fee;
        self
    }

    /// Set price provider to the given.
    pub fn price_provider(&mut self, program: Pubkey) -> &mut Self {
        self.price_provider = program;
        self
    }

    /// Set whether to close order after execution.
    pub fn close(&mut self, close: bool) -> &mut Self {
        self.close = close;
        self
    }

    /// Set event buffer index.
    pub fn event_buffer_index(&mut self, index: u8) -> &mut Self {
        self.event_buffer_index = index;
        self
    }

    /// Insert an Address Lookup Table.
    pub fn add_alt(&mut self, account: AddressLookupTableAccount) -> &mut Self {
        self.alts.insert(account.key, account.addresses);
        self
    }

    /// Set hint with the given order.
    pub fn hint(
        &mut self,
        order: &Order,
        market: &Market,
        store: &Store,
        map: &impl TokenMapAccess,
        user: Option<&UserHeader>,
    ) -> crate::Result<&mut Self> {
        let params = order.params();
        let swap = order.swap();
        let market_token = *order.market_token();
        let kind = params.kind()?;
        let tokens = order.tokens();
        let owner = *order.header().owner();
        let user_address = self.client.find_user_address(&self.store, &owner);
        let referrer = user.and_then(|user| user.referral().referrer().copied());
        self.hint = Some(ExecuteOrderHint {
            kind,
            store_program_id: *self.client.store_program_id(),
            store: *store,
            market_token,
            position: params.position().copied(),
            owner,
            user: user_address,
            referrer,
            long_token_mint: market.meta().long_token_mint,
            short_token_mint: market.meta().short_token_mint,
            pnl_token: if params.side()?.is_long() {
                market.meta().long_token_mint
            } else {
                market.meta().short_token_mint
            },
            feeds: swap.to_feeds(map)?,
            swap: *swap,
            initial_collateral_token_and_account: tokens.initial_collateral().token_and_account(),
            final_output_token_and_account: tokens.final_output_token().token_and_account(),
            long_token_and_account: tokens.long_token().token_and_account(),
            short_token_and_account: tokens.short_token().token_and_account(),
        });
        Ok(self)
    }

    /// Parse feeds with the given price udpates map.
    #[cfg(feature = "pyth-pull-oracle")]
    pub fn parse_with_pyth_price_updates(&mut self, price_updates: Prices) -> &mut Self {
        self.feeds_parser.with_pyth_price_updates(price_updates);
        self
    }

    /// Prepare [`ExecuteOrderHint`].
    pub async fn prepare_hint(&mut self) -> crate::Result<ExecuteOrderHint> {
        loop {
            match &self.hint {
                Some(hint) => return Ok(hint.clone()),
                None => {
                    let order = self.client.order(&self.order).await?;
                    let market = read_market(
                        &self.client.data_store().solana_rpc(),
                        order.header().market(),
                    )
                    .await?;
                    let store =
                        read_store(&self.client.data_store().solana_rpc(), &self.store).await?;
                    let token_map_address = self.get_token_map().await?;
                    let token_map = self.client.token_map(&token_map_address).await?;
                    let owner = order.header().owner();
                    let user = self.client.find_user_address(&self.store, owner);
                    let user = self
                        .client
                        .account::<ZeroCopy<UserHeader>>(&user)
                        .await?
                        .map(|user| user.0);
                    self.hint(&order, &market, &store, &token_map, user.as_ref())?;
                }
            }
        }
    }

    /// Set recent timestamp with the given.
    ///
    /// Default to current unix timestamp.
    pub fn recent_timestamp(&mut self, timestamp: i64) -> &mut Self {
        self.recent_timestamp = timestamp;
        self
    }

    /// Get claimable accounts.
    ///
    /// The returned values are of the form `[long_for_user, short_for_user, pnl_for_holding]`.
    pub async fn claimable_accounts(&mut self) -> crate::Result<[Pubkey; 3]> {
        let hint = self.prepare_hint().await?;
        let long_for_user =
            hint.claimable_long_token_account(&self.store, self.recent_timestamp)?;
        let short_for_user =
            hint.claimable_short_token_account(&self.store, self.recent_timestamp)?;
        let pnl_for_holding =
            hint.claimable_pnl_token_account_for_holding(&self.store, self.recent_timestamp)?;
        Ok([long_for_user, short_for_user, pnl_for_holding])
    }

    async fn get_token_map(&mut self) -> crate::Result<Pubkey> {
        if let Some(address) = self.token_map {
            Ok(address)
        } else {
            let address = self
                .client
                .authorized_token_map_address(&self.store)
                .await?
                .ok_or(crate::Error::invalid_argument(
                    "token map is not set for this store",
                ))?;
            self.token_map = Some(address);
            Ok(address)
        }
    }

    /// Set token map.
    pub fn token_map(&mut self, address: Pubkey) -> &mut Self {
        self.token_map = Some(address);
        self
    }

    /// Build [`TransactionBuilder`] for `execute_order` instructions.
    pub async fn build(&mut self) -> crate::Result<TransactionBuilder<'a, C>> {
        let hint = self.prepare_hint().await?;
        let [claimable_long_token_account_for_user, claimable_short_token_account_for_user, claimable_pnl_token_account_for_holding] =
            self.claimable_accounts().await?;

        let authority = self.client.payer();
        let feeds = self
            .feeds_parser
            .parse(&hint.feeds)
            .collect::<Result<Vec<_>, _>>()?;
        let token_map = self.get_token_map().await?;
        let swap_markets = hint
            .swap
            .unique_market_tokens_excluding_current(&hint.market_token)
            .map(|mint| AccountMeta {
                pubkey: self.client.find_market_address(&self.store, mint),
                is_signer: false,
                is_writable: true,
            });
        let event = self.client.find_trade_event_buffer_address(
            &self.store,
            &authority,
            self.event_buffer_index,
        );

        let kind = hint.kind;
        let mut require_claimable_accounts = false;

        let mut execute_order = match kind {
            OrderKind::MarketDecrease | OrderKind::LimitDecrease | OrderKind::StopLossDecrease => {
                require_claimable_accounts = true;

                self.client
                    .store_rpc()
                    .accounts(fix_optional_account_metas(
                        accounts::ExecuteDecreaseOrder {
                            authority,
                            owner: hint.owner,
                            user: hint.user,
                            store: self.store,
                            oracle: self.oracle,
                            token_map,
                            market: self
                                .client
                                .find_market_address(&self.store, &hint.market_token),
                            order: self.order,
                            position: hint
                                .position
                                .ok_or(crate::Error::invalid_argument("missing position"))?,
                            event,
                            final_output_token_vault: hint
                                .final_output_token_and_account
                                .as_ref()
                                .map(|(token, _)| {
                                    self.client.find_market_vault_address(&self.store, token)
                                })
                                .ok_or(crate::Error::invalid_argument(
                                    "missing final output token",
                                ))?,
                            long_token_vault: hint
                                .long_token_vault(&self.store)
                                .ok_or(crate::Error::invalid_argument("missing long token"))?,
                            short_token_vault: hint
                                .short_token_vault(&self.store)
                                .ok_or(crate::Error::invalid_argument("missing short token"))?,
                            claimable_long_token_account_for_user,
                            claimable_short_token_account_for_user,
                            claimable_pnl_token_account_for_holding,
                            event_authority: self.client.store_event_authority(),
                            token_program: anchor_spl::token::ID,
                            system_program: system_program::ID,
                            long_token: hint
                                .long_token_and_account
                                .map(|(token, _)| token)
                                .ok_or(crate::Error::invalid_argument("missing long token"))?,
                            short_token: hint
                                .short_token_and_account
                                .map(|(token, _)| token)
                                .ok_or(crate::Error::invalid_argument("missing short token"))?,
                            final_output_token: hint
                                .final_output_token_and_account
                                .map(|(token, _)| token)
                                .ok_or(crate::Error::invalid_argument(
                                    "missing final output token",
                                ))?,
                            final_output_token_escrow: hint
                                .final_output_token_and_account
                                .map(|(_, account)| account)
                                .ok_or(crate::Error::invalid_argument(
                                    "missing final output token",
                                ))?,
                            long_token_escrow: hint
                                .long_token_and_account
                                .map(|(_, account)| account)
                                .ok_or(crate::Error::invalid_argument("missing long token"))?,
                            short_token_escrow: hint
                                .short_token_and_account
                                .map(|(_, account)| account)
                                .ok_or(crate::Error::invalid_argument("missing short token"))?,
                            program: *self.client.store_program_id(),
                            chainlink_program: None,
                        },
                        &crate::program_ids::DEFAULT_GMSOL_STORE_ID,
                        self.client.store_program_id(),
                    ))
                    .args(instruction::ExecuteDecreaseOrder {
                        recent_timestamp: self.recent_timestamp,
                        execution_fee: self.execution_fee,
                        throw_on_execution_error: !self.cancel_on_execution_error,
                    })
            }
            _ => self
                .client
                .store_rpc()
                .accounts(crate::utils::fix_optional_account_metas(
                    accounts::ExecuteIncreaseOrSwapOrder {
                        authority,
                        owner: hint.owner,
                        user: hint.user,
                        store: self.store,
                        oracle: self.oracle,
                        token_map,
                        market: self
                            .client
                            .find_market_address(&self.store, &hint.market_token),
                        order: self.order,
                        position: hint.position,
                        event: (!kind.is_swap()).then_some(event),
                        final_output_token_vault: hint.final_output_token_and_account.as_ref().map(
                            |(token, _)| self.client.find_market_vault_address(&self.store, token),
                        ),
                        long_token_vault: hint.long_token_vault(&self.store),
                        short_token_vault: hint.short_token_vault(&self.store),
                        event_authority: self.client.store_event_authority(),
                        token_program: anchor_spl::token::ID,
                        system_program: system_program::ID,
                        initial_collateral_token: hint
                            .initial_collateral_token_and_account
                            .map(|(token, _)| token),
                        initial_collateral_token_vault: hint
                            .initial_collateral_token_and_account
                            .map(|(token, _)| {
                                self.client.find_market_vault_address(&self.store, &token)
                            }),
                        initial_collateral_token_escrow: hint
                            .initial_collateral_token_and_account
                            .map(|(_, account)| account),
                        long_token: hint.long_token_and_account.map(|(token, _)| token),
                        short_token: hint.short_token_and_account.map(|(token, _)| token),
                        final_output_token: hint
                            .final_output_token_and_account
                            .map(|(token, _)| token),
                        final_output_token_escrow: hint
                            .final_output_token_and_account
                            .map(|(_, account)| account),
                        long_token_escrow: hint.long_token_and_account.map(|(_, account)| account),
                        short_token_escrow: hint
                            .short_token_and_account
                            .map(|(_, account)| account),
                        program: *self.client.store_program_id(),
                        chainlink_program: None,
                    },
                    &crate::program_ids::DEFAULT_GMSOL_STORE_ID,
                    self.client.store_program_id(),
                ))
                .args(instruction::ExecuteIncreaseOrSwapOrder {
                    recent_timestamp: self.recent_timestamp,
                    execution_fee: self.execution_fee,
                    throw_on_execution_error: !self.cancel_on_execution_error,
                }),
        };

        execute_order = execute_order
            .accounts(feeds.into_iter().chain(swap_markets).collect::<Vec<_>>())
            .compute_budget(ComputeBudget::default().with_limit(EXECUTE_ORDER_COMPUTE_BUDGET))
            .lookup_tables(self.alts.clone());

        if !kind.is_swap() {
            let prepare_event_buffer = self
                .client
                .store_rpc()
                .accounts(accounts::PrepareTradeEventBuffer {
                    authority,
                    store: self.store,
                    event,
                    system_program: system_program::ID,
                })
                .args(instruction::PrepareTradeEventBuffer {
                    index: self.event_buffer_index,
                });
            execute_order = prepare_event_buffer.merge(execute_order);
        }

        if self.close {
            let close = self
                .client
                .close_order(&self.order)?
                .reason("executed")
                .hint(CloseOrderHint {
                    owner: hint.owner,
                    store: self.store,
                    initial_collateral_token_and_account: hint.initial_collateral_token_and_account,
                    final_output_token_and_account: hint.final_output_token_and_account,
                    long_token_and_account: hint.long_token_and_account,
                    short_token_and_account: hint.short_token_and_account,
                    user: hint.user,
                    referrer: hint.referrer,
                })
                .build()
                .await?;
            execute_order = execute_order.merge(close);
        }

        let mut builder = ClaimableAccountsBuilder::new(
            self.recent_timestamp,
            self.store,
            hint.owner,
            *hint.store.holding(),
        );

        if require_claimable_accounts {
            builder.claimable_long_token_account_for_user(
                &hint.long_token_mint,
                &claimable_long_token_account_for_user,
            );
            builder.claimable_short_token_account_for_user(
                &hint.short_token_mint,
                &claimable_short_token_account_for_user,
            );
            builder.claimable_pnl_token_account_for_holding(
                &hint.pnl_token,
                &claimable_pnl_token_account_for_holding,
            );
        }

        let (pre_builder, post_builder) = builder.build(self.client);

        let mut transaction_builder =
            TransactionBuilder::new(self.client.data_store().solana_rpc());
        transaction_builder
            .try_push(pre_builder)?
            .try_push(execute_order)?
            .try_push(post_builder)?;
        Ok(transaction_builder)
    }
}

/// Close Order Builder.
pub struct CloseOrderBuilder<'a, C> {
    client: &'a crate::Client<C>,
    order: Pubkey,
    hint: Option<CloseOrderHint>,
    reason: String,
}

/// Close Order Hint.
#[derive(Clone, Copy)]
pub struct CloseOrderHint {
    pub(super) owner: Pubkey,
    pub(super) store: Pubkey,
    pub(super) initial_collateral_token_and_account: Option<(Pubkey, Pubkey)>,
    pub(super) final_output_token_and_account: Option<(Pubkey, Pubkey)>,
    pub(super) long_token_and_account: Option<(Pubkey, Pubkey)>,
    pub(super) short_token_and_account: Option<(Pubkey, Pubkey)>,
    pub(super) user: Pubkey,
    pub(super) referrer: Option<Pubkey>,
}

impl CloseOrderHint {
    /// Create hint from order and user account.
    pub fn new(
        order: &Order,
        user: Option<&UserHeader>,
        program_id: &Pubkey,
    ) -> crate::Result<Self> {
        let tokens = order.tokens();
        let owner = order.header().owner();
        let store = order.header().store();
        let user_address = crate::pda::find_user_pda(store, owner, program_id).0;
        let referrer = user.and_then(|user| user.referral().referrer().copied());
        Ok(Self {
            owner: *owner,
            store: *store,
            user: user_address,
            referrer,
            initial_collateral_token_and_account: tokens.initial_collateral().token_and_account(),
            final_output_token_and_account: tokens.final_output_token().token_and_account(),
            long_token_and_account: tokens.long_token().token_and_account(),
            short_token_and_account: tokens.short_token().token_and_account(),
        })
    }
}

impl<'a, S, C> CloseOrderBuilder<'a, C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    pub(super) fn new(client: &'a crate::Client<C>, order: &Pubkey) -> Self {
        Self {
            client,
            order: *order,
            hint: None,
            reason: "cancelled".into(),
        }
    }

    /// Set hint with the given order.
    pub fn hint_with_order(
        &mut self,
        order: &Order,
        user: Option<&UserHeader>,
        program_id: &Pubkey,
    ) -> crate::Result<&mut Self> {
        Ok(self.hint(CloseOrderHint::new(order, user, program_id)?))
    }

    /// Set hint.
    pub fn hint(&mut self, hint: CloseOrderHint) -> &mut Self {
        self.hint = Some(hint);
        self
    }

    /// Set reason.
    pub fn reason(&mut self, reason: impl ToString) -> &mut Self {
        self.reason = reason.to_string();
        self
    }

    async fn prepare_hint(&mut self) -> crate::Result<CloseOrderHint> {
        match &self.hint {
            Some(hint) => Ok(*hint),
            None => {
                let order: ZeroCopy<Order> = self
                    .client
                    .account_with_config(&self.order, Default::default())
                    .await?
                    .into_value()
                    .ok_or(crate::Error::invalid_argument("order not found"))?;
                let user = self
                    .client
                    .find_user_address(order.0.header().store(), order.0.header().owner());
                let user = self.client.account(&user).await?;
                let hint =
                    CloseOrderHint::new(&order.0, user.as_ref(), self.client.store_program_id())?;
                self.hint = Some(hint);
                Ok(hint)
            }
        }
    }

    /// Build [`RpcBuilder`] for cancelling the order.
    pub async fn build(&mut self) -> crate::Result<RpcBuilder<'a, C>> {
        let hint = self.prepare_hint().await?;
        let payer = self.client.payer();
        let owner = hint.owner;
        let referrer_user = hint
            .referrer
            .map(|owner| self.client.find_user_address(&hint.store, &owner));
        Ok(self
            .client
            .store_rpc()
            .accounts(crate::utils::fix_optional_account_metas(
                accounts::CloseOrder {
                    store: hint.store,
                    event_authority: self.client.store_event_authority(),
                    order: self.order,
                    executor: payer,
                    owner,
                    user: hint.user,
                    referrer_user,
                    initial_collateral_token: hint
                        .initial_collateral_token_and_account
                        .map(|(token, _)| token),
                    initial_collateral_token_escrow: hint
                        .initial_collateral_token_and_account
                        .map(|(_, account)| account),
                    long_token: hint.long_token_and_account.map(|(token, _)| token),
                    short_token: hint.short_token_and_account.map(|(token, _)| token),
                    final_output_token: hint.final_output_token_and_account.map(|(token, _)| token),
                    final_output_token_escrow: hint
                        .final_output_token_and_account
                        .map(|(_, account)| account),
                    long_token_escrow: hint.long_token_and_account.map(|(_, account)| account),
                    short_token_escrow: hint.short_token_and_account.map(|(_, account)| account),
                    initial_collateral_token_ata: hint
                        .initial_collateral_token_and_account
                        .as_ref()
                        .map(|(token, _)| get_associated_token_address(&owner, token)),
                    final_output_token_ata: hint
                        .final_output_token_and_account
                        .as_ref()
                        .map(|(token, _)| get_associated_token_address(&owner, token)),
                    long_token_ata: hint
                        .long_token_and_account
                        .as_ref()
                        .map(|(token, _)| get_associated_token_address(&owner, token)),
                    short_token_ata: hint
                        .short_token_and_account
                        .as_ref()
                        .map(|(token, _)| get_associated_token_address(&owner, token)),
                    associated_token_program: anchor_spl::associated_token::ID,
                    token_program: anchor_spl::token::ID,
                    system_program: system_program::ID,
                    program: *self.client.store_program_id(),
                },
                &gmsol_store::ID,
                self.client.store_program_id(),
            ))
            .args(instruction::CloseOrder {
                reason: self.reason.clone(),
            }))
    }
}

pub(super) fn recent_timestamp() -> crate::Result<i64> {
    use std::time::SystemTime;

    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(crate::Error::unknown)?
        .as_secs()
        .try_into()
        .map_err(|_| crate::Error::unknown("failed to convert timestamp"))
}

pub(super) struct ClaimableAccountsBuilder {
    recent_timestamp: i64,
    store: Pubkey,
    user: Pubkey,
    holding: Pubkey,
    claimable_long_token_account_for_user: Option<(Pubkey, Pubkey)>,
    claimable_short_token_account_for_user: Option<(Pubkey, Pubkey)>,
    claimable_pnl_token_account_for_holding: Option<(Pubkey, Pubkey)>,
}

impl ClaimableAccountsBuilder {
    pub(super) fn new(recent_timestamp: i64, store: Pubkey, user: Pubkey, holding: Pubkey) -> Self {
        Self {
            recent_timestamp,
            store,
            user,
            holding,
            claimable_long_token_account_for_user: None,
            claimable_short_token_account_for_user: None,
            claimable_pnl_token_account_for_holding: None,
        }
    }

    pub(super) fn claimable_long_token_account_for_user(
        &mut self,
        long_token_mint: &Pubkey,
        account: &Pubkey,
    ) -> &mut Self {
        self.claimable_long_token_account_for_user = Some((*long_token_mint, *account));
        self
    }

    pub(super) fn claimable_short_token_account_for_user(
        &mut self,
        short_token_mint: &Pubkey,
        account: &Pubkey,
    ) -> &mut Self {
        self.claimable_short_token_account_for_user = Some((*short_token_mint, *account));
        self
    }

    pub(super) fn claimable_pnl_token_account_for_holding(
        &mut self,
        pnl_token_mint: &Pubkey,
        account: &Pubkey,
    ) -> &mut Self {
        self.claimable_pnl_token_account_for_holding = Some((*pnl_token_mint, *account));
        self
    }

    pub(super) fn build<'a, C: Deref<Target = impl Signer> + Clone>(
        &self,
        client: &'a crate::Client<C>,
    ) -> (RpcBuilder<'a, C>, RpcBuilder<'a, C>) {
        use crate::store::token::TokenAccountOps;

        let mut pre_builder = client.store_rpc();
        let mut post_builder = client.store_rpc();
        let mut accounts: HashSet<&Pubkey> = Default::default();
        if let Some((long_token_mint, account)) =
            self.claimable_long_token_account_for_user.as_ref()
        {
            pre_builder = pre_builder.merge(client.use_claimable_account(
                &self.store,
                long_token_mint,
                &self.user,
                self.recent_timestamp,
                account,
                0,
            ));
            post_builder = post_builder.merge(client.close_empty_claimable_account(
                &self.store,
                long_token_mint,
                &self.user,
                self.recent_timestamp,
                account,
            ));
            accounts.insert(account);
        }
        if let Some((short_token_mint, account)) =
            self.claimable_short_token_account_for_user.as_ref()
        {
            if !accounts.contains(account) {
                pre_builder = pre_builder.merge(client.use_claimable_account(
                    &self.store,
                    short_token_mint,
                    &self.user,
                    self.recent_timestamp,
                    account,
                    0,
                ));
                post_builder = post_builder.merge(client.close_empty_claimable_account(
                    &self.store,
                    short_token_mint,
                    &self.user,
                    self.recent_timestamp,
                    account,
                ));
                accounts.insert(account);
            }
        }
        if let Some((pnl_token_mint, account)) =
            self.claimable_pnl_token_account_for_holding.as_ref()
        {
            if !accounts.contains(account) {
                let holding = &self.holding;
                pre_builder = pre_builder.merge(client.use_claimable_account(
                    &self.store,
                    pnl_token_mint,
                    holding,
                    self.recent_timestamp,
                    account,
                    0,
                ));
                post_builder = post_builder.merge(client.close_empty_claimable_account(
                    &self.store,
                    pnl_token_mint,
                    holding,
                    self.recent_timestamp,
                    account,
                ));
            }
        }
        (pre_builder, post_builder)
    }
}

#[cfg(feature = "pyth-pull-oracle")]
impl<'a, C: Deref<Target = impl Signer> + Clone> ExecuteWithPythPrices<'a, C>
    for ExecuteOrderBuilder<'a, C>
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
        let tx = self
            .parse_with_pyth_price_updates(price_updates)
            .build()
            .await?;
        Ok(tx.into_builders())
    }
}
