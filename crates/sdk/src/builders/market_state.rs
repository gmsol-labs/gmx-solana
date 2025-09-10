use gmsol_programs::gmsol_store::{
    accounts::{Market, Store},
    client::{accounts, args},
};
use gmsol_solana_utils::{
    client_traits::{FromRpcClientWith, RpcClientExt},
    AtomicGroup, IntoAtomicGroup, ProgramExt,
};
use gmsol_utils::{
    oracle::PriceProviderKind,
    pubkey::optional_address,
    token_config::{token_records, TokensWithFeed},
};
use typed_builder::TypedBuilder;

use crate::{
    serde::{
        serde_price_feed::{to_tokens_with_feeds, SerdeTokenRecord},
        StringPubkey,
    },
    utils::{
        market::ordered_tokens,
        token_map::{FeedAddressMap, FeedsParser, TokenMap},
        zero_copy::ZeroCopy,
    },
};

use super::StoreProgram;

/// Builder for `udpate_closed_state` instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct UpdateClosedState {
    /// Payer (a.k.a. authority).
    #[builder(setter(into))]
    pub payer: StringPubkey,
    /// Store program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub store_program: StoreProgram,
    /// Oracle buffer account.
    #[builder(setter(into))]
    pub oracle: StringPubkey,
    /// Market token mint address.
    #[builder(setter(into))]
    pub market_token: StringPubkey,
    /// Feeds Parser.
    #[cfg_attr(serde, serde(skip))]
    #[builder(default)]
    pub feeds_parser: FeedsParser,
}

impl UpdateClosedState {
    /// Insert a feed parser.
    pub fn insert_feed_parser(
        &mut self,
        provider: PriceProviderKind,
        map: FeedAddressMap,
    ) -> crate::Result<()> {
        self.feeds_parser
            .insert_pull_oracle_feed_parser(provider, map);
        Ok(())
    }
}

impl IntoAtomicGroup for UpdateClosedState {
    type Hint = UpdateClosedStateHint;

    fn into_atomic_group(self, hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let authority = &self.payer.0;
        let market = self.store_program.find_market_address(&self.market_token);
        let feeds = self
            .feeds_parser
            .parse(&hint.to_tokens_with_feeds()?)
            .collect::<Result<Vec<_>, _>>()
            .map_err(gmsol_solana_utils::Error::custom)?;
        let update = self
            .store_program
            .anchor_instruction(args::UpdateClosedState {})
            .anchor_accounts(
                accounts::UpdateClosedState {
                    authority: *authority,
                    store: self.store_program.store.0,
                    token_map: hint.token_map.0,
                    oracle: self.oracle.0,
                    market,
                },
                false,
            )
            .accounts(feeds)
            .build();
        Ok(AtomicGroup::with_instructions(authority, [update]))
    }
}

/// Hint for [`UpdateClosedState`].
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct UpdateClosedStateHint {
    /// Token map.
    #[builder(setter(into))]
    pub token_map: StringPubkey,
    /// Feeds.
    #[builder(setter(into))]
    pub feeds: Vec<SerdeTokenRecord>,
}

impl UpdateClosedStateHint {
    /// Create [`TokensWithFeed`]
    pub fn to_tokens_with_feeds(&self) -> gmsol_solana_utils::Result<TokensWithFeed> {
        to_tokens_with_feeds(&self.feeds).map_err(gmsol_solana_utils::Error::custom)
    }
}

impl FromRpcClientWith<UpdateClosedState> for UpdateClosedStateHint {
    async fn from_rpc_client_with<'a>(
        builder: &'a UpdateClosedState,
        client: &'a impl gmsol_solana_utils::client_traits::RpcClient,
    ) -> gmsol_solana_utils::Result<Self> {
        let store_program = &builder.store_program;
        let store_address = &store_program.store.0;
        let store = client
            .get_anchor_account::<ZeroCopy<Store>>(store_address, Default::default())
            .await?
            .0;
        let token_map_address = optional_address(&store.token_map)
            .ok_or_else(|| gmsol_solana_utils::Error::custom("token map is not set"))?;
        let market_address = store_program.find_market_address(&builder.market_token);
        let market = client
            .get_anchor_account::<ZeroCopy<Market>>(&market_address, Default::default())
            .await?
            .0;
        let tokens = ordered_tokens(&market.meta.into());
        let token_map = client
            .get_anchor_account::<TokenMap>(token_map_address, Default::default())
            .await?;
        let feeds = token_records(&token_map, &tokens)
            .map_err(gmsol_solana_utils::Error::custom)?
            .into_iter()
            .map(SerdeTokenRecord::try_from)
            .collect::<Result<Vec<_>, _>>()
            .map_err(gmsol_solana_utils::Error::custom)?;

        Ok(Self {
            token_map: (*token_map_address).into(),
            feeds,
        })
    }
}
