use std::{collections::BTreeSet, future::Future, ops::Deref};

use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use gmsol_model::{price::Prices, PnlFactorKind};
use gmsol_programs::gmsol_store::{
    client::{accounts, args},
    types::EntryArgs,
};
use gmsol_solana_utils::{
    make_bundle_builder::MakeBundleBuilder, transaction_builder::TransactionBuilder,
};
use gmsol_utils::{
    market::{MarketConfigFlag, MarketConfigKey, MarketMeta},
    oracle::PriceProviderKind,
    token_config::{token_records, TokensWithFeed},
};
use solana_sdk::{pubkey::Pubkey, signer::Signer, system_program};

use crate::{
    client::{
        feeds_parser::{FeedAddressMap, FeedsParser},
        pull_oracle::{FeedIds, PullOraclePriceConsumer},
    },
    Client,
};

use super::token_account::TokenAccountOps;

type Factor = u128;

const DEFAULT_MAX_AGE: u32 = 120;

/// Market operations.
pub trait MarketOps<C> {
    /// Initialize a market vault for the given token.
    fn initialize_market_vault(
        &self,
        store: &Pubkey,
        token: &Pubkey,
    ) -> (TransactionBuilder<C>, Pubkey);

    /// Create a new market and return its token mint address.
    #[allow(clippy::too_many_arguments)]
    fn create_market(
        &self,
        store: &Pubkey,
        name: &str,
        index_token: &Pubkey,
        long_token: &Pubkey,
        short_token: &Pubkey,
        enable: bool,
        token_map: Option<&Pubkey>,
    ) -> impl Future<Output = crate::Result<(TransactionBuilder<C>, Pubkey)>>;

    /// Fund the given market.
    fn fund_market(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        source_account: &Pubkey,
        amount: u64,
        token: Option<&Pubkey>,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C>>>;

    /// Claim fees.
    fn claim_fees(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        is_long_token: bool,
    ) -> ClaimFeesBuilder<C>;

    /// Get market status.
    fn get_market_status(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        prices: Prices<u128>,
        maximize_pnl: bool,
        maximize_pool_value: bool,
    ) -> TransactionBuilder<C>;

    /// Get market token price.
    fn get_market_token_price(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        prices: Prices<u128>,
        pnl_factor: PnlFactorKind,
        maximize: bool,
    ) -> TransactionBuilder<C>;

    /// Get market token value.
    fn get_market_token_value(
        &self,
        store: &Pubkey,
        oracle: &Pubkey,
        market_token: &Pubkey,
        amount: u64,
    ) -> GetMarketTokenValueBuilder<'_, C>;

    /// Update market config.
    fn update_market_config(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        key: &str,
        value: &Factor,
    ) -> crate::Result<TransactionBuilder<C>>;

    /// Update market config flag
    fn update_market_config_flag(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        key: &str,
        value: bool,
    ) -> crate::Result<TransactionBuilder<C>>;

    /// Update market config by key.
    fn update_market_config_by_key(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        key: MarketConfigKey,
        value: &Factor,
    ) -> crate::Result<TransactionBuilder<C>> {
        let key = key.to_string();
        self.update_market_config(store, market_token, &key, value)
    }

    /// Update market config flag by key.
    fn update_market_config_flag_by_key(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        key: MarketConfigFlag,
        value: bool,
    ) -> crate::Result<TransactionBuilder<C>> {
        let key = key.to_string();
        self.update_market_config_flag(store, market_token, &key, value)
    }

    /// Toggle market.
    fn toggle_market(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        enable: bool,
    ) -> TransactionBuilder<C>;

    /// Toggle GT minting.
    fn toggle_gt_minting(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        enable: bool,
    ) -> TransactionBuilder<C>;

    /// Initialize Market Config Buffer.
    fn initialize_market_config_buffer<'a>(
        &'a self,
        store: &Pubkey,
        buffer: &'a dyn Signer,
        expire_after_secs: u32,
    ) -> TransactionBuilder<'a, C>;

