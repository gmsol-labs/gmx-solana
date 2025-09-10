//! Generic RPC client implementation.

use std::future::Future;

use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;
use solana_account_decoder_client_types::{UiAccount, UiAccountEncoding};
use solana_rpc_client_api::{
    client_error::Error as ClientError,
    config, filter,
    request::{RpcError, RpcRequest},
    response::{self, Response},
};
use solana_sdk::{account::Account, commitment_config::CommitmentConfig, pubkey::Pubkey};

use crate::utils::WithSlot;

pub mod generic;

/// A RPC client.
pub trait RpcClient {
    /// Returns the configured default commitment level.
    fn commitment(&self) -> CommitmentConfig;

    /// Send an [`RpcRequest`] with parameters.
    fn send<T>(
        &self,
        request: RpcRequest,
        params: impl Serialize,
    ) -> impl Future<Output = crate::Result<T>>
    where
        T: DeserializeOwned;
}

/// A trait that extends [`RpcClient`] with RPC methods.
pub trait RpcClientExt: RpcClient {
    /// Get account info for `pubkey`, including the context slot.
    /// Returns `None` if the account does not exist.
    fn get_optional_account_with_slot(
        &self,
        address: &Pubkey,
        mut config: config::RpcAccountInfoConfig,
    ) -> impl Future<Output = crate::Result<WithSlot<Option<Account>>>> {
        config.encoding = Some(config.encoding.unwrap_or(UiAccountEncoding::Base64));
        let commitment = config.commitment.unwrap_or_else(|| self.commitment());
        config.commitment = Some(commitment);
        tracing::trace!(%address, ?config, "fetching account with config");
        async move {
            let res = self
                .send::<Response<Option<UiAccount>>>(
                    RpcRequest::GetAccountInfo,
                    json!([address.to_string(), config]),
                )
                .await
                .map_err(crate::Error::custom)?;
            Ok(WithSlot::new(res.context.slot, res.value)
                .map(|value| value.and_then(|a| a.decode())))
        }
    }

    /// Get account for `pubkey` and decode with [`AccountDeserialize`](anchor_lang::AccountDeserialize), including the context slot.
    /// Returns `None` if the account does not exist.
    #[cfg(anchor_lang)]
    fn get_optional_anchor_account_with_slot<T: anchor_lang::AccountDeserialize>(
        &self,
        address: &Pubkey,
        config: config::RpcAccountInfoConfig,
    ) -> impl Future<Output = crate::Result<WithSlot<Option<T>>>> {
        async move {
            let res = self.get_optional_account_with_slot(address, config).await?;
            Ok(res
                .map(|a| {
                    a.map(|account| T::try_deserialize(&mut (&account.data as &[u8])))
                        .transpose()
                })
                .transpose()?)
        }
    }

    /// Get account for `pubkey` and decode with [`AccountDeserialize`](anchor_lang::AccountDeserialize), including the context slot.
    /// Returns `Err` if the account does not exist.
    #[cfg(anchor_lang)]
    fn get_anchor_account_with_slot<T: anchor_lang::AccountDeserialize>(
        &self,
        address: &Pubkey,
        config: config::RpcAccountInfoConfig,
    ) -> impl Future<Output = crate::Result<WithSlot<T>>> {
        async move {
            let res = self
                .get_optional_anchor_account_with_slot(address, config)
                .await?;
            res.map(|a| a.ok_or_else(|| crate::Error::AccountNotFound(*address)))
                .transpose()
        }
    }

    /// Get account for `pubkey` and decode with [`AccountDeserialize`](anchor_lang::AccountDeserialize).
    /// Returns `Err` if the account does not exist.
    #[cfg(anchor_lang)]
    fn get_anchor_account<T: anchor_lang::AccountDeserialize>(
        &self,
        address: &Pubkey,
        config: config::RpcAccountInfoConfig,
    ) -> impl Future<Output = crate::Result<T>> {
        async move {
            let res = self
                .get_optional_anchor_account_with_slot(address, config)
                .await?;
            res.map(|a| a.ok_or_else(|| crate::Error::AccountNotFound(*address)))
                .transpose()
                .map(|w| w.into_value())
        }
    }

    /// Get program accounts with slot.
    fn get_program_accounts_with_slot(
        &self,
        program: &Pubkey,
        mut config: RpcProgramAccountsConfig,
    ) -> impl Future<Output = crate::Result<WithSlot<Vec<(Pubkey, Account)>>>> {
        let commitment = config
            .account_config
            .commitment
            .unwrap_or_else(|| self.commitment());
        config.account_config.commitment = Some(commitment);
        let config = config::RpcProgramAccountsConfig {
            filters: config.filters,
            account_config: config.account_config,
            with_context: Some(true),
            sort_results: None,
        };
        tracing::trace!(%program, ?config, "fetching program accounts");
        async move {
            let res = self
                .send::<Response<Vec<response::RpcKeyedAccount>>>(
                    RpcRequest::GetProgramAccounts,
                    json!([program.to_string(), config]),
                )
                .await
                .map_err(crate::Error::custom)?;
            WithSlot::new(res.context.slot, res.value)
                .map(|accounts| parse_keyed_accounts(accounts, RpcRequest::GetProgramAccounts))
                .transpose()
        }
    }

