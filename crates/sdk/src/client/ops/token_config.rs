use std::ops::Deref;

use gmsol_programs::gmsol_store::client::{accounts, args};
use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use gmsol_utils::{oracle::PriceProviderKind, token_config::UpdateTokenConfigParams};
use solana_sdk::{pubkey::Pubkey, signer::Signer, system_program};

/// Operations for token config.
pub trait TokenConfigOps<C> {
    /// Initialize a  `TokenMap` account.
    fn initialize_token_map<'a>(
        &'a self,
        store: &Pubkey,
        token_map: &'a dyn Signer,
    ) -> (TransactionBuilder<'a, C>, Pubkey);

    /// Insert or update config for the given token.
    #[allow(clippy::too_many_arguments)]
    fn insert_token_config(
        &self,
        store: &Pubkey,
        token_map: &Pubkey,
        name: &str,
        token: &Pubkey,
        builder: UpdateTokenConfigParams,
        enable: bool,
        new: bool,
    ) -> TransactionBuilder<C>;

    /// Insert or update config the given synthetic token.
    #[allow(clippy::too_many_arguments)]
    fn insert_synthetic_token_config(
        &self,
        store: &Pubkey,
        token_map: &Pubkey,
        name: &str,
        token: &Pubkey,
        decimals: u8,
        builder: UpdateTokenConfigParams,
        enable: bool,
        new: bool,
    ) -> TransactionBuilder<C>;

    /// Toggle token config.
    fn toggle_token_config(
        &self,
        store: &Pubkey,
        token_map: &Pubkey,
        token: &Pubkey,
        enable: bool,
    ) -> TransactionBuilder<C>;

    /// Toggle token price adjustment.
    fn toggle_token_price_adjustment(
        &self,
        store: &Pubkey,
        token_map: &Pubkey,
        token: &Pubkey,
        enable: bool,
    ) -> TransactionBuilder<C>;

    /// Set expected provider.
    fn set_expected_provider(
        &self,
        store: &Pubkey,
        token_map: &Pubkey,
        token: &Pubkey,
        provider: PriceProviderKind,
    ) -> TransactionBuilder<C>;

    /// Update feed config.
    fn update_feed_config(
        &self,
        store: &Pubkey,
        token_map: &Pubkey,
        token: &Pubkey,
        provider: PriceProviderKind,
        update: UpdateFeedConfig,
    ) -> TransactionBuilder<C>;
}

impl<C: Deref<Target = impl Signer> + Clone> TokenConfigOps<C> for crate::Client<C> {
    fn initialize_token_map<'a>(
        &'a self,
        store: &Pubkey,
        token_map: &'a dyn Signer,
    ) -> (TransactionBuilder<'a, C>, Pubkey) {
        let builder = self
            .store_transaction()
            .anchor_accounts(accounts::InitializeTokenMap {
                payer: self.payer(),
                store: *store,
                token_map: token_map.pubkey(),
                system_program: system_program::ID,
            })
            .anchor_args(args::InitializeTokenMap {})
            .signer(token_map);
        (builder, token_map.pubkey())
    }

    fn insert_token_config(
        &self,
        store: &Pubkey,
        token_map: &Pubkey,
        name: &str,
        token: &Pubkey,
        builder: UpdateTokenConfigParams,
        enable: bool,
        new: bool,
    ) -> TransactionBuilder<C> {
        let authority = self.payer();
        self.store_transaction()
            .anchor_accounts(accounts::PushToTokenMap {
                authority,
                store: *store,
                token_map: *token_map,
                token: *token,
                system_program: system_program::ID,
            })
            .anchor_args(args::PushToTokenMap {
                name: name.to_owned(),
                builder: builder.into(),
                enable,
                new,
            })
    }

    fn insert_synthetic_token_config(
        &self,
        store: &Pubkey,
        token_map: &Pubkey,
        name: &str,
        token: &Pubkey,
        decimals: u8,
        builder: UpdateTokenConfigParams,
        enable: bool,
        new: bool,
    ) -> TransactionBuilder<C> {
        let authority = self.payer();
        self.store_transaction()
            .anchor_accounts(accounts::PushToTokenMapSynthetic {
                authority,
                store: *store,
                token_map: *token_map,
                system_program: system_program::ID,
            })
            .anchor_args(args::PushToTokenMapSynthetic {
                name: name.to_owned(),
                token: *token,
                token_decimals: decimals,
                builder: builder.into(),
                enable,
                new,
            })
    }

    fn toggle_token_config(
        &self,
        store: &Pubkey,
        token_map: &Pubkey,
        token: &Pubkey,
        enable: bool,
    ) -> TransactionBuilder<C> {
        let authority = self.payer();
        self.store_transaction()
            .anchor_accounts(accounts::ToggleTokenConfig {
                authority,
                store: *store,
                token_map: *token_map,
            })
            .anchor_args(args::ToggleTokenConfig {
                token: *token,
                enable,
            })
    }

    fn toggle_token_price_adjustment(
        &self,
        store: &Pubkey,
        token_map: &Pubkey,
        token: &Pubkey,
        enable: bool,
    ) -> TransactionBuilder<C> {
        let authority = self.payer();
        self.store_transaction()
            .anchor_accounts(accounts::ToggleTokenPriceAdjustment {
                authority,
                store: *store,
                token_map: *token_map,
            })
            .anchor_args(args::ToggleTokenPriceAdjustment {
                token: *token,
                enable,
            })
    }

    fn set_expected_provider(
        &self,
        store: &Pubkey,
        token_map: &Pubkey,
        token: &Pubkey,
        provider: PriceProviderKind,
    ) -> TransactionBuilder<C> {
        let authority = self.payer();
        self.store_transaction()
            .anchor_accounts(accounts::SetExpectedProvider {
                authority,
                store: *store,
                token_map: *token_map,
            })
            .anchor_args(args::SetExpectedProvider {
                token: *token,
                provider: provider as u8,
            })
    }

    fn update_feed_config(
        &self,
        store: &Pubkey,
        token_map: &Pubkey,
        token: &Pubkey,
        provider: PriceProviderKind,
        update: UpdateFeedConfig,
    ) -> TransactionBuilder<C> {
        let authority = self.payer();
        self.store_transaction()
            .anchor_accounts(accounts::SetFeedConfigV2 {
                authority,
                store: *store,
                token_map: *token_map,
            })
            .anchor_args(args::SetFeedConfigV2 {
                token: *token,
                provider: provider.into(),
                feed: update.feed_id,
                timestamp_adjustment: update.timestamp_adjustment,
                max_deviation_factor: update.max_deviation_factor,
            })
    }
}

/// Contains updated parameters for the feed config.
#[derive(typed_builder::TypedBuilder)]
pub struct UpdateFeedConfig {
    /// Feed id.
    #[builder(default)]
    pub feed_id: Option<Pubkey>,
    /// Timestamp adjustment.
    #[builder(default)]
    pub timestamp_adjustment: Option<u32>,
    /// Max deviation factor.
    #[builder(default)]
    pub max_deviation_factor: Option<u128>,
}