    /// Close Market Config Buffer.
    fn close_marekt_config_buffer(
        &self,
        buffer: &Pubkey,
        receiver: Option<&Pubkey>,
    ) -> TransactionBuilder<C>;

    /// Push to Market Config Buffer.
    fn push_to_market_config_buffer<S: ToString>(
        &self,
        buffer: &Pubkey,
        new_configs: impl IntoIterator<Item = (S, Factor)>,
    ) -> TransactionBuilder<C>;

    /// Set the authority of the Market Config Buffer.
    fn set_market_config_buffer_authority(
        &self,
        buffer: &Pubkey,
        new_authority: &Pubkey,
    ) -> TransactionBuilder<C>;

    /// Update Market Config with the buffer.
    fn update_market_config_with_buffer(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        buffer: &Pubkey,
    ) -> TransactionBuilder<C>;
}

impl<C: Deref<Target = impl Signer> + Clone> MarketOps<C> for crate::Client<C> {
    fn initialize_market_vault(
        &self,
        store: &Pubkey,
        token: &Pubkey,
    ) -> (TransactionBuilder<C>, Pubkey) {
        let authority = self.payer();
        let vault = self.find_market_vault_address(store, token);
        let builder = self
            .store_transaction()
            .anchor_accounts(accounts::InitializeMarketVault {
                authority,
                store: *store,
                mint: *token,
                vault,
                system_program: system_program::ID,
                token_program: anchor_spl::token::ID,
            })
            .anchor_args(args::InitializeMarketVault {});
        (builder, vault)
    }

    async fn create_market(
        &self,
        store: &Pubkey,
        name: &str,
        index_token: &Pubkey,
        long_token: &Pubkey,
        short_token: &Pubkey,
        enable: bool,
        token_map: Option<&Pubkey>,
    ) -> crate::Result<(TransactionBuilder<C>, Pubkey)> {
        let token_map = match token_map {
            Some(token_map) => *token_map,
            None => self
                .authorized_token_map_address(store)
                .await?
                .ok_or(crate::Error::NotFound)?,
        };
        let authority = self.payer();
        let market_token =
            self.find_market_token_address(store, index_token, long_token, short_token);
        let prepare_long_token_vault = self.initialize_market_vault(store, long_token).0;
        let prepare_short_token_vault = self.initialize_market_vault(store, short_token).0;
        let prepare_market_token_vault = self.initialize_market_vault(store, &market_token).0;
        let builder = self
            .store_transaction()
            .anchor_accounts(accounts::InitializeMarket {
                authority,
                store: *store,
                token_map,
                market: self.find_market_address(store, &market_token),
                market_token_mint: market_token,
                long_token_mint: *long_token,
                short_token_mint: *short_token,
                long_token_vault: self.find_market_vault_address(store, long_token),
                short_token_vault: self.find_market_vault_address(store, short_token),
                system_program: system_program::ID,
                token_program: anchor_spl::token::ID,
            })
            .anchor_args(args::InitializeMarket {
                name: name.to_string(),
                index_token_mint: *index_token,
                enable,
            });
        Ok((
            prepare_long_token_vault
                .merge(prepare_short_token_vault)
                .merge(builder)
                .merge(prepare_market_token_vault),
            market_token,
        ))
    }

    async fn fund_market(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        source_account: &Pubkey,
        amount: u64,
        token: Option<&Pubkey>,
    ) -> crate::Result<TransactionBuilder<C>> {
        use anchor_spl::token::TokenAccount;

        let token = match token {
            Some(token) => *token,
            None => {
                let account = self
                    .account::<TokenAccount>(source_account)
                    .await?
                    .ok_or(crate::Error::NotFound)?;
                account.mint
            }
        };
        let vault = self.find_market_vault_address(store, &token);
        let market = self.find_market_address(store, market_token);
        Ok(self
            .store_transaction()
            .anchor_args(args::MarketTransferIn { amount })
            .anchor_accounts(accounts::MarketTransferIn {
                authority: self.payer(),
                from_authority: self.payer(),
                store: *store,
                market,
                vault,
                from: *source_account,
                token_program: anchor_spl::token::ID,
                event_authority: self.store_event_authority(),
                program: *self.store_program_id(),
            }))
    }

