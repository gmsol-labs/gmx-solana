use std::{collections::BTreeSet, ops::Deref};

use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use gmsol_programs::gmsol_store::{
    accounts::Glv,
    client::{accounts, args},
    types::UpdateGlvParams,
};
use gmsol_solana_utils::{
    make_bundle_builder::MakeBundleBuilder, transaction_builder::TransactionBuilder,
};
use gmsol_utils::{
    glv::GlvMarketFlag, oracle::PriceProviderKind, swap::SwapActionParams,
    token_config::TokensWithFeed,
};
use solana_sdk::{instruction::AccountMeta, pubkey::Pubkey, signer::Signer, system_program};

use crate::{
    client::{
        feeds_parser::{FeedAddressMap, FeedsParser},
        pull_oracle::{FeedIds, PullOraclePriceConsumer},
    },
    utils::zero_copy::ZeroCopy,
    Client,
};

const DEFAULT_MAX_AGE: u32 = 120;

/// GLV operations.
pub trait GlvOps<C> {
    /// Initialize GLV.
    fn initialize_glv(
        &self,
        store: &Pubkey,
        index: u16,
        market_tokens: impl IntoIterator<Item = Pubkey>,
    ) -> crate::Result<(TransactionBuilder<C>, Pubkey)>;

    /// GLV Update Market Config.
    fn update_glv_market_config(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
        max_amount: Option<u64>,
        max_value: Option<u128>,
    ) -> TransactionBuilder<C>;

    /// GLV toggle market flag.
    fn toggle_glv_market_flag(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
        flag: GlvMarketFlag,
        enable: bool,
    ) -> TransactionBuilder<C>;

    /// Update GLV config.
    fn update_glv_config(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        params: UpdateGlvParams,
    ) -> TransactionBuilder<C>;

    /// Insert GLV market.
    fn insert_glv_market(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
        token_program_id: Option<&Pubkey>,
    ) -> TransactionBuilder<C>;

    /// Remove GLV market.
    fn remove_glv_market(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
        token_program_id: Option<&Pubkey>,
    ) -> TransactionBuilder<C>;

    /// Get glv token value.
    fn get_glv_token_value(
        &self,
        store: &Pubkey,
        oracle: &Pubkey,
        glv_token: &Pubkey,
        amount: u64,
    ) -> GetGlvTokenValueBuilder<'_, C>;
}

impl<C: Deref<Target = impl Signer> + Clone> GlvOps<C> for crate::Client<C> {
    fn initialize_glv(
        &self,
        store: &Pubkey,
        index: u16,
        market_tokens: impl IntoIterator<Item = Pubkey>,
    ) -> crate::Result<(TransactionBuilder<C>, Pubkey)> {
        let authority = self.payer();
        let glv_token = self.find_glv_token_address(store, index);
        let glv = self.find_glv_address(&glv_token);
        let market_token_program_id = anchor_spl::token::ID;

        let (accounts, length) = split_to_accounts(
            market_tokens,
            &glv,
            store,
            self.store_program_id(),
            &market_token_program_id,
            true,
        );

        let rpc = self
            .store_transaction()
            .anchor_accounts(accounts::InitializeGlv {
                authority,
                store: *store,
                glv_token,
                glv,
                system_program: system_program::ID,
                token_program: anchor_spl::token_2022::ID,
                market_token_program: market_token_program_id,
                associated_token_program: anchor_spl::associated_token::ID,
            })
            .anchor_args(args::InitializeGlv {
                index,
                length: length
                    .try_into()
                    .map_err(|_| crate::Error::custom("too many markets"))?,
            })
            .accounts(accounts);
        Ok((rpc, glv_token))
    }

    fn update_glv_market_config(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
        max_amount: Option<u64>,
        max_value: Option<u128>,
    ) -> TransactionBuilder<C> {
        let glv = self.find_glv_address(glv_token);
        self.store_transaction()
            .anchor_accounts(accounts::UpdateGlvMarketConfig {
                authority: self.payer(),
                store: *store,
                glv,
                market_token: *market_token,
            })
            .anchor_args(args::UpdateGlvMarketConfig {
                max_amount,
                max_value,
            })
    }

