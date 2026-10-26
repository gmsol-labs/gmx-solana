/// Wormhole Ops.
pub mod wormhole;

/// Pyth Reciever Ops.
pub mod receiver;

/// Hermes.
pub mod hermes;

/// Utils.
pub mod utils;

mod pull_oracle_impl;

use std::ops::Deref;

use gmsol_solana_utils::{
    bundle_builder::BundleBuilder, program::Program, transaction_builder::TransactionBuilder,
};
use solana_account_decoder::{UiAccountEncoding, UiDataSliceConfig};
use solana_client::rpc_config::RpcAccountInfoConfig;
use solana_sdk::{pubkey::Pubkey, signer::Signer};

use self::wormhole::WORMHOLE_PROGRAM_ID;

pub use self::{
    pull_oracle_impl::{PriceUpdates, PythPullOracleWithHermes},
    receiver::PythReceiverOps,
    wormhole::WormholeOps,
};

const VAA_SPLIT_INDEX: usize = 755;

/// Max addresses per `getMultipleAccounts` RPC call.
const MAX_ACCOUNTS_PER_FETCH: usize = 100;

/// Pyth Pull Oracle.
pub struct PythPullOracle<C> {
    wormhole: Program<C>,
    pyth: Program<C>,
}

impl<S, C> PythPullOracle<C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    /// Create a new [`PythPullOracle`] client from [`Client`](crate::Client).
    pub fn try_new(client: &crate::Client<C>) -> crate::Result<Self> {
        Ok(Self {
            wormhole: client.program(WORMHOLE_PROGRAM_ID),
            pyth: client.program(pyth_solana_receiver_sdk::ID),
        })
    }

    /// Build a bundle that closes the given temporary accounts that still exist, returning
    /// their rent to the payer. The result is empty when nothing is left to close.
    ///
    /// `accounts` comes from
    /// [`WithPullOracle::temporary_accounts`](crate::client::pull_oracle::WithPullOracle::temporary_accounts).
    /// Addresses that do not exist (their creating transaction failed, or they are already
    /// closed) are skipped, so calling this again after a partial cleanup is safe.
    ///
    /// Only the payer signs, and it must be the payer that created the accounts.
    ///
    /// INVARIANT: no transaction that could still create one of `accounts` may be in flight,
    /// or that account is missed. Settle unresolved sends first with
    /// [`wait_for_blockhash_expiry`](gmsol_solana_utils::client::wait_for_blockhash_expiry),
    /// passing `client`'s commitment level: the lookup reads at that level, and a wait at a
    /// weaker level (e.g. `confirmed` for a `finalized` client) can end seconds before the
    /// account is visible.
    pub async fn close_temporary_accounts<'a>(
        &'a self,
        client: &'a crate::Client<C>,
        accounts: &[Pubkey],
    ) -> crate::Result<BundleBuilder<'a, C>> {
        let mut bundle = client.bundle();
        let mut closes = bundle.push_parallel();
        for chunk in accounts.chunks(MAX_ACCOUNTS_PER_FETCH) {
            // Only existence and owner matter, so skip the account data.
            let config = RpcAccountInfoConfig {
                encoding: Some(UiAccountEncoding::Base64),
                data_slice: Some(UiDataSliceConfig {
                    offset: 0,
                    length: 0,
                }),
                ..Default::default()
            };
            let fetched = client
                .rpc()
                .get_multiple_accounts_with_config(chunk, config)
                .await
                .map_err(crate::Error::custom)?
                .value;
            for (address, account) in chunk.iter().zip(fetched) {
                let Some(account) = account else {
                    continue;
                };
                let Some(close) = self.close_temporary_account(address, &account.owner) else {
                    tracing::warn!(%address, owner = %account.owner, "not a temporary Pyth account, skipped");
                    continue;
                };
                closes.add(close);
            }
        }
        drop(closes);
        Ok(bundle)
    }

    /// The close instruction for a temporary account, picked by the program that owns it.
    fn close_temporary_account(
        &self,
        address: &Pubkey,
        owner: &Pubkey,
    ) -> Option<TransactionBuilder<'_, C>> {
        if owner == self.wormhole.id() {
            Some(self.wormhole.close_encoded_vaa(address))
        } else if owner == self.pyth.id() {
            Some(self.pyth.reclaim_rent(address))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gmsol_solana_utils::cluster::Cluster;
    use solana_sdk::signature::Keypair;

    use super::*;

    fn closing_program(oracle: &PythPullOracle<Arc<Keypair>>, owner: &Pubkey) -> Option<Pubkey> {
        let close = oracle.close_temporary_account(&Pubkey::new_unique(), owner)?;
        Some(close.instructions().last()?.program_id)
    }

    #[test]
    fn picks_close_by_owner() {
        let client = crate::Client::new(Cluster::Devnet, Arc::new(Keypair::new())).unwrap();
        let oracle = PythPullOracle::try_new(&client).unwrap();
        let receiver = pyth_solana_receiver_sdk::ID;

        assert_eq!(
            closing_program(&oracle, &WORMHOLE_PROGRAM_ID),
            Some(WORMHOLE_PROGRAM_ID)
        );
        assert_eq!(closing_program(&oracle, &receiver), Some(receiver));
        assert_eq!(closing_program(&oracle, &Pubkey::new_unique()), None);
    }
}
