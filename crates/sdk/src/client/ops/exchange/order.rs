use std::{
    collections::{BTreeSet, HashMap, HashSet},
    ops::Deref,
    sync::Arc,
};

use anchor_spl::associated_token::get_associated_token_address;
use gmsol_programs::gmsol_store::{
    accounts::{Market, Order, Position, Store, UserHeader},
    client::{accounts, args},
    types::{CreateOrderParams, DecreasePositionSwapType},
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
    market::MarketMeta,
    oracle::PriceProviderKind,
    order::{OrderKind, PositionCutKind},
    pubkey::optional_address,
    swap::SwapActionParams,
    token_config::{token_records, TokenMapAccess, TokensWithFeed},
};
use solana_sdk::{
    address_lookup_table::AddressLookupTableAccount, instruction::AccountMeta, pubkey::Pubkey,
    signer::Signer, system_program,
};

use crate::{
    builders::{
        callback::{Callback, CallbackParams},
        utils::{generate_nonce, get_ata_or_owner},
    },
    client::{
        feeds_parser::{FeedAddressMap, FeedsParser},
        ops::token_account::TokenAccountOps,
        pull_oracle::{FeedIds, PullOraclePriceConsumer},
        token_account::TokenAccountParams,
        token_map::TokenMap,
    },
    pda::NonceBytes,
    utils::{optional::fix_optional_account_metas, zero_copy::ZeroCopy},
};

use super::{ExchangeOps, VirtualInventoryCollector};

/// Compute unit limit for `execute_order`
pub const EXECUTE_ORDER_COMPUTE_BUDGET: u32 = 400_000;

/// The compute budget for `position_cut` instruction.
pub const POSITION_CUT_COMPUTE_BUDGET: u32 = 400_000;

/// The compute budget for `auto_deleverage`.
pub const ADL_COMPUTE_BUDGET: u32 = 800_000;

/// Min execution lamports for deposit.
pub const MIN_EXECUTION_LAMPORTS: u64 = 300_000;

/// Order Params.
#[derive(Debug, Clone)]
pub struct OrderParams {
    /// Order kind.
    pub kind: OrderKind,
    /// Decrease Position Swap Type.
    pub decrease_position_swap_type: Option<DecreasePositionSwapType>,
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
    /// Valid from timestamp.
    pub valid_from_ts: Option<i64>,
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
    should_unwrap_native_token: bool,
    receiver: Pubkey,
    callback: Option<Callback>,
    alts: HashMap<Pubkey, Vec<Pubkey>>,
}

/// Create Order Hint.
#[derive(Clone, Copy)]
pub struct CreateOrderHint {
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
            execution_fee: MIN_EXECUTION_LAMPORTS,
            params,
            swap_path: vec![],
            is_output_token_long,
            hint: None,
            initial_token: Default::default(),
            final_token: Default::default(),
            long_token_account: None,
            short_token_account: None,
            should_unwrap_native_token: true,
            receiver: client.payer(),
            callback: None,
            alts: Default::default(),
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

    /// Setup hint with the given market meta.
    pub fn hint(&mut self, meta: &MarketMeta) -> &mut Self {
        self.hint = Some(CreateOrderHint {
            long_token: meta.long_token_mint,
            short_token: meta.short_token_mint,
        });
        self
    }

