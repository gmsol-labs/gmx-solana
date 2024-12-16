use std::{future::Future, ops::Deref};

use anchor_client::{
    anchor_lang::system_program,
    solana_sdk::{pubkey::Pubkey, signer::Signer},
};
use gmsol_store::{
    accounts, instruction,
    states::{PriceProviderKind, TokenConfigBuilder},
};

use crate::utils::{view, RpcBuilder};

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
    ) -> (RpcBuilder<'a, C>, Pubkey);

    /// Insert or update config for the given token.
    #[allow(clippy::too_many_arguments)]
    fn insert_token_config(
        &self,
        store: &Pubkey,
        token_map: &Pubkey,
        name: &str,
        token: &Pubkey,
        builder: TokenConfigBuilder,
        enable: bool,
        new: bool,
    ) -> RpcBuilder<C>;

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
        builder: TokenConfigBuilder,
        enable: bool,
        new: bool,
    ) -> RpcBuilder<C>;

    /// Toggle token config.
    fn toggle_token_config(
        &self,
        store: &Pubkey,
        token_map: &Pubkey,
        token: &Pubkey,
        enable: bool,
    ) -> RpcBuilder<C>;

    /// Set expected provider.
    fn set_expected_provider(
        &self,
        store: &Pubkey,
        token_map: &Pubkey,
        token: &Pubkey,
        provider: PriceProviderKind,
    ) -> RpcBuilder<C>;

    /// Get the name for the given token.
    fn token_name(&self, token_map: &Pubkey, token: &Pubkey) -> RpcBuilder<C>;

    /// Get the token decimals for the given token.
    fn token_decimals(&self, token_map: &Pubkey, token: &Pubkey) -> RpcBuilder<C>;

    /// Get the price precision for the given token.
    fn token_precision(&self, token_map: &Pubkey, token: &Pubkey) -> RpcBuilder<C>;

    /// Check if the config of the given token is enbaled.
    fn is_token_config_enabled(&self, token_map: &Pubkey, token: &Pubkey) -> RpcBuilder<C>;

    /// Get expected provider for the given token.
    fn token_expected_provider(&self, token_map: &Pubkey, token: &Pubkey) -> RpcBuilder<C>;

    /// Get feed address of the provider of the given token.
    fn token_feed(
        &self,
        token_map: &Pubkey,
        token: &Pubkey,
        provider: PriceProviderKind,
    ) -> RpcBuilder<C>;

    /// Get timestamp adjustment of the given token and provider.
    fn token_timestamp_adjustment(
        &self,
        token_map: &Pubkey,
        token: &Pubkey,
        provider: PriceProviderKind,
    ) -> RpcBuilder<C>;

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
    ) -> (RpcBuilder<'a, C>, Pubkey) {
        let builder = self
            .store_rpc()
            .accounts(accounts::InitializeTokenMap {
                payer: self.payer(),
                store: *store,
                token_map: token_map.pubkey(),
                system_program: system_program::ID,
            })
            .args(instruction::InitializeTokenMap {})
            .signer(token_map);
        (builder, token_map.pubkey())
    }

    fn insert_token_config(
        &self,
        store: &Pubkey,
        token_map: &Pubkey,
        name: &str,
        token: &Pubkey,
        builder: TokenConfigBuilder,
        enable: bool,
        new: bool,
    ) -> RpcBuilder<C> {
        let authority = self.payer();
        self.store_rpc()
            .accounts(accounts::PushToTokenMap {
                authority,
                store: *store,
                token_map: *token_map,
                token: *token,
                system_program: system_program::ID,
            })
            .args(instruction::PushToTokenMap {
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
        builder: TokenConfigBuilder,
        enable: bool,
        new: bool,
    ) -> RpcBuilder<C> {
        let authority = self.payer();
        self.store_rpc()
            .accounts(accounts::PushToTokenMapSynthetic {
                authority,
                store: *store,
                token_map: *token_map,
                system_program: system_program::ID,
            })
            .args(instruction::PushToTokenMapSynthetic {
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
    ) -> RpcBuilder<C> {
        let authority = self.payer();
        self.store_rpc()
            .accounts(accounts::ToggleTokenConfig {
                authority,
                store: *store,
                token_map: *token_map,
            })
            .args(instruction::ToggleTokenConfig {
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
    ) -> RpcBuilder<C> {
        let authority = self.payer();
        self.store_rpc()
            .accounts(accounts::SetExpectedProvider {
                authority,
                store: *store,
                token_map: *token_map,
            })
            .args(instruction::SetExpectedProvider {
                token: *token,
                provider: provider as u8,
            })
    }

    fn token_name(&self, token_map: &Pubkey, token: &Pubkey) -> RpcBuilder<C> {
        self.store_rpc()
            .args(instruction::TokenName { token: *token })
            .accounts(accounts::ReadTokenMap {
                token_map: *token_map,
            })
    }

    fn token_decimals(&self, token_map: &Pubkey, token: &Pubkey) -> RpcBuilder<C> {
        self.store_rpc()
            .args(instruction::TokenDecimals { token: *token })
            .accounts(accounts::ReadTokenMap {
                token_map: *token_map,
            })
    }

    fn token_precision(&self, token_map: &Pubkey, token: &Pubkey) -> RpcBuilder<C> {
        self.store_rpc()
            .args(instruction::TokenPrecision { token: *token })
            .accounts(accounts::ReadTokenMap {
                token_map: *token_map,
            })
    }

    fn is_token_config_enabled(&self, token_map: &Pubkey, token: &Pubkey) -> RpcBuilder<C> {
        self.store_rpc()
            .args(instruction::IsTokenConfigEnabled { token: *token })
            .accounts(accounts::ReadTokenMap {
                token_map: *token_map,
            })
    }

    fn token_expected_provider(&self, token_map: &Pubkey, token: &Pubkey) -> RpcBuilder<C> {
        self.store_rpc()
            .args(instruction::TokenExpectedProvider { token: *token })
            .accounts(accounts::ReadTokenMap {
                token_map: *token_map,
            })
    }

    fn token_feed(
        &self,
        token_map: &Pubkey,
        token: &Pubkey,
        provider: PriceProviderKind,
    ) -> RpcBuilder<C> {
        self.store_rpc()
            .args(instruction::TokenFeed {
                token: *token,
                provider: provider as u8,
            })
            .accounts(accounts::ReadTokenMap {
                token_map: *token_map,
            })
    }

    fn token_timestamp_adjustment(
        &self,
        token_map: &Pubkey,
        token: &Pubkey,
        provider: PriceProviderKind,
    ) -> RpcBuilder<C> {
        self.store_rpc()
            .args(instruction::TokenTimestampAdjustment {
                token: *token,
                provider: provider as u8,
            })
            .accounts(accounts::ReadTokenMap {
                token_map: *token_map,
            })
    }

    async fn token_config(&self, token_map: &Pubkey, token: &Pubkey) -> crate::Result<TokenConfig> {
        let client = self.store_program().solana_rpc();
        let name = self
            .token_name(token_map, token)
            .into_anchor_request()
            .signed_transaction()
            .await?;
        let token_decimals = self
            .token_decimals(token_map, token)
            .into_anchor_request()
            .signed_transaction()
            .await?;
        let precision = self
            .token_precision(token_map, token)
            .into_anchor_request()
            .signed_transaction()
            .await?;
        let expected_provider = self
            .token_expected_provider(token_map, token)
            .into_anchor_request()
            .signed_transaction()
            .await?;
        let is_enabled = self
            .is_token_config_enabled(token_map, token)
            .into_anchor_request()
            .signed_transaction()
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
