use std::ops::Deref;

use gmsol_programs::anchor_lang::{AccountDeserialize, Discriminator};
use gmsol_solana_utils::{program::Program, utils::WithSlot};
use serde_json::json;
use solana_account_decoder::{UiAccount, UiAccountEncoding};
use solana_client::{
    client_error::ClientError,
    nonblocking::rpc_client::RpcClient,
    rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig, RpcTokenAccountsFilter},
    rpc_filter::{Memcmp, RpcFilterType},
    rpc_request::{RpcError, RpcRequest, TokenAccountsFilter},
    rpc_response::{Response, RpcKeyedAccount},
};
use solana_sdk::{
    account::Account, commitment_config::CommitmentConfig, pubkey::Pubkey, signer::Signer,
};

/// Program Accounts Config.
#[derive(Debug, Default)]
pub struct ProgramAccountsConfigForRpc {
    /// Filters.
    pub filters: Option<Vec<RpcFilterType>>,
    /// Account Config.
    pub account_config: RpcAccountInfoConfig,
}

/// Get program accounts with context.
///
/// # Note
/// This function only supports RPC Node versions `>= 1.17`.
pub async fn get_program_accounts_with_context(
    client: &RpcClient,
    program: &Pubkey,
    mut config: ProgramAccountsConfigForRpc,
) -> crate::Result<WithSlot<Vec<(Pubkey, Account)>>> {
    let commitment = config
        .account_config
        .commitment
        .unwrap_or_else(|| client.commitment());
    config.account_config.commitment = Some(commitment);
    let config = RpcProgramAccountsConfig {
        filters: config.filters,
        account_config: config.account_config,
        with_context: Some(true),
        sort_results: None,
    };
    tracing::debug!(%program, ?config, "fetching program accounts");
    let res = client
        .send::<Response<Vec<RpcKeyedAccount>>>(
            RpcRequest::GetProgramAccounts,
            json!([program.to_string(), config]),
        )
        .await
        .map_err(crate::Error::custom)?;
    WithSlot::new(res.context.slot, res.value)
        .map(|accounts| parse_keyed_accounts(accounts, RpcRequest::GetProgramAccounts))
        .transpose()
}

/// Get account with context.
///
/// The value inside the context will be `None` if the account does not exist.
pub async fn get_account_with_context(
    client: &RpcClient,
    address: &Pubkey,
    mut config: RpcAccountInfoConfig,
) -> crate::Result<WithSlot<Option<Account>>> {
    config.encoding = Some(config.encoding.unwrap_or(UiAccountEncoding::Base64));
    let commitment = config.commitment.unwrap_or_else(|| client.commitment());
    config.commitment = Some(commitment);
    tracing::debug!(%address, ?config, "fetching account");
    let res = client
        .send::<Response<Option<UiAccount>>>(
            RpcRequest::GetAccountInfo,
            json!([address.to_string(), config]),
        )
        .await
        .map_err(crate::Error::custom)?;
    Ok(WithSlot::new(res.context.slot, res.value).map(|value| value.and_then(|a| a.decode())))
}

/// Program Accounts Config.
#[derive(Debug, Default)]
pub struct ProgramAccountsConfig {
    /// Whether to skip the account type filter.
    pub skip_account_type_filter: bool,
    /// Commitment.
    pub commitment: Option<CommitmentConfig>,
    /// Min context slot.
    pub min_context_slot: Option<u64>,
}

/// Returns all program accounts of the given type matching the specified filters as
/// an iterator, along with context. Deserialization is executed lazily.
pub async fn accounts_lazy_with_context<
    T: AccountDeserialize + Discriminator,
    C: Deref<Target = impl Signer> + Clone,
