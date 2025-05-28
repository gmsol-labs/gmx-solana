use std::{future::Future, ops::Deref};

use anchor_client::{
    anchor_lang::system_program,
    solana_sdk::{pubkey::Pubkey, signer::Signer},
};
use gmsol_solana_utils::transaction_builder::{default_before_sign, TransactionBuilder};
use gmsol_store::{
    accounts, instruction,
    states::{PriceProviderKind, UpdateTokenConfigParams},
};

use crate::utils::view;

/// Token Config.
#[derive(Debug)]
pub struct TokenConfig {
    name: String,
    is_enabled: bool,
    decimals: u8,
    precision: u8,
    expected_provider: PriceProviderKind,
}

impl TokenConfig {
    /// Get name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get token decimals.
    pub fn decimals(&self) -> u8 {
        self.decimals
    }

    /// Get price precision.
    pub fn precision(&self) -> u8 {
        self.precision
    }

    /// Get expected price provider.
    pub fn expected_provider(&self) -> PriceProviderKind {
        self.expected_provider
    }

    /// Get is enabled.
    pub fn is_enabled(&self) -> bool {
        self.is_enabled
    }
}

/// Token config management for GMSOL.
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
    // FIXME: reduce the number of args.
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

    /// Set expected provider.
    fn set_expected_provider(
        &self,
        store: &Pubkey,
        token_map: &Pubkey,
        token: &Pubkey,
        provider: PriceProviderKind,
    ) -> TransactionBuilder<C>;

    /// Get the name for the given token.
    fn token_name(&self, token_map: &Pubkey, token: &Pubkey) -> TransactionBuilder<C>;

    /// Get the token decimals for the given token.
    fn token_decimals(&self, token_map: &Pubkey, token: &Pubkey) -> TransactionBuilder<C>;

    /// Get the price precision for the given token.
    fn token_precision(&self, token_map: &Pubkey, token: &Pubkey) -> TransactionBuilder<C>;

    /// Check if the config of the given token is enbaled.
    fn is_token_config_enabled(&self, token_map: &Pubkey, token: &Pubkey) -> TransactionBuilder<C>;

    /// Get expected provider for the given token.
    fn token_expected_provider(&self, token_map: &Pubkey, token: &Pubkey) -> TransactionBuilder<C>;

    /// Get feed address of the provider of the given token.
    fn token_feed(
        &self,
        token_map: &Pubkey,
        token: &Pubkey,
        provider: PriceProviderKind,
    ) -> TransactionBuilder<C>;

    /// Get timestamp adjustment of the given token and provider.
    fn token_timestamp_adjustment(
        &self,
        token_map: &Pubkey,
        token: &Pubkey,
        provider: PriceProviderKind,
    ) -> TransactionBuilder<C>;

    /// Get basic token config.
    fn token_config(
        &self,
        token_map: &Pubkey,
        token: &Pubkey,
    ) -> impl Future<Output = crate::Result<TokenConfig>>;
}

impl<C, S> TokenConfigOps<C> for crate::Client<C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
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
            .anchor_args(instruction::InitializeTokenMap {})
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
            .anchor_args(instruction::PushToTokenMap {
                name: name.to_owned(),
                builder,
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
            .anchor_args(instruction::PushToTokenMapSynthetic {
                name: name.to_owned(),
                token: *token,
                token_decimals: decimals,
                builder,
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
            .anchor_args(instruction::ToggleTokenConfig {
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
            .anchor_args(instruction::SetExpectedProvider {
                token: *token,
                provider: provider as u8,
            })
    }

    fn token_name(&self, token_map: &Pubkey, token: &Pubkey) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(instruction::TokenName { token: *token })
            .anchor_accounts(accounts::ReadTokenMap {
                token_map: *token_map,
            })
    }

    fn token_decimals(&self, token_map: &Pubkey, token: &Pubkey) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(instruction::TokenDecimals { token: *token })
            .anchor_accounts(accounts::ReadTokenMap {
                token_map: *token_map,
            })
    }

    fn token_precision(&self, token_map: &Pubkey, token: &Pubkey) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(instruction::TokenPrecision { token: *token })
            .anchor_accounts(accounts::ReadTokenMap {
                token_map: *token_map,
            })
    }

    fn is_token_config_enabled(&self, token_map: &Pubkey, token: &Pubkey) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(instruction::IsTokenConfigEnabled { token: *token })
            .anchor_accounts(accounts::ReadTokenMap {
                token_map: *token_map,
            })
    }

    fn token_expected_provider(&self, token_map: &Pubkey, token: &Pubkey) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(instruction::TokenExpectedProvider { token: *token })
            .anchor_accounts(accounts::ReadTokenMap {
                token_map: *token_map,
            })
    }

    fn token_feed(
        &self,
        token_map: &Pubkey,
        token: &Pubkey,
        provider: PriceProviderKind,
    ) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(instruction::TokenFeed {
                token: *token,
                provider: provider as u8,
            })
            .anchor_accounts(accounts::ReadTokenMap {
                token_map: *token_map,
            })
    }

    fn token_timestamp_adjustment(
        &self,
        token_map: &Pubkey,
        token: &Pubkey,
        provider: PriceProviderKind,
    ) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(instruction::TokenTimestampAdjustment {
                token: *token,
                provider: provider as u8,
            })
            .anchor_accounts(accounts::ReadTokenMap {
                token_map: *token_map,
            })
    }

    async fn token_config(&self, token_map: &Pubkey, token: &Pubkey) -> crate::Result<TokenConfig> {
        let client = self.store_program().rpc();
        let name = self
            .token_name(token_map, token)
            .signed_transaction_with_options(true, None, default_before_sign)
            .await?;
        let token_decimals = self
            .token_decimals(token_map, token)
            .signed_transaction_with_options(true, None, default_before_sign)
            .await?;
        let precision = self
            .token_precision(token_map, token)
            .signed_transaction_with_options(true, None, default_before_sign)
            .await?;
        let expected_provider = self
            .token_expected_provider(token_map, token)
            .signed_transaction_with_options(true, None, default_before_sign)
            .await?;
        let is_enabled = self
            .is_token_config_enabled(token_map, token)
            .signed_transaction_with_options(true, None, default_before_sign)
            .await?;

        Ok(TokenConfig {
            name: view(&client, &name).await?,
            decimals: view(&client, &token_decimals).await?,
            precision: view(&client, &precision).await?,
            expected_provider: view::<u8>(&client, &expected_provider)
                .await?
                .try_into()
                .map_err(crate::Error::unknown)?,
            is_enabled: view(&client, &is_enabled).await?,
        })
    }
}