    fn claim_fees(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        is_long_token: bool,
    ) -> ClaimFeesBuilder<C> {
        ClaimFeesBuilder::new(self, store, market_token, is_long_token)
    }

    fn get_market_status(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        prices: Prices<u128>,
        maximize_pnl: bool,
        maximize_pool_value: bool,
    ) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(args::GetMarketStatus {
                prices: prices.into(),
                maximize_pnl,
                maximize_pool_value,
            })
            .anchor_accounts(accounts::GetMarketStatus {
                market: self.find_market_address(store, market_token),
            })
    }

    fn get_market_token_price(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        prices: Prices<u128>,
        pnl_factor: PnlFactorKind,
        maximize: bool,
    ) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(args::GetMarketTokenPrice {
                prices: prices.into(),
                pnl_factor: pnl_factor.to_string(),
                maximize,
            })
            .anchor_accounts(accounts::GetMarketTokenPrice {
                market: self.find_market_address(store, market_token),
                market_token: *market_token,
            })
    }

    fn get_market_token_value(
        &self,
        store: &Pubkey,
        oracle: &Pubkey,
        market_token: &Pubkey,
        amount: u64,
    ) -> GetMarketTokenValueBuilder<'_, C> {
        GetMarketTokenValueBuilder::new(self, *store, *oracle, *market_token, amount)
    }

    fn update_market_config(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        key: &str,
        value: &Factor,
    ) -> crate::Result<TransactionBuilder<C>> {
        let req = self
            .store_transaction()
            .anchor_args(args::UpdateMarketConfig {
                key: key.to_string(),
                value: *value,
            })
            .anchor_accounts(accounts::UpdateMarketConfig {
                authority: self.payer(),
                store: *store,
                market: self.find_market_address(store, market_token),
            });
        Ok(req)
    }

    fn update_market_config_flag(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        key: &str,
        value: bool,
    ) -> crate::Result<TransactionBuilder<C>> {
        let req = self
            .store_transaction()
            .anchor_args(args::UpdateMarketConfigFlag {
                key: key.to_string(),
                value,
            })
            .anchor_accounts(accounts::UpdateMarketConfig {
                authority: self.payer(),
                store: *store,
                market: self.find_market_address(store, market_token),
            });
        Ok(req)
    }

    fn toggle_market(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        enable: bool,
    ) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(args::ToggleMarket { enable })
            .anchor_accounts(accounts::ToggleMarket {
                authority: self.payer(),
                store: *store,
                market: self.find_market_address(store, market_token),
            })
    }

    fn toggle_gt_minting(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        enable: bool,
    ) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(args::ToggleGtMinting { enable })
            .anchor_accounts(accounts::ToggleGtMinting {
                authority: self.payer(),
                store: *store,
                market: self.find_market_address(store, market_token),
            })
    }

    fn initialize_market_config_buffer<'a>(
        &'a self,
        store: &Pubkey,
        buffer: &'a dyn Signer,
        expire_after_secs: u32,
    ) -> TransactionBuilder<'a, C> {
        self.store_transaction()
            .anchor_args(args::InitializeMarketConfigBuffer { expire_after_secs })
            .anchor_accounts(accounts::InitializeMarketConfigBuffer {
                authority: self.payer(),
                store: *store,
                buffer: buffer.pubkey(),
                system_program: system_program::ID,
            })
            .signer(buffer)
    }

    fn close_marekt_config_buffer(
        &self,
        buffer: &Pubkey,
        receiver: Option<&Pubkey>,
    ) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(args::CloseMarketConfigBuffer {})
            .anchor_accounts(accounts::CloseMarketConfigBuffer {
                authority: self.payer(),
                buffer: *buffer,
                receiver: receiver.copied().unwrap_or(self.payer()),
            })
    }

    fn push_to_market_config_buffer<K: ToString>(
        &self,
        buffer: &Pubkey,
        new_configs: impl IntoIterator<Item = (K, Factor)>,
    ) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(args::PushToMarketConfigBuffer {
                new_configs: new_configs
                    .into_iter()
                    .map(|(key, value)| EntryArgs {
                        key: key.to_string(),
                        value,
                    })
                    .collect(),
            })
            .anchor_accounts(accounts::PushToMarketConfigBuffer {
                authority: self.payer(),
                buffer: *buffer,
                system_program: system_program::ID,
            })
    }

    fn set_market_config_buffer_authority(
        &self,
        buffer: &Pubkey,
        new_authority: &Pubkey,
    ) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(args::SetMarketConfigBufferAuthority {
                new_authority: *new_authority,
            })
            .anchor_accounts(accounts::SetMarketConfigBufferAuthority {
                authority: self.payer(),
                buffer: *buffer,
            })
    }

    fn update_market_config_with_buffer(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        buffer: &Pubkey,
    ) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(args::UpdateMarketConfigWithBuffer {})
            .anchor_accounts(accounts::UpdateMarketConfigWithBuffer {
                authority: self.payer(),
                store: *store,
                market: self.find_market_address(store, market_token),
                buffer: *buffer,
            })
    }
}

