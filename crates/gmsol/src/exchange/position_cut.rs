use std::{collections::HashMap, ops::Deref, sync::Arc};

use anchor_client::{
    anchor_lang::{system_program, Id},
    solana_sdk::{address_lookup_table::AddressLookupTableAccount, pubkey::Pubkey, signer::Signer},
};
use anchor_spl::associated_token::get_associated_token_address;
use gmsol_store::{
    accounts, instruction,
    ops::order::PositionCutKind,
    states::{
        common::TokensWithFeed, user::UserHeader, MarketMeta, NonceBytes, Position, Pyth, Store,
        TokenMap,
    },
};

use crate::{
    exchange::generate_nonce,
    store::{token::TokenAccountOps, utils::FeedsParser},
    utils::{fix_optional_account_metas, ComputeBudget, TransactionBuilder, ZeroCopy},
};

use super::{
    order::{recent_timestamp, ClaimableAccountsBuilder, CloseOrderHint},
    ExchangeOps,
};

#[cfg(feature = "pyth-pull-oracle")]
use crate::pyth::pull_oracle::{ExecuteWithPythPrices, Prices, PythPullOracleContext};

/// The compute budget for `position_cut` instruction.
pub const POSITION_CUT_COMPUTE_BUDGET: u32 = 400_000;

/// `PositionCut` instruction builder.
pub struct PositionCutBuilder<'a, C> {
    client: &'a crate::Client<C>,
    kind: PositionCutKind,
    nonce: Option<NonceBytes>,
    recent_timestamp: i64,
    execution_fee: u64,
    oracle: Pubkey,
    position: Pubkey,
    price_provider: Pubkey,
    hint: Option<PositionCutHint>,
    feeds_parser: FeedsParser,
    close: bool,
    event_buffer_index: u8,
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
            .ok_or(crate::Error::invalid_argument(
                "token map is not configurated for the store",
            ))?;
        let token_map = client.token_map(&token_map_address).await?;
        let market = client.find_market_address(&store_address, &position.market_token);
        let meta = *client.market(&market).await?.meta();
        let user = client.find_user_address(&store_address, &position.owner);
        let user = client
            .account::<ZeroCopy<UserHeader>>(&user)
            .await?
            .map(|user| user.0);

        Self::try_new(
            position,
            Arc::new(store),
            &token_map,
            market,
            meta,
            user.as_ref(),
            client.store_program_id(),
        )
    }

    /// Create a new hint.
    pub fn try_new(
        position: &Position,
        store: Arc<Store>,
        token_map: &TokenMap,
        market: Pubkey,
        market_meta: MarketMeta,
        user: Option<&UserHeader>,
        program_id: &Pubkey,
    ) -> crate::Result<Self> {
        use gmsol_store::states::common::token_with_feeds::token_records;

        let records = token_records(
            token_map,
            &[
                market_meta.index_token_mint,
                market_meta.long_token_mint,
                market_meta.short_token_mint,
            ]
            .into(),
        )?;
        let tokens_with_feed = TokensWithFeed::try_from_records(records)?;
        let user_address =
            crate::pda::find_user_pda(&position.store, &position.owner, program_id).0;
        let referrer = user.and_then(|user| user.referral().referrer().copied());

        Ok(Self {
            store_address: position.store,
            owner: position.owner,
            user: user_address,
            referrer,
            token_map: *store.token_map().ok_or(crate::Error::invalid_argument(
                "missing token map for the store",
            ))?,
            market,
            store,
            tokens_with_feed,
            collateral_token: position.collateral_token,
            pnl_token: market_meta.pnl_token(position.try_is_long()?),
            meta: market_meta,
            position_size: position.state.size_in_usd,
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
            price_provider: Pyth::id(),
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
    pub fn event_buffer_index(&mut self, index: u8) -> &mut Self {
        self.event_buffer_index = index;
        self
    }

    /// Set hint with the given position for position cut.
    pub fn hint(&mut self, hint: PositionCutHint) -> &mut Self {
        self.hint = Some(hint);
        self
    }

    /// Set price provider to the given.
    pub fn price_provider(&mut self, program: &Pubkey) -> &mut Self {
        self.price_provider = *program;
        self
    }

    /// Set execution fee.
    pub fn execution_fee(&mut self, lamports: u64) -> &mut Self {
        self.execution_fee = lamports;
        self
    }

    /// Insert an Address Lookup Table.
    pub fn add_alt(&mut self, account: AddressLookupTableAccount) -> &mut Self {
        self.alts.insert(account.key, account.addresses);
        self
    }

    /// Parse feeds with the given price udpates map.
    #[cfg(feature = "pyth-pull-oracle")]
    pub fn parse_with_pyth_price_updates(&mut self, price_updates: Prices) -> &mut Self {
        self.feeds_parser.with_pyth_price_updates(price_updates);
        self
    }

    /// Build [`TransactionBuilder`] for position cut.
    pub async fn build(&mut self) -> crate::Result<TransactionBuilder<'a, C>> {
        let token_program_id = anchor_spl::token::ID;

        let payer = self.client.payer();
        let nonce = self.nonce.unwrap_or_else(generate_nonce);
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
            hint.store.holding(),
            &time_key,
        );
        let feeds = self
            .feeds_parser
            .parse(hint.feeds())
            .collect::<Result<Vec<_>, _>>()?;
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
            .store_rpc()
            .accounts(accounts::PrepareTradeEventBuffer {
                authority: payer,
                store,
                event,
                system_program: system_program::ID,
            })
            .args(instruction::PrepareTradeEventBuffer {
                index: self.event_buffer_index,
            });
        let mut exec_builder = self
            .client
            .store_rpc()
            .accounts(fix_optional_account_metas(
                accounts::PositionCut {
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
                &crate::program_ids::DEFAULT_GMSOL_STORE_ID,
                self.client.store_program_id(),
            ))
            .accounts(feeds)
            .compute_budget(ComputeBudget::default().with_limit(POSITION_CUT_COMPUTE_BUDGET))
            .lookup_tables(self.alts.clone());

        match self.kind {
            PositionCutKind::Liquidate => {
                exec_builder = exec_builder.args(instruction::Liquidate {
                    nonce,
                    recent_timestamp: self.recent_timestamp,
                    execution_fee: self.execution_fee,
                });
            }
            PositionCutKind::AutoDeleverage(size_delta_in_usd) => {
                exec_builder = exec_builder.args(instruction::AutoDeleverage {
                    nonce,
                    recent_timestamp: self.recent_timestamp,
                    size_delta_in_usd,
                    execution_fee: self.execution_fee,
                })
            }
        }

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
            *hint.store.holding(),
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

        let mut builder = TransactionBuilder::new(self.client.data_store().solana_rpc());
        builder
            .try_push(pre_builder.merge(prepare_event_buffer))?
            .try_push(prepare.merge(exec_builder))?
            .try_push(post_builder)?;
        Ok(builder)
    }
}

#[cfg(feature = "pyth-pull-oracle")]
impl<'a, C: Deref<Target = impl Signer> + Clone> ExecuteWithPythPrices<'a, C>
    for PositionCutBuilder<'a, C>
{
    fn set_execution_fee(&mut self, lamports: u64) {
        self.execution_fee(lamports);
    }

    async fn context(&mut self) -> crate::Result<PythPullOracleContext> {
        let hint = self.prepare_hint().await?;
        let ctx = PythPullOracleContext::try_from_feeds(hint.feeds())?;
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