>(
    program: &Program<C>,
    filters: impl IntoIterator<Item = RpcFilterType>,
    config: ProgramAccountsConfig,
) -> crate::Result<WithSlot<impl Iterator<Item = crate::Result<(Pubkey, T)>>>> {
    let ProgramAccountsConfig {
        skip_account_type_filter,
        commitment,
        min_context_slot,
    } = config;
    let filters = (!skip_account_type_filter)
        .then(|| RpcFilterType::Memcmp(Memcmp::new_base58_encoded(0, T::DISCRIMINATOR)))
        .into_iter()
        .chain(filters)
        .collect::<Vec<_>>();
    let config = ProgramAccountsConfigForRpc {
        filters: (!filters.is_empty()).then_some(filters),
        account_config: RpcAccountInfoConfig {
            encoding: Some(UiAccountEncoding::Base64),
            commitment,
            min_context_slot,
            ..Default::default()
        },
    };
    let client = program.rpc();
    let res = get_program_accounts_with_context(&client, program.id(), config).await?;
    Ok(res.map(|accounts| {
        accounts
            .into_iter()
            .map(|(key, account)| Ok((key, T::try_deserialize(&mut (&account.data as &[u8]))?)))
    }))
}

/// Return the decoded account at the given address, along with context.
///
/// The value inside the context will be `None` if the account does not exist.
pub async fn account_with_context<T: AccountDeserialize>(
    client: &RpcClient,
    address: &Pubkey,
    config: RpcAccountInfoConfig,
) -> crate::Result<WithSlot<Option<T>>> {
    let res = get_account_with_context(client, address, config).await?;
    Ok(res
        .map(|a| {
            a.map(|account| T::try_deserialize(&mut (&account.data as &[u8])))
                .transpose()
        })
        .transpose()?)
}

fn parse_keyed_accounts(
    accounts: Vec<RpcKeyedAccount>,
    request: RpcRequest,
) -> crate::Result<Vec<(Pubkey, Account)>> {
    let mut pubkey_accounts: Vec<(Pubkey, Account)> = Vec::with_capacity(accounts.len());
    for RpcKeyedAccount { pubkey, account } in accounts.into_iter() {
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

/// Get token accounts by owner and return with the context.
pub async fn get_token_accounts_by_owner_with_context(
    client: &RpcClient,
    owner: &Pubkey,
    token_account_filter: TokenAccountsFilter,
    mut config: RpcAccountInfoConfig,
) -> crate::Result<WithSlot<Vec<RpcKeyedAccount>>> {
    let token_account_filter = match token_account_filter {
        TokenAccountsFilter::Mint(mint) => RpcTokenAccountsFilter::Mint(mint.to_string()),
        TokenAccountsFilter::ProgramId(program_id) => {
            RpcTokenAccountsFilter::ProgramId(program_id.to_string())
        }
    };

    if config.commitment.is_none() {
        config.commitment = Some(client.commitment());
    }

    let res = client
        .send::<Response<Vec<RpcKeyedAccount>>>(
            RpcRequest::GetTokenAccountsByOwner,
            json!([owner.to_string(), token_account_filter, config]),
        )
        .await
        .map_err(crate::Error::custom)?;

    Ok(WithSlot::new(res.context.slot, res.value))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use anchor_spl::token::ID;
    use gmsol_solana_utils::cluster::Cluster;
    use solana_sdk::signature::Keypair;

    use crate::client::Client;

    use super::*;

    #[tokio::test]
    async fn get_token_accounts_by_owner() -> crate::Result<()> {
        let client = Client::new(Cluster::Devnet, Arc::new(Keypair::new()))?;
        let rpc = client.rpc();
        let owner = "A1TMhSGzQxMr1TboBKtgixKz1sS6REASMxPo1qsyTSJd"
            .parse()
            .unwrap();
        let accounts = get_token_accounts_by_owner_with_context(
            rpc,
            &owner,
            TokenAccountsFilter::ProgramId(ID),
            RpcAccountInfoConfig {
                encoding: Some(UiAccountEncoding::Base64),
                data_slice: None,
                ..Default::default()
            },
        )
        .await?;

        assert!(!accounts.value().is_empty());

        Ok(())
    }
}