    fn toggle_glv_market_flag(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
        flag: GlvMarketFlag,
        enable: bool,
    ) -> TransactionBuilder<C> {
        let glv = self.find_glv_address(glv_token);
        self.store_transaction()
            .anchor_accounts(accounts::UpdateGlvMarketConfig {
                authority: self.payer(),
                store: *store,
                glv,
                market_token: *market_token,
            })
            .anchor_args(args::ToggleGlvMarketFlag {
                flag: flag.to_string(),
                enable,
            })
    }

    fn update_glv_config(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        params: UpdateGlvParams,
    ) -> TransactionBuilder<C> {
        let glv = self.find_glv_address(glv_token);
        self.store_transaction()
            .anchor_accounts(accounts::UpdateGlvConfig {
                authority: self.payer(),
                store: *store,
                glv,
            })
            .anchor_args(args::UpdateGlvConfig { params })
    }

    fn insert_glv_market(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
        token_program_id: Option<&Pubkey>,
    ) -> TransactionBuilder<C> {
        let token_program_id = token_program_id.unwrap_or(&anchor_spl::token::ID);
        let glv = self.find_glv_address(glv_token);
        let market = self.find_market_address(store, market_token);
        let vault =
            get_associated_token_address_with_program_id(&glv, market_token, token_program_id);
        self.store_transaction()
            .anchor_accounts(accounts::InsertGlvMarket {
                authority: self.payer(),
                store: *store,
                glv,
                market_token: *market_token,
                market,
                vault,
                system_program: system_program::ID,
                token_program: *token_program_id,
                associated_token_program: anchor_spl::associated_token::ID,
            })
            .anchor_args(args::InsertGlvMarket {})
    }

    fn remove_glv_market(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
        token_program_id: Option<&Pubkey>,
    ) -> TransactionBuilder<C> {
        let token_program_id = token_program_id.unwrap_or(&anchor_spl::token::ID);
        let glv = self.find_glv_address(glv_token);
        let vault =
            get_associated_token_address_with_program_id(&glv, market_token, token_program_id);
        let store_wallet = self.find_store_wallet_address(store);
        let store_wallet_ata = get_associated_token_address_with_program_id(
            &store_wallet,
            market_token,
            token_program_id,
        );
        self.store_transaction()
            .anchor_accounts(accounts::RemoveGlvMarket {
                authority: self.payer(),
                store: *store,
                store_wallet,
                glv,
                market_token: *market_token,
                vault,
                store_wallet_ata,
                token_program: *token_program_id,
                associated_token_program: anchor_spl::associated_token::ID,
                system_program: system_program::ID,
            })
            .anchor_args(args::RemoveGlvMarket {})
    }

    fn get_glv_token_value(
        &self,
        store: &Pubkey,
        oracle: &Pubkey,
        glv_token: &Pubkey,
        amount: u64,
    ) -> GetGlvTokenValueBuilder<'_, C> {
        GetGlvTokenValueBuilder::new(self, *store, *oracle, *glv_token, amount)
    }
}

pub(crate) fn split_to_accounts(
    market_tokens: impl IntoIterator<Item = Pubkey>,
    glv: &Pubkey,
    store: &Pubkey,
    store_program_id: &Pubkey,
    token_program_id: &Pubkey,
    with_vaults: bool,
) -> (Vec<AccountMeta>, usize) {
    let market_token_addresses = market_tokens.into_iter().collect::<BTreeSet<_>>();

    let markets = market_token_addresses.iter().map(|token| {
        AccountMeta::new_readonly(
            crate::pda::find_market_address(store, token, store_program_id).0,
            false,
        )
    });

    let market_tokens = market_token_addresses
        .iter()
        .map(|token| AccountMeta::new_readonly(*token, false));

    let length = market_token_addresses.len();

    let accounts = if with_vaults {
        let market_token_vaults = market_token_addresses.iter().map(|token| {
            let market_token_vault =
                get_associated_token_address_with_program_id(glv, token, token_program_id);

            AccountMeta::new(market_token_vault, false)
        });

        markets
            .chain(market_tokens)
            .chain(market_token_vaults)
            .collect::<Vec<_>>()
    } else {
        markets.chain(market_tokens).collect::<Vec<_>>()
    };

    (accounts, length)
}