    /// Set decrease position swap type.
    pub fn decrease_position_swap_type(
        &mut self,
        ty: Option<DecreasePositionSwapType>,
    ) -> &mut Self {
        self.params.decrease_position_swap_type = ty;
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

    /// Set valid from ts.
    pub fn valid_from_ts(&mut self, ts: i64) -> &mut Self {
        self.params.valid_from_ts = Some(ts);
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

    /// Set callback.
    pub fn callback(&mut self, callback: Option<Callback>) -> &mut Self {
        self.callback = callback;
        self
    }

    /// Participant in a competition.
    #[cfg(competition)]
    pub fn competition(&mut self, competition: &Pubkey) -> &mut Self {
        use crate::programs::gmsol_competition::ID;

        let participant =
            crate::pda::find_participant_address(competition, &self.client.payer(), &ID).0;
        self.callback(Some(Callback {
            version: 0,
            program: ID.into(),
            shared_data: (*competition).into(),
            partitioned_data: participant.into(),
        }))
    }

    /// Insert an Address Lookup Table.
    pub fn add_alt(&mut self, account: AddressLookupTableAccount) -> &mut Self {
        self.alts.insert(account.key, account.addresses);
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
            let market = self.client.market(&self.market()).await?;
            self.hint(&market.meta.into());
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
                    self.params.is_long,
                )?;
                Ok(Some(position))
            }
            OrderKind::MarketSwap | OrderKind::LimitSwap => Ok(None),
            kind => Err(crate::Error::custom(format!(
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
                    return Err(crate::Error::custom(
                        "missing initial collateral token parameters",
                    ));
                };
                Ok(Some((token, account)))
            }
            OrderKind::MarketDecrease
            | OrderKind::Liquidation
            | OrderKind::LimitDecrease
            | OrderKind::StopLossDecrease => Ok(None),
            kind => Err(crate::Error::custom(format!(
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
            kind => Err(crate::Error::custom(format!(
                "unsupported order kind: {kind:?}"
            ))),
        }
    }

    /// Create [`TransactionBuilder`] and return order address.
    pub async fn build_with_address(
        &mut self,
    ) -> crate::Result<(TransactionBuilder<'a, C>, Pubkey)> {
        let (rpc, order, _) = self.build_with_addresses().await?;
        Ok((rpc, order))
    }

    /// Create [`TransactionBuilder`] and return order address and optional position address.
    pub async fn build_with_addresses(
        &mut self,
    ) -> crate::Result<(TransactionBuilder<'a, C>, Pubkey, Option<Pubkey>)> {
        let token_program_id = anchor_spl::token::ID;

        let nonce = self.nonce.unwrap_or_else(|| generate_nonce().to_bytes());
        let owner = &self.client.payer();
        let receiver = self.receiver;
        let order = self.client.find_order_address(&self.store, owner, &nonce);
        let (initial_collateral_token, initial_collateral_token_account) =
            self.initial_collateral_accounts().await?.unzip();
        let final_output_token = self.get_final_output_token().await?;
        let hint = self.prepare_hint().await?;
        let is_swap = matches!(
            self.params.kind,
            OrderKind::LimitSwap | OrderKind::MarketSwap
        );
        let is_decrease = matches!(
            self.params.kind,
            OrderKind::AutoDeleveraging
                | OrderKind::LimitDecrease
                | OrderKind::MarketDecrease
                | OrderKind::StopLossDecrease
        );
        let (long_token, short_token) = if is_swap {
            (None, None)
        } else {
            (Some(hint.long_token), Some(hint.short_token))
        };

        let initial_collateral_token_escrow = initial_collateral_token
            .as_ref()
            .map(|token| get_associated_token_address(&order, token));
        let long_token_accounts = long_token.as_ref().map(|token| {
            let escrow = get_associated_token_address(&order, token);
            let ata = get_associated_token_address(&receiver, token);
            (escrow, ata)
        });
        let short_token_accounts = short_token.as_ref().map(|token| {
            let escrow = get_associated_token_address(&order, token);
            let ata = get_associated_token_address(&receiver, token);
            (escrow, ata)
        });
        let final_output_token_accounts = if is_swap || is_decrease {
            let escrow = get_associated_token_address(&order, &final_output_token);
            let ata = get_associated_token_address(&receiver, &final_output_token);
            Some((escrow, ata))
        } else {
            None
        };
        let position = self.position().await?;
        let user = self.client.find_user_address(&self.store, owner);

        let kind = self.params.kind;
        let params = CreateOrderParams {
            execution_lamports: self.execution_fee,
            swap_path_length: self
                .swap_path
                .len()
                .try_into()
                .map_err(|_| crate::Error::custom("number out of range"))?,
            kind: kind.try_into()?,
            decrease_position_swap_type: self.params.decrease_position_swap_type,
            initial_collateral_delta_amount: self.params.initial_collateral_delta_amount,
            size_delta_value: self.params.size_delta_usd,
            is_long: self.params.is_long,
            is_collateral_long: self.is_output_token_long,
            min_output: Some(self.params.min_output_amount),
            trigger_price: self.params.trigger_price,
            acceptable_price: self.params.acceptable_price,
            should_unwrap_native_token: self.should_unwrap_native_token,
            valid_from_ts: self.params.valid_from_ts,
        };

        let mut prepare = match kind {
            OrderKind::MarketSwap | OrderKind::LimitSwap => {
                let swap_in_token = initial_collateral_token
                    .ok_or(crate::Error::custom("swap in token is not provided"))?;
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
                    Some(&receiver),
                );
                escrow.merge(ata)
            }
            OrderKind::MarketIncrease | OrderKind::LimitIncrease => {
                let initial_collateral_token = initial_collateral_token.ok_or(
                    crate::Error::custom("initial collateral token is not provided"),
                )?;
                let long_token =
                    long_token.ok_or(crate::Error::custom("long token is not provided"))?;
                let short_token =
                    short_token.ok_or(crate::Error::custom("short token is not provided"))?;

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
                    Some(&receiver),
                );
                let short_token_ata = self.client.prepare_associated_token_account(
                    &short_token,
                    &token_program_id,
                    Some(&receiver),
                );

                let prepare_position = self
                    .client
                    .store_transaction()
                    .anchor_accounts(accounts::PreparePosition {
                        owner: *owner,
                        store: self.store,
                        market: self.market(),
                        position: position.expect("must provided"),
                        system_program: system_program::ID,
                    })
                    .anchor_args(args::PreparePosition { params });

                escrow
                    .merge(long_token_ata)
                    .merge(short_token_ata)
                    .merge(prepare_position)
            }
            OrderKind::MarketDecrease | OrderKind::LimitDecrease | OrderKind::StopLossDecrease => {
                let long_token =
                    long_token.ok_or(crate::Error::custom("long token is not provided"))?;
                let short_token =
                    short_token.ok_or(crate::Error::custom("short token is not provided"))?;

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
                    Some(&receiver),
                );
                let short_token_ata = self.client.prepare_associated_token_account(
                    &short_token,
                    &token_program_id,
                    Some(&receiver),
                );
                let final_output_token_ata = self.client.prepare_associated_token_account(
                    &final_output_token,
                    &token_program_id,
                    Some(&receiver),
                );

                escrow
                    .merge(long_token_ata)
                    .merge(short_token_ata)
                    .merge(final_output_token_ata)
            }
            _ => {
                return Err(crate::Error::custom("unsupported order kind"));
            }
        };

        let prepare_user = self
            .client
            .store_transaction()
            .anchor_accounts(accounts::PrepareUser {
                owner: *owner,
                store: self.store,
                user,
                system_program: system_program::ID,
            })
            .anchor_args(args::PrepareUser {});
        prepare = prepare.merge(prepare_user);

        let CallbackParams {
            callback_version,
            callback_authority,
            callback_program,
            callback_shared_data_account,
            callback_partitioned_data_account,
        } = self.client.get_callback_params(self.callback.as_ref());

        #[cfg(competition)]
        if let Some(callback) = self.callback.as_ref() {
            use crate::ops::competition::CompetitionOps;
            if callback.program.0 == crate::programs::gmsol_competition::ID {
                let (prepare_participant, participant) = self
                    .client
                    .create_participant_idempotent(&callback.shared_data, None)
                    .swap_output(());
                if participant != callback.partitioned_data.0 {
                    return Err(crate::Error::custom("invalid participant account"));
                }
                prepare = prepare.merge(prepare_participant);
            }
        }

        let create = self
            .client
            .store_transaction()
            .accounts(fix_optional_account_metas(
                accounts::CreateOrderV2 {
                    store: self.store,
                    order,
                    position,
                    market: self.market(),
                    owner: *owner,
                    receiver,
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
                    system_program: system_program::ID,
                    token_program: anchor_spl::token::ID,
                    associated_token_program: anchor_spl::associated_token::ID,
                    callback_authority,
                    callback_program,
                    callback_shared_data_account,
                    callback_partitioned_data_account,
                    event_authority: self.client.store_event_authority(),
                    program: *self.client.store_program_id(),
                },
                &ID,
                self.client.store_program_id(),
            ))
            .anchor_args(args::CreateOrderV2 {
                nonce,
                params,
                callback_version,
            })
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

        Ok((
            prepare.merge(create).lookup_tables(self.alts.clone()),
            order,
            position,
        ))
    }
}

