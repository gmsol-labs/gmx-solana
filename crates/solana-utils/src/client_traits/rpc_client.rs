//! Generic RPC client implementation.

use std::time::Duration;

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

use super::RpcSender;

/// RPC client configuration.
#[derive(Debug, Default, Clone)]
pub struct RpcClientConfig {
    /// Commitment level for RPC queries. See [`CommitmentConfig`].
    pub commitment_config: CommitmentConfig,
    /// Initial timeout for transaction confirmation; `None` uses the client default.
    pub confirm_transaction_initial_timeout: Option<Duration>,
}

/// Generic RPC client implementation.
#[derive(Debug, Clone)]
pub struct RpcClient<S> {
    sender: S,
    config: RpcClientConfig,
}

impl<S: RpcSender> RpcClient<S> {
    /// Returns the configured default commitment level.
    pub fn commitment(&self) -> CommitmentConfig {
        self.config.commitment_config
    }

    /// Send an [`RpcRequest`] with parameters.
    pub async fn send<T>(&self, request: RpcRequest, params: impl Serialize) -> crate::Result<T>
    where
        T: DeserializeOwned,
    {
        let params = serde_json::to_value(params)?;
        if !params.is_array() && !params.is_null() {
            return Err(crate::Error::custom(
                "`params` is neither an array nor null",
            ));
        }

        let response = self.sender.send(request, params).await?;
        Ok(serde_json::from_value(response)?)
    }

    /// Get account info for `pubkey`, including the context slot.
    pub async fn get_account_with_slot(
        &self,
        address: &Pubkey,
        mut config: config::RpcAccountInfoConfig,
    ) -> crate::Result<WithSlot<Option<Account>>> {
        config.encoding = Some(config.encoding.unwrap_or(UiAccountEncoding::Base64));
        let commitment = config.commitment.unwrap_or_else(|| self.commitment());
        config.commitment = Some(commitment);
        tracing::trace!(%address, ?config, "fetching account with config");
        let res = self
            .send::<Response<Option<UiAccount>>>(
                RpcRequest::GetAccountInfo,
                json!([address.to_string(), config]),
            )
            .await
            .map_err(crate::Error::custom)?;
        Ok(WithSlot::new(res.context.slot, res.value).map(|value| value.and_then(|a| a.decode())))
    }

    /// Get account for `pubkey` and decode with [`AccountDeserialize`](anchor_lang::AccountDeserialize).
    #[cfg(feature = "anchor-lang")]
    pub async fn get_anchor_account_with_slot<T: anchor_lang::AccountDeserialize>(
        &self,
        address: &Pubkey,
        config: config::RpcAccountInfoConfig,
    ) -> crate::Result<WithSlot<Option<T>>> {
        let res = self.get_account_with_slot(address, config).await?;
        Ok(res
            .map(|a| {
                a.map(|account| T::try_deserialize(&mut (&account.data as &[u8])))
                    .transpose()
            })
            .transpose()?)
    }

    /// Get program accounts with slot.
    pub async fn get_program_accounts_with_slot(
        &self,
        program: &Pubkey,
        mut config: RpcProgramAccountsConfig,
    ) -> crate::Result<WithSlot<Vec<(Pubkey, Account)>>> {
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

    /// Get programs accounts and decode with [`AccountDeserialize`](anchor_lang::AccountDeserialize).
    #[cfg(feature = "anchor-lang")]
    pub async fn get_anchor_accounts_with_slot<T>(
        &self,
        program: &Pubkey,
        filters: impl IntoIterator<Item = filter::RpcFilterType>,
        config: AnchorAccountsConfig,
    ) -> crate::Result<WithSlot<impl Iterator<Item = crate::Result<(Pubkey, T)>>>>
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
        let res = self.get_program_accounts_with_slot(program, config).await?;
        Ok(res.map(|accounts| {
            accounts
                .into_iter()
                .map(|(key, account)| Ok((key, T::try_deserialize(&mut (&account.data as &[u8]))?)))
        }))
    }
}

/// Configuration for program accounts.
#[derive(Debug, Default)]
pub struct RpcProgramAccountsConfig {
    /// Filters.
    pub filters: Option<Vec<filter::RpcFilterType>>,
    /// Account Config.
    pub account_config: config::RpcAccountInfoConfig,
}

/// Configuagtion for anchor accounts.
#[cfg(feature = "anchor-lang")]
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