/// Claim fees builder.
pub struct ClaimFeesBuilder<'a, C> {
    client: &'a crate::Client<C>,
    store: Pubkey,
    market_token: Pubkey,
    is_long_token: bool,
    hint_token: Option<Pubkey>,
}

impl<'a, C: Deref<Target = impl Signer> + Clone> ClaimFeesBuilder<'a, C> {
    /// Create a new builder.
    pub fn new(
        client: &'a crate::Client<C>,
        store: &Pubkey,
        market_token: &Pubkey,
        is_long_token: bool,
    ) -> Self {
        Self {
            client,
            store: *store,
            market_token: *market_token,
            is_long_token,
            hint_token: None,
        }
    }

    /// Set hint.
    pub fn set_hint(&mut self, token: Pubkey) -> &mut Self {
        self.hint_token = Some(token);
        self
    }

    /// Build.
    pub async fn build(&self) -> crate::Result<TransactionBuilder<'a, C>> {
        let market = self
            .client
            .find_market_address(&self.store, &self.market_token);
        let token = match self.hint_token {
            Some(token) => token,
            None => {
                let market = self.client.market(&market).await?;
                MarketMeta::from(market.meta).pnl_token(self.is_long_token)
            }
        };

        let authority = self.client.payer();
        let vault = self.client.find_market_vault_address(&self.store, &token);
        // Note: If possible, the program ID should be read from the market.
        let token_program = anchor_spl::token::ID;
        let target =
            get_associated_token_address_with_program_id(&authority, &token, &token_program);

        let prepare = self
            .client
            .prepare_associated_token_account(&token, &token_program, None);

        let rpc = self
            .client
            .store_transaction()
            .anchor_accounts(accounts::ClaimFeesFromMarket {
                authority,
                store: self.store,
                market,
                token_mint: token,
                vault,
                target,
                token_program,
                event_authority: self.client.store_event_authority(),
                program: *self.client.store_program_id(),
            })
            .anchor_args(args::ClaimFeesFromMarket {});

        Ok(prepare.merge(rpc))
    }
}

/// Builder for `get_market_token_value` instruction.
pub struct GetMarketTokenValueBuilder<'a, C> {
    client: &'a Client<C>,
    store: Pubkey,
    oracle: Pubkey,
    market_token: Pubkey,
    amount: u64,
    pnl_factor: PnlFactorKind,
    maximize: bool,
    max_age: u32,
    emit_event: bool,
    feeds_parser: FeedsParser,
    hint: Option<GetMarketTokenValueHint>,
}

/// Hint for [`GetMarketTokenValue`].
#[derive(Debug, Clone)]
pub struct GetMarketTokenValueHint {
    /// Token map address.
    pub token_map: Pubkey,
    /// Feeds.
    pub feeds: TokensWithFeed,
}

