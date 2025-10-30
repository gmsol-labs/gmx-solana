use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use solana_sdk::{pubkey::Pubkey, signer::Signer};

use super::types::PerMarketUpdates;
use crate::{client::ops::market::MarketOps, Client};

pub struct RiskOracleApplier<'a, C> {
    client: &'a Client<C>,
    store: Pubkey,
}

impl<'a, C: Clone> RiskOracleApplier<'a, C> {
    pub fn new(client: &'a Client<C>, store: Pubkey) -> Self {
        Self { client, store }
    }
}

impl<'a, C, S> RiskOracleApplier<'a, C>
where
    C: Clone + std::ops::Deref<Target = S>,
    S: Signer,
{
    pub fn build_apply_transactions<'b>(
        &self,
        buffer: &'b dyn Signer,
        expire_after_secs: u32,
        updates: &[PerMarketUpdates],
    ) -> crate::Result<(
        TransactionBuilder<'b, C>,
        Vec<TransactionBuilder<'_, C>>,
        Vec<TransactionBuilder<'_, C>>,
    )>
    where
        'a: 'b,
    {
        let init =
            self.client
                .initialize_market_config_buffer(&self.store, buffer, expire_after_secs);

        let mut pushes: Vec<TransactionBuilder<'_, C>> = Vec::new();
        for u in updates {
            let push = self
                .client
                .push_to_market_config_buffer(&buffer.pubkey(), u.entries.iter().cloned());
            pushes.push(push);
        }

        let mut applies: Vec<TransactionBuilder<'_, C>> = Vec::new();
        for u in updates {
            let apply = self.client.update_market_config_with_buffer(
                &self.store,
                &u.market,
                &buffer.pubkey(),
            );
            applies.push(apply);
        }

        Ok((init, pushes, applies))
    }
}