    /// Get programs accounts and decode with [`AccountDeserialize`](anchor_lang::AccountDeserialize).
    #[cfg(anchor_lang)]
    fn get_anchor_accounts_with_slot<T>(
        &self,
        program: &Pubkey,
        filters: impl IntoIterator<Item = filter::RpcFilterType>,
        config: AnchorAccountsConfig,
    ) -> impl Future<Output = crate::Result<WithSlot<impl Iterator<Item = crate::Result<(Pubkey, T)>>>>>
    where
        T: anchor_lang::AccountDeserialize + anchor_lang::Discriminator,
    {
        let AnchorAccountsConfig {
            skip_account_type_filter,
            commitment,
            min_context_slot,
        } = config;
        let filters = (!skip_account_type_filter)
            .then(|| {
                filter::RpcFilterType::Memcmp(filter::Memcmp::new_base58_encoded(
                    0,
                    T::DISCRIMINATOR,
                ))
            })
            .into_iter()
            .chain(filters)
            .collect::<Vec<_>>();
        let config = RpcProgramAccountsConfig {
            filters: (!filters.is_empty()).then_some(filters),
            account_config: config::RpcAccountInfoConfig {
                encoding: Some(UiAccountEncoding::Base64),
                commitment,
                min_context_slot,
                ..Default::default()
            },
        };
        async move {
            let res = self.get_program_accounts_with_slot(program, config).await?;
            Ok(res.map(|accounts| {
                accounts.into_iter().map(|(key, account)| {
                    Ok((key, T::try_deserialize(&mut (&account.data as &[u8]))?))
                })
            }))
        }
    }
}

impl<C: RpcClient + ?Sized> RpcClientExt for C {}

/// Configuration for program accounts.
#[derive(Debug, Default)]
pub struct RpcProgramAccountsConfig {
    /// Filters.
    pub filters: Option<Vec<filter::RpcFilterType>>,
    /// Account Config.
    pub account_config: config::RpcAccountInfoConfig,
}

/// Configuagtion for anchor accounts.
#[cfg(anchor_lang)]
#[derive(Debug, Default)]
pub struct AnchorAccountsConfig {
    /// Whether to skip the account type filter.
    pub skip_account_type_filter: bool,
    /// Commitment.
    pub commitment: Option<CommitmentConfig>,
    /// Min context slot.
    pub min_context_slot: Option<u64>,
}

fn parse_keyed_accounts(
    accounts: Vec<response::RpcKeyedAccount>,
    request: RpcRequest,
) -> crate::Result<Vec<(Pubkey, Account)>> {
    let mut pubkey_accounts: Vec<(Pubkey, Account)> = Vec::with_capacity(accounts.len());
    for response::RpcKeyedAccount { pubkey, account } in accounts.into_iter() {
        let pubkey = pubkey
            .parse()
            .map_err(|_| {
                ClientError::new_with_request(
                    RpcError::ParseError("Pubkey".to_string()).into(),
                    request,
                )
            })
            .map_err(crate::Error::custom)?;
        pubkey_accounts.push((
            pubkey,
            account
                .decode()
                .ok_or_else(|| {
                    ClientError::new_with_request(
                        RpcError::ParseError("Account from rpc".to_string()).into(),
                        request,
                    )
                })
                .map_err(crate::Error::custom)?,
        ));
    }
    Ok(pubkey_accounts)
}

#[cfg(client)]
impl RpcClient for solana_client::nonblocking::rpc_client::RpcClient {
    fn commitment(&self) -> CommitmentConfig {
        self.commitment()
    }

    async fn send<T>(&self, request: RpcRequest, params: impl Serialize) -> crate::Result<T>
    where
        T: DeserializeOwned,
    {
        self.send(request, serde_json::to_value(params)?)
            .await
            .map_err(crate::Error::custom)
    }
}

/// Types that can be created from an [`RpcClient`] with the given builder.
pub trait FromRpcClientWith<B: ?Sized>: Sized {
    /// Create from [`RpcClient`].
    fn from_rpc_client_with<'a>(
        builder: &'a B,
        client: &'a impl RpcClient,
    ) -> impl Future<Output = crate::Result<Self>> + 'a;
}

impl<B> FromRpcClientWith<B> for () {
    async fn from_rpc_client_with<'a>(
        _builder: &'a B,
        _client: &'a impl RpcClient,
    ) -> crate::Result<Self> {
        Ok(())
    }
}