/// Builder for `get_glv_token_value` instruction.
pub struct GetGlvTokenValueBuilder<'a, C> {
    client: &'a Client<C>,
    store: Pubkey,
    oracle: Pubkey,
    glv_token: Pubkey,
    amount: u64,
    maximize: bool,
    max_age: u32,
    emit_event: bool,
    feeds_parser: FeedsParser,
    hint: Option<GetGlvTokenValueHint>,
}

/// Hint for [`GetGlvTokenValueBuilder`].
#[derive(Debug, Clone)]
pub struct GetGlvTokenValueHint {
    /// Token map address.
    pub token_map: Pubkey,
    /// Market token mints in GLV.
    pub glv_market_tokens: BTreeSet<Pubkey>,
    /// Feeds.
    pub feeds: TokensWithFeed,
}

impl<C> GetGlvTokenValueBuilder<'_, C> {
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
    pub fn hint(&mut self, hint: Option<GetGlvTokenValueHint>) -> &mut Self {
        self.hint = hint;
        self
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> GetGlvTokenValueBuilder<'a, C> {
    fn new(
        client: &'a Client<C>,
        store: Pubkey,
        oracle: Pubkey,
        glv_token: Pubkey,
        amount: u64,
    ) -> Self {
        Self {
            client,
            store,
            oracle,
            glv_token,
            amount,
            maximize: false,
            max_age: DEFAULT_MAX_AGE,
            emit_event: true,
            feeds_parser: Default::default(),
            hint: None,
        }
    }

    /// Prepare hint.
    pub async fn prepare_hint(&mut self) -> crate::Result<GetGlvTokenValueHint> {
        if let Some(hint) = self.hint.as_ref() {
            return Ok(hint.clone());
        }

        let store = self.client.store(&self.store).await?;
        let token_map_address = store.token_map;
        let token_map = self.client.token_map(&token_map_address).await?;

        let glv = self.client.find_glv_address(&self.glv_token);
        let glv = self
            .client
            .account::<ZeroCopy<Glv>>(&glv)
            .await?
            .ok_or(crate::Error::NotFound)?
            .0;

        let mut collector = glv.tokens_collector(None::<&SwapActionParams>);
        for token in glv.market_tokens() {
            let market = self.client.market_by_token(&self.store, &token).await?;
            collector.insert_token(&market.meta.index_token_mint);
        }

        let glv_market_tokens = glv.market_tokens().collect();
        let hint = GetGlvTokenValueHint {
            token_map: token_map_address,
            glv_market_tokens,
            feeds: collector
                .to_feeds(&token_map)
                .map_err(crate::Error::custom)?,
        };
        self.hint = Some(hint.clone());
        Ok(hint)
    }

    async fn build_txn(&mut self) -> crate::Result<TransactionBuilder<'a, C>> {
        let token_program_id = anchor_spl::token::ID;
        let hint = self.prepare_hint().await?;
        let Self {
            client,
            store,
            oracle,
            glv_token,
            amount,
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
        let glv = client.find_glv_address(glv_token);
        let glv_accounts = split_to_accounts(
            hint.glv_market_tokens.iter().copied(),
            &glv,
            store,
            client.store_program_id(),
            &token_program_id,
            false,
        )
        .0;
        let txn = client
            .store_transaction()
            .anchor_args(args::GetGlvTokenValue {
                amount: *amount,
                maximize: *maximize,
                max_age: *max_age,
                emit_event: *emit_event,
            })
            .anchor_accounts(accounts::GetGlvTokenValue {
                authority,
                store: *store,
                token_map: hint.token_map,
                oracle: *oracle,
                glv,
                glv_token: *glv_token,
                event_authority: client.store_event_authority(),
                program: *client.store_program_id(),
            })
            .accounts(glv_accounts)
            .accounts(feeds);
        Ok(txn)
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> MakeBundleBuilder<'a, C>
    for GetGlvTokenValueBuilder<'a, C>
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
    for GetGlvTokenValueBuilder<'_, C>
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
