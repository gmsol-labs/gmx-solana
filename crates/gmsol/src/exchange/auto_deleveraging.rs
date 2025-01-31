use std::ops::Deref;

use anchor_client::{
    anchor_lang::Id,
    solana_sdk::{pubkey::Pubkey, signer::Signer},
};
use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use gmsol_store::states::{common::TokensWithFeed, Market, Pyth};

use crate::{store::utils::FeedsParser, utils::fix_optional_account_metas};

/// The compute budget for `auto_deleverage`.
pub const ADL_COMPUTE_BUDGET: u32 = 800_000;

#[cfg(feature = "pyth-pull-oracle")]
use crate::pyth::pull_oracle::{ExecuteWithPythPrices, Prices, PythPullOracleContext};

/// Update ADL state Instruction Builder.
pub struct UpdateAdlBuilder<'a, C> {
    client: &'a crate::Client<C>,
    store: Pubkey,
    market_token: Pubkey,
    oracle: Pubkey,
    is_long: bool,
    price_provider: Pubkey,
    hint: Option<UpdateAdlHint>,
    feeds_parser: FeedsParser,
}

impl<'a, C: Deref<Target = impl Signer> + Clone> UpdateAdlBuilder<'a, C> {
    pub(super) fn try_new(
        client: &'a crate::Client<C>,
        store: &Pubkey,
        oracle: &Pubkey,
        market_token: &Pubkey,
        is_long: bool,
    ) -> crate::Result<Self> {
        Ok(Self {
            client,
            store: *store,
            market_token: *market_token,
            oracle: *oracle,
            is_long,
            price_provider: Pyth::id(),
            hint: None,
            feeds_parser: FeedsParser::default(),
        })
    }

    /// Prepare hint for auto-deleveraging.
    pub async fn prepare_hint(&mut self) -> crate::Result<UpdateAdlHint> {
        match &self.hint {
            Some(hint) => Ok(hint.clone()),
            None => {
                let market_address = self
                    .client
                    .find_market_address(&self.store, &self.market_token);
                let market = self.client.market(&market_address).await?;
                let hint = UpdateAdlHint::from_market(self.client, &market).await?;
                self.hint = Some(hint.clone());
                Ok(hint)
            }
        }
    }

    /// Set price provider to the given.
    pub fn price_provider(&mut self, program: &Pubkey) -> &mut Self {
        self.price_provider = *program;
        self
    }

    /// Parse feeds with the given price udpates map.
    #[cfg(feature = "pyth-pull-oracle")]
    pub fn parse_with_pyth_price_updates(&mut self, price_updates: Prices) -> &mut Self {
        self.feeds_parser.with_pyth_price_updates(price_updates);
        self
    }

    /// Build [`TransactionBuilder`] for auto-delevearaging the position.
    pub async fn build(&mut self) -> crate::Result<TransactionBuilder<'a, C>> {
        let hint = self.prepare_hint().await?;
        let feeds = self
            .feeds_parser
            .parse(hint.feeds())
            .collect::<Result<Vec<_>, _>>()?;
        let rpc = self
            .client
            .store_transaction()
            .accounts(fix_optional_account_metas(
                gmsol_store::accounts::UpdateAdlState {
                    authority: self.client.payer(),
                    store: self.store,
                    token_map: hint.token_map,
                    oracle: self.oracle,
                    market: self
                        .client
                        .find_market_address(&self.store, &self.market_token),
                    chainlink_program: None,
                },
                &crate::program_ids::DEFAULT_GMSOL_STORE_ID,
                self.client.store_program_id(),
            ))
            .anchor_args(gmsol_store::instruction::UpdateAdlState {
                is_long: self.is_long,
            })
            .accounts(feeds);
        Ok(rpc)
    }
}

/// Hint for `update_adl_state`.
#[derive(Clone)]
pub struct UpdateAdlHint {
    token_map: Pubkey,
    tokens_with_feed: TokensWithFeed,
}

impl UpdateAdlHint {
    async fn from_market<C: Deref<Target = impl Signer> + Clone>(
        client: &crate::Client<C>,
        market: &Market,
    ) -> crate::Result<Self> {
        use gmsol_store::states::common::token_with_feeds::token_records;

        let store_address = market.store;
        let token_map_address = client
            .authorized_token_map_address(&store_address)
            .await?
            .ok_or(crate::Error::invalid_argument(
                "token map is not configurated for the store",
            ))?;
        let token_map = client.token_map(&token_map_address).await?;
        let meta = market.meta();

        let records = token_records(
            &token_map,
            &[
                meta.index_token_mint,
                meta.long_token_mint,
                meta.short_token_mint,
            ]
            .into(),
        )?;
        let tokens_with_feed = TokensWithFeed::try_from_records(records)?;

        Ok(Self {
            token_map: token_map_address,
            tokens_with_feed,
        })
    }

    /// Get feeds.
    pub fn feeds(&self) -> &TokensWithFeed {
        &self.tokens_with_feed
    }
}

#[cfg(feature = "pyth-pull-oracle")]
impl<'a, C: Deref<Target = impl Signer> + Clone> ExecuteWithPythPrices<'a, C>
    for UpdateAdlBuilder<'a, C>
{
    fn should_estiamte_execution_fee(&self) -> bool {
        false
    }

    fn set_execution_fee(&mut self, _lamports: u64) {}

    async fn context(&mut self) -> crate::Result<PythPullOracleContext> {
        let hint = self.prepare_hint().await?;
        let ctx = PythPullOracleContext::try_from_feeds(hint.feeds())?;
        Ok(ctx)
    }

    async fn build_rpc_with_price_updates(
        &mut self,
        price_updates: Prices,
    ) -> crate::Result<Vec<TransactionBuilder<'a, C, ()>>> {
        let tx = self
            .parse_with_pyth_price_updates(price_updates)
            .build()
            .await?;
        Ok(vec![tx])
    }
}