/// Execute Order Builder.
pub struct ExecuteOrderBuilder<'a, C> {
    client: &'a crate::Client<C>,
    store: Pubkey,
    oracle: Pubkey,
    order: Pubkey,
    execution_fee: u64,
    feeds_parser: FeedsParser,
    recent_timestamp: i64,
    hint: Option<ExecuteOrderHint>,
    token_map: Option<Pubkey>,
    cancel_on_execution_error: bool,
    close: bool,
    event_buffer_index: u16,
    alts: HashMap<Pubkey, Vec<Pubkey>>,
}

/// Hint for executing order.
#[derive(Clone)]
pub struct ExecuteOrderHint {
    kind: OrderKind,
    store_program_id: Pubkey,
    store: Arc<Store>,
    market_token: Pubkey,
    position: Option<Pubkey>,
    owner: Pubkey,
    receiver: Pubkey,
    rent_receiver: Pubkey,
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
    swap: SwapActionParams,
    should_unwrap_native_token: bool,
    callback: Option<Callback>,
    virtual_inventories: BTreeSet<Pubkey>,
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
        Ok(crate::pda::find_claimable_account_address(
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
        Ok(crate::pda::find_claimable_account_address(
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
        Ok(crate::pda::find_claimable_account_address(
            store,
            &self.pnl_token,
            &self.store.address.holding,
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

    /// Set whether to close order after execution.
    pub fn close(&mut self, close: bool) -> &mut Self {
        self.close = close;
        self
    }

    /// Set event buffer index.
    pub fn event_buffer_index(&mut self, index: u16) -> &mut Self {
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
        store: &Arc<Store>,
        map: &impl TokenMapAccess,
        user: Option<&UserHeader>,
        virtual_inventories: BTreeSet<Pubkey>,
    ) -> crate::Result<&mut Self> {
        let params = &order.params;
        let swap = SwapActionParams::from(order.swap);
        let market_token = order.market_token;
        let kind = params.kind()?;
        let tokens = &order.tokens;
        let owner = order.header.owner;
        let rent_receiver = order.header.rent_receiver;
        let user_address = self.client.find_user_address(&self.store, &owner);
        let referrer = user.and_then(|user| optional_address(&user.referral.referrer).copied());
        self.hint = Some(ExecuteOrderHint {
            kind,
            store_program_id: *self.client.store_program_id(),
            store: store.clone(),
            market_token,
            position: optional_address(&params.position).copied(),
            owner,
            receiver: order.header.receiver,
            rent_receiver,
            user: user_address,
            referrer,
            long_token_mint: market.meta.long_token_mint,
            short_token_mint: market.meta.short_token_mint,
            pnl_token: if params.side()?.is_long() {
                market.meta.long_token_mint
            } else {
                market.meta.short_token_mint
            },
            feeds: swap.to_feeds(map).map_err(crate::Error::custom)?,
            swap,
            initial_collateral_token_and_account: tokens.initial_collateral.token_and_account(),
            final_output_token_and_account: tokens.final_output_token.token_and_account(),
            long_token_and_account: tokens.long_token.token_and_account(),
            short_token_and_account: tokens.short_token.token_and_account(),
            should_unwrap_native_token: order
                .header
                .flags
                .get_flag(ActionFlag::ShouldUnwrapNativeToken),
            callback: Callback::from_header(&order.header)?,
            virtual_inventories,
        });
        Ok(self)
    }

    /// Prepare [`ExecuteOrderHint`].
    pub async fn prepare_hint(&mut self) -> crate::Result<ExecuteOrderHint> {
        loop {
            match &self.hint {
                Some(hint) => return Ok(hint.clone()),
                None => {
                    let order = self.client.order(&self.order).await?;
                    let market = self.client.market(&order.header.market).await?;
                    let store = self.client.store(&self.store).await?;
                    let token_map_address = self.get_token_map().await?;
                    let token_map = self.client.token_map(&token_map_address).await?;
                    let owner = order.header.owner;
                    let user = self.client.find_user_address(&self.store, &owner);
                    let user = self
                        .client
                        .account::<ZeroCopy<UserHeader>>(&user)
                        .await?
                        .map(|user| user.0);
                    let swap = order.swap.into();
                    let virtual_inventories = VirtualInventoryCollector::from_swap(&swap)
                        .collect(self.client, &self.store)
                        .await?;
                    self.hint(
                        &order,
                        &market,
                        &store,
                        &token_map,
                        user.as_ref(),
                        virtual_inventories,
                    )?;
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
                .ok_or(crate::Error::custom("token map is not set for this store"))?;
            self.token_map = Some(address);
            Ok(address)
        }
    }

    /// Set token map.
    pub fn token_map(&mut self, address: Pubkey) -> &mut Self {
        self.token_map = Some(address);
        self
    }

    async fn build_txns(&mut self, options: BundleOptions) -> crate::Result<BundleBuilder<'a, C>> {
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
        let virtual_inventories = hint
            .virtual_inventories
            .iter()
            .map(|pubkey| AccountMeta::new(*pubkey, false));
        let event = self.client.find_trade_event_buffer_address(
            &self.store,
            &authority,
            self.event_buffer_index,
        );

        let kind = hint.kind;
        let is_swap = matches!(kind, OrderKind::LimitSwap | OrderKind::MarketSwap);
        let mut require_claimable_accounts = false;

        let CallbackParams {
            callback_authority,
            callback_program,
            callback_shared_data_account,
            callback_partitioned_data_account,
            ..
        } = self.client.get_callback_params(hint.callback.as_ref());

        let mut execute_order = match kind {
            OrderKind::MarketDecrease | OrderKind::LimitDecrease | OrderKind::StopLossDecrease => {
                require_claimable_accounts = true;

                self.client
                    .store_transaction()
                    .accounts(fix_optional_account_metas(
                        accounts::ExecuteDecreaseOrderV2 {
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
                                .ok_or(crate::Error::custom("missing position"))?,
                            event,
                            final_output_token_vault: hint
                                .final_output_token_and_account
                                .as_ref()
                                .map(|(token, _)| {
                                    self.client.find_market_vault_address(&self.store, token)
                                })
                                .ok_or(crate::Error::custom("missing final output token"))?,
                            long_token_vault: hint
                                .long_token_vault(&self.store)
                                .ok_or(crate::Error::custom("missing long token"))?,
                            short_token_vault: hint
                                .short_token_vault(&self.store)
                                .ok_or(crate::Error::custom("missing short token"))?,
                            claimable_long_token_account_for_user,
                            claimable_short_token_account_for_user,
                            claimable_pnl_token_account_for_holding,
                            event_authority: self.client.store_event_authority(),
                            token_program: anchor_spl::token::ID,
                            system_program: system_program::ID,
                            long_token: hint
                                .long_token_and_account
                                .map(|(token, _)| token)
                                .ok_or(crate::Error::custom("missing long token"))?,
                            short_token: hint
                                .short_token_and_account
                                .map(|(token, _)| token)
                                .ok_or(crate::Error::custom("missing short token"))?,
                            final_output_token: hint
                                .final_output_token_and_account
                                .map(|(token, _)| token)
                                .ok_or(crate::Error::custom("missing final output token"))?,
                            final_output_token_escrow: hint
                                .final_output_token_and_account
                                .map(|(_, account)| account)
                                .ok_or(crate::Error::custom("missing final output token"))?,
                            long_token_escrow: hint
                                .long_token_and_account
                                .map(|(_, account)| account)
                                .ok_or(crate::Error::custom("missing long token"))?,
                            short_token_escrow: hint
                                .short_token_and_account
                                .map(|(_, account)| account)
                                .ok_or(crate::Error::custom("missing short token"))?,
                            program: *self.client.store_program_id(),
                            callback_authority,
                            callback_program,
                            callback_shared_data_account,
                            callback_partitioned_data_account,
                        },
                        &ID,
                        self.client.store_program_id(),
                    ))
                    .anchor_args(args::ExecuteDecreaseOrderV2 {
                        recent_timestamp: self.recent_timestamp,
                        execution_fee: self.execution_fee,
                        throw_on_execution_error: !self.cancel_on_execution_error,
                    })
            }
            _ => self
                .client
                .store_transaction()
                .accounts(fix_optional_account_metas(
                    accounts::ExecuteIncreaseOrSwapOrderV2 {
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
                        event: (!is_swap).then_some(event),
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
                        callback_authority,
                        callback_program,
                        callback_shared_data_account,
                        callback_partitioned_data_account,
                    },
                    &ID,
                    self.client.store_program_id(),
                ))
                .anchor_args(args::ExecuteIncreaseOrSwapOrderV2 {
                    recent_timestamp: self.recent_timestamp,
                    execution_fee: self.execution_fee,
                    throw_on_execution_error: !self.cancel_on_execution_error,
                }),
        };

        execute_order = execute_order
            .accounts(
                feeds
                    .into_iter()
                    .chain(swap_markets)
                    .chain(virtual_inventories)
                    .collect::<Vec<_>>(),
            )
            .compute_budget(ComputeBudget::default().with_limit(EXECUTE_ORDER_COMPUTE_BUDGET))
            .lookup_tables(self.alts.clone());

        if !is_swap {
            let prepare_event_buffer = self
                .client
                .store_transaction()
                .anchor_accounts(accounts::PrepareTradeEventBuffer {
                    authority,
                    store: self.store,
                    event,
                    system_program: system_program::ID,
                })
                .anchor_args(args::PrepareTradeEventBuffer {
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
                    receiver: hint.receiver,
                    store: self.store,
                    initial_collateral_token_and_account: hint.initial_collateral_token_and_account,
                    final_output_token_and_account: hint.final_output_token_and_account,
                    long_token_and_account: hint.long_token_and_account,
                    short_token_and_account: hint.short_token_and_account,
                    user: hint.user,
                    referrer: hint.referrer,
                    rent_receiver: hint.rent_receiver,
                    should_unwrap_native_token: hint.should_unwrap_native_token,
                    callback: hint.callback,
                })
                .build()
                .await?;
            execute_order = execute_order.merge(close);
        }

        let mut builder = ClaimableAccountsBuilder::new(
            self.recent_timestamp,
            self.store,
            hint.owner,
            hint.store.address.holding,
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

        let mut bundle = self.client.bundle_with_options(options);
        bundle
            .try_push(pre_builder)
            .map_err(|(_, err)| err)?
            .try_push(execute_order)
            .map_err(|(_, err)| err)?
            .try_push(post_builder)
            .map_err(|(_, err)| err)?;
        Ok(bundle)
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> MakeBundleBuilder<'a, C>
    for ExecuteOrderBuilder<'a, C>
{
    async fn build_with_options(
        &mut self,
        options: BundleOptions,
    ) -> gmsol_solana_utils::Result<BundleBuilder<'a, C>> {
        self.build_txns(options)
            .await
            .map_err(gmsol_solana_utils::Error::custom)
    }
}

impl<C: Deref<Target = impl Signer> + Clone> PullOraclePriceConsumer
    for ExecuteOrderBuilder<'_, C>
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

impl<C> SetExecutionFee for ExecuteOrderBuilder<'_, C> {
    fn set_execution_fee(&mut self, lamports: u64) -> &mut Self {
        self.execution_fee = lamports;
        self
    }
}

/// Close Order Builder.
pub struct CloseOrderBuilder<'a, C> {
    client: &'a crate::Client<C>,
    order: Pubkey,
    hint: Option<CloseOrderHint>,
    reason: String,
    skip_callback: bool,
}

/// Close Order Hint.
#[derive(Clone)]
pub struct CloseOrderHint {
    pub(super) owner: Pubkey,
    pub(super) receiver: Pubkey,
    pub(super) store: Pubkey,
    pub(super) initial_collateral_token_and_account: Option<(Pubkey, Pubkey)>,
    pub(super) final_output_token_and_account: Option<(Pubkey, Pubkey)>,
    pub(super) long_token_and_account: Option<(Pubkey, Pubkey)>,
    pub(super) short_token_and_account: Option<(Pubkey, Pubkey)>,
    pub(super) user: Pubkey,
    pub(super) referrer: Option<Pubkey>,
    pub(super) rent_receiver: Pubkey,
    pub(super) should_unwrap_native_token: bool,
    pub(super) callback: Option<Callback>,
}

impl CloseOrderHint {
    /// Create hint from order and user account.
    pub fn new(
        order: &Order,
        user: Option<&UserHeader>,
        program_id: &Pubkey,
    ) -> crate::Result<Self> {
        let tokens = &order.tokens;
        let owner = order.header.owner;
        let store = order.header.store;
        let user_address = crate::pda::find_user_address(&store, &owner, program_id).0;
        let referrer = user.and_then(|user| optional_address(&user.referral.referrer).copied());
        let rent_receiver = order.header.rent_receiver;
        Ok(Self {
            owner,
            receiver: order.header.receiver,
            store,
            user: user_address,
            referrer,
            initial_collateral_token_and_account: tokens.initial_collateral.token_and_account(),
            final_output_token_and_account: tokens.final_output_token.token_and_account(),
            long_token_and_account: tokens.long_token.token_and_account(),
            short_token_and_account: tokens.short_token.token_and_account(),
            rent_receiver,
            should_unwrap_native_token: order
                .header
                .flags
                .get_flag(ActionFlag::ShouldUnwrapNativeToken),
            callback: Callback::from_header(&order.header)?,
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
            skip_callback: false,
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

    /// Set whether to skip callback.
    pub fn skip_callback(&mut self, skip: bool) -> &mut Self {
        self.skip_callback = skip;
        self
    }

    async fn prepare_hint(&mut self) -> crate::Result<CloseOrderHint> {
        match &self.hint {
            Some(hint) => Ok(hint.clone()),
            None => {
                let order: ZeroCopy<Order> = self
                    .client
                    .account_with_config(&self.order, Default::default())
                    .await?
                    .into_value()
                    .ok_or(crate::Error::custom("order not found"))?;
                let user = self
                    .client
                    .find_user_address(&order.0.header.store, &order.0.header.owner);
                let user = self.client.account::<ZeroCopy<_>>(&user).await?;
                let hint = CloseOrderHint::new(
                    &order.0,
                    user.as_ref().map(|user| &user.0),
                    self.client.store_program_id(),
                )?;
                self.hint = Some(hint.clone());
                Ok(hint)
            }
        }
    }

    /// Build [`TransactionBuilder`] for cancelling the order.
    pub async fn build(&mut self) -> crate::Result<TransactionBuilder<'a, C>> {
        let hint = self.prepare_hint().await?;
        let payer = self.client.payer();
        let owner = hint.owner;
        let referrer_user = hint
            .referrer
            .map(|owner| self.client.find_user_address(&hint.store, &owner));
        let CallbackParams {
            callback_authority,
            callback_program,
            callback_shared_data_account,
            callback_partitioned_data_account,
            ..
        } = self.client.get_callback_params(
            (!self.skip_callback)
                .then_some(hint.callback.as_ref())
                .flatten(),
        );
        Ok(self
            .client
            .store_transaction()
            .accounts(fix_optional_account_metas(
                accounts::CloseOrderV2 {
                    store: hint.store,
                    store_wallet: self.client.find_store_wallet_address(&hint.store),
                    event_authority: self.client.store_event_authority(),
                    order: self.order,
                    executor: payer,
                    owner,
                    receiver: hint.receiver,
                    rent_receiver: hint.rent_receiver,
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
                        .map(|(token, _)| {
                            get_ata_or_owner(&owner, token, hint.should_unwrap_native_token)
                        }),
                    final_output_token_ata: hint.final_output_token_and_account.as_ref().map(
                        |(token, _)| {
                            get_ata_or_owner(&hint.receiver, token, hint.should_unwrap_native_token)
                        },
                    ),
                    long_token_ata: hint.long_token_and_account.as_ref().map(|(token, _)| {
                        get_ata_or_owner(&hint.receiver, token, hint.should_unwrap_native_token)
                    }),
                    short_token_ata: hint.short_token_and_account.as_ref().map(|(token, _)| {
                        get_ata_or_owner(&hint.receiver, token, hint.should_unwrap_native_token)
                    }),
                    associated_token_program: anchor_spl::associated_token::ID,
                    token_program: anchor_spl::token::ID,
                    system_program: system_program::ID,
                    program: *self.client.store_program_id(),
                    callback_authority,
                    callback_program,
                    callback_shared_data_account,
                    callback_partitioned_data_account,
                },
                &ID,
                self.client.store_program_id(),
            ))
            .anchor_args(args::CloseOrderV2 {
                reason: self.reason.clone(),
            }))
    }
}

pub(super) fn recent_timestamp() -> crate::Result<i64> {
    use std::time::SystemTime;

    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(crate::Error::custom)?
        .as_secs()
        .try_into()
        .map_err(|_| crate::Error::custom("failed to convert timestamp"))
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
    ) -> (TransactionBuilder<'a, C>, TransactionBuilder<'a, C>) {
        let mut pre_builder = client.store_transaction();
        let mut post_builder = client.store_transaction();
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

/// `PositionCut` instruction builder.
pub struct PositionCutBuilder<'a, C> {
    client: &'a crate::Client<C>,
    kind: PositionCutKind,
    nonce: Option<NonceBytes>,
    recent_timestamp: i64,
    execution_fee: u64,
    oracle: Pubkey,
    position: Pubkey,
    hint: Option<PositionCutHint>,
    feeds_parser: FeedsParser,
    close: bool,
    event_buffer_index: u16,
    alts: HashMap<Pubkey, Vec<Pubkey>>,
}

/// Hint for `PositionCut`.
#[derive(Clone)]
pub struct PositionCutHint {
    tokens_with_feed: TokensWithFeed,
    meta: MarketMeta,
    store_address: Pubkey,
    owner: Pubkey,
    user: Pubkey,
    referrer: Option<Pubkey>,
    store: Arc<Store>,
    collateral_token: Pubkey,
    pnl_token: Pubkey,
    token_map: Pubkey,
    market: Pubkey,
    position_size: u128,
    virtual_inventories: BTreeSet<Pubkey>,
}

impl PositionCutHint {
    /// Create from position.
    pub async fn from_position<C: Deref<Target = impl Signer> + Clone>(
        client: &crate::Client<C>,
        position: &Position,
    ) -> crate::Result<Self> {
        let store_address = position.store;
        let store = client.store(&store_address).await?;
        let token_map_address = client
            .authorized_token_map_address(&store_address)
            .await?
            .ok_or(crate::Error::custom(
                "token map is not configurated for the store",
            ))?;
        let token_map = client.token_map(&token_map_address).await?;
        let market = client.find_market_address(&store_address, &position.market_token);
        let meta = client.market(&market).await?.meta.into();
        let user = client.find_user_address(&store_address, &position.owner);
        let user = client
            .account::<ZeroCopy<UserHeader>>(&user)
            .await?
            .map(|user| user.0);
        let virtual_inventories = VirtualInventoryCollector::default()
            .insert_market_token(&position.market_token)
            .collect(client, &store_address)
            .await?;

        Self::try_new(
            position,
            store,
            &token_map,
            market,
            meta,
            user.as_ref(),
            client.store_program_id(),
            virtual_inventories,
        )
    }

    /// Create a new hint.
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        position: &Position,
        store: Arc<Store>,
        token_map: &TokenMap,
        market: Pubkey,
        market_meta: MarketMeta,
        user: Option<&UserHeader>,
        program_id: &Pubkey,
        virtual_inventories: BTreeSet<Pubkey>,
    ) -> crate::Result<Self> {
        let records = token_records(
            token_map,
            &[
                market_meta.index_token_mint,
                market_meta.long_token_mint,
                market_meta.short_token_mint,
            ]
            .into(),
        )
        .map_err(crate::Error::custom)?;
        let tokens_with_feed =
            TokensWithFeed::try_from_records(records).map_err(crate::Error::custom)?;
        let user_address =
            crate::pda::find_user_address(&position.store, &position.owner, program_id).0;
        let referrer = user.and_then(|user| optional_address(&user.referral.referrer).copied());

        Ok(Self {
            store_address: position.store,
            owner: position.owner,
            user: user_address,
            referrer,
            token_map: optional_address(&store.token_map)
                .copied()
                .ok_or(crate::Error::custom("missing token map for the store"))?,
            market,
            store,
            tokens_with_feed,
            collateral_token: position.collateral_token,
            pnl_token: market_meta.pnl_token(position.try_is_long()?),
            meta: market_meta,
            position_size: position.state.size_in_usd,
            virtual_inventories,
        })
    }

    /// Get feeds.
    pub fn feeds(&self) -> &TokensWithFeed {
        &self.tokens_with_feed
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> PositionCutBuilder<'a, C> {
    pub(super) fn try_new(
        client: &'a crate::Client<C>,
        kind: PositionCutKind,
        oracle: &Pubkey,
        position: &Pubkey,
    ) -> crate::Result<Self> {
        Ok(Self {
            client,
            kind,
            oracle: *oracle,
            nonce: None,
            recent_timestamp: recent_timestamp()?,
            execution_fee: 0,
            position: *position,
            hint: None,
            feeds_parser: Default::default(),
            close: true,
            event_buffer_index: 0,
            alts: Default::default(),
        })
    }

    /// Prepare hint for position cut.
    pub async fn prepare_hint(&mut self) -> crate::Result<PositionCutHint> {
        match &self.hint {
            Some(hint) => Ok(hint.clone()),
            None => {
                let position = self.client.position(&self.position).await?;
                let hint = PositionCutHint::from_position(self.client, &position).await?;
                self.hint = Some(hint.clone());
                Ok(hint)
            }
        }
    }

    /// Set whether to close the order after the execution.
    pub fn close(&mut self, close: bool) -> &mut Self {
        self.close = close;
        self
    }

    /// Set event buffer index.
    pub fn event_buffer_index(&mut self, index: u16) -> &mut Self {
        self.event_buffer_index = index;
        self
    }

    /// Set hint with the given position for position cut.
    pub fn hint(&mut self, hint: PositionCutHint) -> &mut Self {
        self.hint = Some(hint);
        self
    }

    /// Insert an Address Lookup Table.
    pub fn add_alt(&mut self, account: AddressLookupTableAccount) -> &mut Self {
        self.alts.insert(account.key, account.addresses);
        self
    }

    async fn build_txns(&mut self, options: BundleOptions) -> crate::Result<BundleBuilder<'a, C>> {
        let token_program_id = anchor_spl::token::ID;

        let payer = self.client.payer();
        let nonce = self.nonce.unwrap_or_else(|| generate_nonce().to_bytes());
        let hint = self.prepare_hint().await?;
        let owner = hint.owner;
        let store = hint.store_address;
        let meta = &hint.meta;
        let long_token_mint = meta.long_token_mint;
        let short_token_mint = meta.short_token_mint;

        let time_key = hint.store.claimable_time_key(self.recent_timestamp)?;
        let claimable_long_token_account_for_user =
            self.client
                .find_claimable_account_address(&store, &long_token_mint, &owner, &time_key);
        let claimable_short_token_account_for_user = self.client.find_claimable_account_address(
            &store,
            &short_token_mint,
            &owner,
            &time_key,
        );
        let claimable_pnl_token_account_for_holding = self.client.find_claimable_account_address(
            &store,
            &hint.pnl_token,
            &hint.store.address.holding,
            &time_key,
        );
        let feeds = self.feeds_parser.parse_and_sort_by_tokens(hint.feeds())?;
        let virtual_inventories = hint
            .virtual_inventories
            .iter()
            .map(|pubkey| AccountMeta::new(*pubkey, false))
            .collect();

        let order = self.client.find_order_address(&store, &payer, &nonce);

        let long_token_escrow = get_associated_token_address(&order, &long_token_mint);
        let short_token_escrow = get_associated_token_address(&order, &short_token_mint);
        let output_token_escrow = get_associated_token_address(&order, &hint.collateral_token);
        let long_token_vault = self
            .client
            .find_market_vault_address(&store, &long_token_mint);
        let short_token_vault = self
            .client
            .find_market_vault_address(&store, &short_token_mint);
        let event =
            self.client
                .find_trade_event_buffer_address(&store, &payer, self.event_buffer_index);

        let prepare = self
            .client
            .prepare_associated_token_account(
                &hint.collateral_token,
                &token_program_id,
                Some(&order),
            )
            .merge(self.client.prepare_associated_token_account(
                &long_token_mint,
                &token_program_id,
                Some(&order),
            ))
            .merge(self.client.prepare_associated_token_account(
                &short_token_mint,
                &token_program_id,
                Some(&order),
            ));
        let prepare_event_buffer = self
            .client
            .store_transaction()
            .anchor_accounts(accounts::PrepareTradeEventBuffer {
                authority: payer,
                store,
                event,
                system_program: system_program::ID,
            })
            .anchor_args(args::PrepareTradeEventBuffer {
                index: self.event_buffer_index,
            });
        let mut exec_builder = self.client.store_transaction();

        match self.kind {
            PositionCutKind::Liquidate => {
                exec_builder = exec_builder
                    .accounts(fix_optional_account_metas(
                        accounts::Liquidate {
                            authority: payer,
                            owner,
                            user: hint.user,
                            store,
                            token_map: hint.token_map,
                            oracle: self.oracle,
                            market: hint.market,
                            order,
                            position: self.position,
                            event,
                            long_token: long_token_mint,
                            short_token: short_token_mint,
                            long_token_escrow,
                            short_token_escrow,
                            long_token_vault,
                            short_token_vault,
                            claimable_long_token_account_for_user,
                            claimable_short_token_account_for_user,
                            claimable_pnl_token_account_for_holding,
                            system_program: system_program::ID,
                            token_program: anchor_spl::token::ID,
                            associated_token_program: anchor_spl::associated_token::ID,
                            event_authority: self.client.store_event_authority(),
                            program: *self.client.store_program_id(),
                            chainlink_program: None,
                        },
                        &ID,
                        self.client.store_program_id(),
                    ))
                    .anchor_args(args::Liquidate {
                        nonce,
                        recent_timestamp: self.recent_timestamp,
                        execution_fee: self.execution_fee,
                    });
            }
            PositionCutKind::AutoDeleverage(size_delta_in_usd) => {
                exec_builder = exec_builder
                    .accounts(fix_optional_account_metas(
                        accounts::AutoDeleverage {
                            authority: payer,
                            owner,
                            user: hint.user,
                            store,
                            token_map: hint.token_map,
                            oracle: self.oracle,
                            market: hint.market,
                            order,
                            position: self.position,
                            event,
                            long_token: long_token_mint,
                            short_token: short_token_mint,
                            long_token_escrow,
                            short_token_escrow,
                            long_token_vault,
                            short_token_vault,
                            claimable_long_token_account_for_user,
                            claimable_short_token_account_for_user,
                            claimable_pnl_token_account_for_holding,
                            system_program: system_program::ID,
                            token_program: anchor_spl::token::ID,
                            associated_token_program: anchor_spl::associated_token::ID,
                            event_authority: self.client.store_event_authority(),
                            program: *self.client.store_program_id(),
                            chainlink_program: None,
                        },
                        &ID,
                        self.client.store_program_id(),
                    ))
                    .anchor_args(args::AutoDeleverage {
                        nonce,
                        recent_timestamp: self.recent_timestamp,
                        size_delta_in_usd,
                        execution_fee: self.execution_fee,
                    })
            }
        }

        exec_builder = exec_builder
            .accounts(feeds)
            .accounts(virtual_inventories)
            .compute_budget(ComputeBudget::default().with_limit(POSITION_CUT_COMPUTE_BUDGET))
            .lookup_tables(self.alts.clone());

        let is_full_close = match self.kind {
            PositionCutKind::Liquidate => true,
            PositionCutKind::AutoDeleverage(size) => size >= hint.position_size,
        };

        if self.close {
            let close = self
                .client
                .close_order(&order)?
                .hint(CloseOrderHint {
                    owner,
                    receiver: owner,
                    store,
                    initial_collateral_token_and_account: None,
                    final_output_token_and_account: Some((
                        hint.collateral_token,
                        output_token_escrow,
                    )),
                    long_token_and_account: Some((long_token_mint, long_token_escrow)),
                    short_token_and_account: Some((short_token_mint, short_token_escrow)),
                    user: hint.user,
                    referrer: hint.referrer,
                    rent_receiver: if is_full_close { owner } else { payer },
                    should_unwrap_native_token: true,
                    callback: None,
                })
                .reason("position cut")
                .build()
                .await?;
            exec_builder = exec_builder.merge(close);
        }

        let (pre_builder, post_builder) = ClaimableAccountsBuilder::new(
            self.recent_timestamp,
            store,
            owner,
            hint.store.address.holding,
        )
        .claimable_long_token_account_for_user(
            &long_token_mint,
            &claimable_long_token_account_for_user,
        )
        .claimable_short_token_account_for_user(
            &short_token_mint,
            &claimable_short_token_account_for_user,
        )
        .claimable_pnl_token_account_for_holding(
            &hint.pnl_token,
            &claimable_pnl_token_account_for_holding,
        )
        .build(self.client);

        let mut bundle = self.client.bundle_with_options(options);
        bundle
            .try_push(pre_builder.merge(prepare_event_buffer))
            .map_err(|(_, err)| err)?
            .try_push(prepare.merge(exec_builder))
            .map_err(|(_, err)| err)?
            .try_push(post_builder)
            .map_err(|(_, err)| err)?;
        Ok(bundle)
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> MakeBundleBuilder<'a, C>
    for PositionCutBuilder<'a, C>
{
    async fn build_with_options(
        &mut self,
        options: BundleOptions,
    ) -> gmsol_solana_utils::Result<BundleBuilder<'a, C>> {
        self.build_txns(options)
            .await
            .map_err(gmsol_solana_utils::Error::custom)
    }
}

impl<C: Deref<Target = impl Signer> + Clone> PullOraclePriceConsumer for PositionCutBuilder<'_, C> {
    async fn feed_ids(&mut self) -> crate::Result<FeedIds> {
        let hint = self.prepare_hint().await?;
        Ok(FeedIds::new(hint.store_address, hint.tokens_with_feed))
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

impl<C> SetExecutionFee for PositionCutBuilder<'_, C> {
    fn set_execution_fee(&mut self, lamports: u64) -> &mut Self {
        self.execution_fee = lamports;
        self
    }
}

/// Update ADL state Instruction Builder.
pub struct UpdateAdlBuilder<'a, C> {
    client: &'a crate::Client<C>,
    store: Pubkey,
    market_token: Pubkey,
    oracle: Pubkey,
    for_long: bool,
    for_short: bool,
    hint: Option<UpdateAdlHint>,
    feeds_parser: FeedsParser,
    alts: HashMap<Pubkey, Vec<Pubkey>>,
}

impl<'a, C: Deref<Target = impl Signer> + Clone> UpdateAdlBuilder<'a, C> {
    pub(super) fn try_new(
        client: &'a crate::Client<C>,
        store: &Pubkey,
        oracle: &Pubkey,
        market_token: &Pubkey,
        for_long: bool,
        for_short: bool,
    ) -> crate::Result<Self> {
        Ok(Self {
            client,
            store: *store,
            market_token: *market_token,
            oracle: *oracle,
            for_long,
            for_short,
            hint: None,
            feeds_parser: FeedsParser::default(),
            alts: Default::default(),
        })
    }

    /// Insert an Address Lookup Table.
    pub fn add_alt(&mut self, account: AddressLookupTableAccount) -> &mut Self {
        self.alts.insert(account.key, account.addresses);
        self
    }

    /// Prepare hint for auto-deleveraging.
    pub async fn prepare_hint(&mut self) -> crate::Result<UpdateAdlHint> {
        match &self.hint {
            Some(hint) => Ok(hint.clone()),
            None => {
                let market_address = self
                    .client
                    .find_market_address(&self.store, &self.market_token);
                let market = self.client.market(&market_address).await?;
                let hint = UpdateAdlHint::from_market(self.client, &market).await?;
                self.hint = Some(hint.clone());
                Ok(hint)
            }
        }
    }

    /// Build [`TransactionBuilder`] for auto-delevearaging the position.
    pub async fn build_txns(&mut self) -> crate::Result<Vec<TransactionBuilder<'a, C>>> {
        let hint = self.prepare_hint().await?;
        let feeds = self
            .feeds_parser
            .parse(hint.feeds())
            .collect::<Result<Vec<_>, _>>()?;

        let mut txns = vec![];

        let sides = self
            .for_long
            .then_some(true)
            .into_iter()
            .chain(self.for_short.then_some(false));

        for is_long in sides {
            let rpc = self
                .client
                .store_transaction()
                .accounts(fix_optional_account_metas(
                    accounts::UpdateAdlState {
                        authority: self.client.payer(),
                        store: self.store,
                        token_map: hint.token_map,
                        oracle: self.oracle,
                        market: self
                            .client
                            .find_market_address(&self.store, &self.market_token),
                        chainlink_program: None,
                    },
                    &ID,
                    self.client.store_program_id(),
                ))
                .anchor_args(args::UpdateAdlState { is_long })
                .accounts(feeds.clone())
                .lookup_tables(self.alts.clone());

            txns.push(rpc);
        }

        Ok(txns)
    }
}

/// Hint for `update_adl_state`.
#[derive(Clone)]
pub struct UpdateAdlHint {
    token_map: Pubkey,
    tokens_with_feed: TokensWithFeed,
}

impl UpdateAdlHint {
    async fn from_market<C: Deref<Target = impl Signer> + Clone>(
        client: &crate::Client<C>,
        market: &Market,
    ) -> crate::Result<Self> {
        let store_address = market.store;
        let token_map_address = client
            .authorized_token_map_address(&store_address)
            .await?
            .ok_or(crate::Error::custom(
                "token map is not configurated for the store",
            ))?;
        let token_map = client.token_map(&token_map_address).await?;
        let meta: MarketMeta = market.meta.into();

        let records = token_records(
            &token_map,
            &[
                meta.index_token_mint,
                meta.long_token_mint,
                meta.short_token_mint,
            ]
            .into(),
        )
        .map_err(crate::Error::custom)?;
        let tokens_with_feed =
            TokensWithFeed::try_from_records(records).map_err(crate::Error::custom)?;

        Ok(Self {
            token_map: token_map_address,
            tokens_with_feed,
        })
    }

    /// Get feeds.
    pub fn feeds(&self) -> &TokensWithFeed {
        &self.tokens_with_feed
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> MakeBundleBuilder<'a, C>
    for UpdateAdlBuilder<'a, C>
{
    async fn build_with_options(
        &mut self,
        options: BundleOptions,
    ) -> gmsol_solana_utils::Result<BundleBuilder<'a, C>> {
        let mut bundle = self.client.bundle_with_options(options);

        bundle.push_many(
            self.build_txns()
                .await
                .map_err(gmsol_solana_utils::Error::custom)?,
            false,
        )?;

        Ok(bundle)
    }
}

impl<C: Deref<Target = impl Signer> + Clone> PullOraclePriceConsumer for UpdateAdlBuilder<'_, C> {
    async fn feed_ids(&mut self) -> crate::Result<FeedIds> {
        let hint = self.prepare_hint().await?;
        Ok(FeedIds::new(self.store, hint.tokens_with_feed))
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

impl<C> SetExecutionFee for UpdateAdlBuilder<'_, C> {
    fn is_execution_fee_estimation_required(&self) -> bool {
        false
    }

    fn set_execution_fee(&mut self, _lamports: u64) -> &mut Self {
        self
    }
}