impl<C> GetMarketTokenValueBuilder<'_, C> {
    /// Set PnL factor kind. Defaults to [`MaxAfterDeposit`](PnlFactorKind::MaxAfterDeposit).
    pub fn pnl_factor(&mut self, kind: PnlFactorKind) -> &mut Self {
        self.pnl_factor = kind;
        self
    }

    /// Set whether to maximize the computed value. Defaults to `false`.
    pub fn maximize(&mut self, maximize: bool) -> &mut Self {
        self.maximize = maximize;
        self
    }

    /// Set max age (seconds). Defaults to `120`.
    pub fn max_age(&mut self, max_age: u32) -> &mut Self {
        self.max_age = max_age;
        self
    }

    /// Set whether to emit event. Defaults to `true`
    pub fn emit_event(&mut self, emit: bool) -> &mut Self {
        self.emit_event = emit;
        self
    }

    /// Set hint.
    pub fn hint(&mut self, hint: Option<GetMarketTokenValueHint>) -> &mut Self {
        self.hint = hint;
        self
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> GetMarketTokenValueBuilder<'a, C> {
    fn new(
        client: &'a Client<C>,
        store: Pubkey,
        oracle: Pubkey,
        market_token: Pubkey,
        amount: u64,
    ) -> Self {
        Self {
            client,
            store,
            oracle,
            market_token,
            amount,
            pnl_factor: PnlFactorKind::MaxAfterDeposit,
            maximize: false,
            max_age: DEFAULT_MAX_AGE,
            emit_event: true,
            feeds_parser: Default::default(),
            hint: None,
        }
    }

    /// Prepare hint.
    pub async fn prepare_hint(&mut self) -> crate::Result<GetMarketTokenValueHint> {
        if let Some(hint) = self.hint.as_ref() {
            return Ok(hint.clone());
        }

        let store = self.client.store(&self.store).await?;
        let token_map_address = store.token_map;
        let market = self
            .client
            .market_by_token(&self.store, &self.market_token)
            .await?;
        let token_map = self.client.token_map(&token_map_address).await?;
        let tokens = ordered_tokens(&market.meta.into());
        let records = token_records(&token_map, &tokens).map_err(crate::Error::custom)?;
        let feeds = TokensWithFeed::try_from_records(records).map_err(crate::Error::custom)?;
        let hint = GetMarketTokenValueHint {
            token_map: token_map_address,
            feeds,
        };
        self.hint = Some(hint.clone());
        Ok(hint)
    }

    async fn build_txn(&mut self) -> crate::Result<TransactionBuilder<'a, C>> {
        let hint = self.prepare_hint().await?;
        let Self {
            client,
            store,
            oracle,
            market_token,
            amount,
            pnl_factor,
            maximize,
            max_age,
            feeds_parser,
            emit_event,
            ..
        } = self;
        let authority = client.payer();
        let feeds = feeds_parser
            .parse(&hint.feeds)
            .collect::<Result<Vec<_>, _>>()?;
        let market = client.find_market_address(store, market_token);
        let txn = client
            .store_transaction()
            .anchor_args(args::GetMarketTokenValue {
                amount: *amount,
                pnl_factor: pnl_factor.to_string(),
                maximize: *maximize,
                max_age: *max_age,
                emit_event: *emit_event,
            })
            .anchor_accounts(accounts::GetMarketTokenValue {
                authority,
                store: *store,
                token_map: hint.token_map,
                oracle: *oracle,
                market,
                market_token: *market_token,
                event_authority: client.store_event_authority(),
                program: *client.store_program_id(),
            })
            .accounts(feeds);
        Ok(txn)
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> MakeBundleBuilder<'a, C>
    for GetMarketTokenValueBuilder<'a, C>
{
    async fn build_with_options(
        &mut self,
        options: gmsol_solana_utils::bundle_builder::BundleOptions,
    ) -> gmsol_solana_utils::Result<gmsol_solana_utils::bundle_builder::BundleBuilder<'a, C>> {
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
    for GetMarketTokenValueBuilder<'_, C>
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

fn ordered_tokens(meta: &MarketMeta) -> BTreeSet<Pubkey> {
    BTreeSet::from([
        meta.index_token_mint,
        meta.long_token_mint,
        meta.short_token_mint,
    ])
}
