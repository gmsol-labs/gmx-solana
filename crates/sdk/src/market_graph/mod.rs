use std::collections::{hash_map::Entry, HashMap, HashSet};

use gmsol_model::price::Price;
use gmsol_programs::{gmsol_store::types::MarketMeta, model::MarketModel};
use petgraph::{
    graph::{EdgeIndex, NodeIndex},
    prelude::StableUnGraph,
};
use solana_sdk::pubkey::Pubkey;

type Graph = StableUnGraph<Node, Edge>;
type TokenIx = NodeIndex;
type MarketIx = EdgeIndex;

struct Node {
    token: Pubkey,
    price: Option<Price<u128>>,
}

impl Node {
    fn new(token: Pubkey) -> Self {
        Self { token, price: None }
    }
}

struct Edge {
    market: MarketModel,
    estimated: Option<Estimated>,
}

struct Estimated {}

impl Edge {
    fn new(market: MarketModel) -> Self {
        Self {
            market,
            estimated: None,
        }
    }
}

struct IndexTokenState {
    node: Node,
    markets: HashSet<Pubkey>,
}

/// Market Graph.
#[derive(Default)]
pub struct MarketGraph {
    index_tokens: HashMap<Pubkey, IndexTokenState>,
    collateral_tokens: HashMap<Pubkey, TokenIx>,
    markets: HashMap<Pubkey, MarketIx>,
    graph: Graph,
    config: MarketGraphConfig,
}

#[derive(Default)]
struct MarketGraphConfig {
    value: u128,
}

impl MarketGraph {
    /// Insert or update a market.
    ///
    /// Return `true` if the market is newly inserted.
    pub fn insert_market(&mut self, market: MarketModel) -> bool {
        let key = market.meta.market_token_mint;
        let (long_token_ix, short_token_ix) = self.insert_tokens_with_meta(&market.meta);
        match self.markets.entry(key) {
            Entry::Vacant(e) => {
                let market_ix =
                    self.graph
                        .add_edge(long_token_ix, short_token_ix, Edge::new(market));
                e.insert(market_ix);
                // TODO: update the market estimation.
                true
            }
            Entry::Occupied(e) => {
                let market_ix = *e.get();
                self.graph
                    .edge_weight_mut(market_ix)
                    .expect("internal: inconsistent market map")
                    .market = market;
                // TODO: update the market estimation.
                false
            }
        }
    }

    /// Update token price.
    ///
    /// Return `true` if the token exists.
    pub fn update_token_price(&mut self, token: &Pubkey, price: &Price<u128>) {
        if let Some(state) = self.index_tokens.get_mut(token) {
            state.node.price = Some(*price);
            // TODO: update related market estimations.
        } else if let Some(ix) = self.collateral_tokens.get(token) {
            self.graph
                .node_weight_mut(*ix)
                .expect("internal: inconsistent token map")
                .price = Some(*price);
            // TODO: update related markets estimations.
        }
    }

    /// Update value for the estimation.
    pub fn update_value(&mut self, value: u128) {
        self.config.value = value;
        // TODO: update all the estimations.
    }

    fn insert_collateral_token(&mut self, token: Pubkey) -> TokenIx {
        match self.collateral_tokens.entry(token) {
            Entry::Vacant(e) => {
                let ix = self.graph.add_node(Node::new(token));
                e.insert(ix);
                ix
            }
            Entry::Occupied(e) => *e.get(),
        }
    }

    fn insert_index_token(&mut self, index_token: Pubkey, market_token: Pubkey) {
        self.index_tokens
            .entry(index_token)
            .or_insert_with(|| IndexTokenState {
                markets: HashSet::default(),
                node: Node::new(index_token),
            })
            .markets
            .insert(market_token);
    }

    fn insert_tokens_with_meta(&mut self, meta: &MarketMeta) -> (TokenIx, TokenIx) {
        self.insert_index_token(meta.index_token_mint, meta.market_token_mint);
        let long_token_ix = self.insert_collateral_token(meta.long_token_mint);
        let short_token_ix = self.insert_collateral_token(meta.short_token_mint);
        (long_token_ix, short_token_ix)
    }

    /// Get market by its market token.
    pub fn get_market(&self, market_token: &Pubkey) -> Option<&MarketModel> {
        let ix = *self.markets.get(market_token)?;
        Some(&self.graph.edge_weight(ix)?.market)
    }

    /// Get all markets.
    pub fn markets(&self) -> impl Iterator<Item = &MarketModel> {
        self.markets
            .values()
            .filter_map(|ix| Some(&self.graph.edge_weight(*ix)?.market))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gmsol_programs::gmsol_store::accounts::Market;

    use crate::utils::zero_copy::try_deserialize_zero_copy_from_base64;

    use super::*;

    fn get_market_updates() -> Vec<(String, u64)> {
        const DATA: &str = include_str!("test_data/markets.csv");
        DATA.trim()
            .split('\n')
            .enumerate()
            .map(|(idx, data)| {
                let (market, supply) = data
                    .split_once(',')
                    .unwrap_or_else(|| panic!("invalid data: {idx}"));
                (
                    market.to_string(),
                    supply.parse().expect("invalid supply format"),
                )
            })
            .collect()
    }

    #[test]
    fn create_and_update_market_graph() -> crate::Result<()> {
        let mut graph = MarketGraph::default();
        let updates = get_market_updates();
        let mut market_tokens = HashSet::<Pubkey>::default();
        for (data, supply) in updates {
            let market = try_deserialize_zero_copy_from_base64::<Market>(&data)?.0;
            market_tokens.insert(market.meta.market_token_mint);
            graph.insert_market(MarketModel::from_parts(Arc::new(market), supply));
        }
        let num_markets = graph.markets().count();
        assert_eq!(num_markets, market_tokens.len());
        for market_token in market_tokens {
            let market = graph.get_market(&market_token).expect("must exist");
            assert_eq!(market.meta.market_token_mint, market_token);
        }
        Ok(())
    }
}
