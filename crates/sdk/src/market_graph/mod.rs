use std::collections::{hash_map::Entry, HashMap, HashSet};

use either::Either;
use gmsol_model::{
    price::{Price, Prices},
    utils::div_to_factor,
    MarketAction, SwapMarketMutExt,
};
use gmsol_programs::{gmsol_store::types::MarketMeta, model::MarketModel};
use petgraph::{
    graph::{EdgeIndex, NodeIndex},
    prelude::StableDiGraph,
};
use rust_decimal::{Decimal, MathematicalOps};
use solana_sdk::pubkey::Pubkey;

use crate::utils::fixed;

type Graph = StableDiGraph<Node, Edge>;
type TokenIx = NodeIndex;

#[derive(Debug)]
struct Node {
    token: Pubkey,
    price: Option<Price<u128>>,
}

impl Node {
    fn new(token: Pubkey) -> Self {
        Self { token, price: None }
    }
}

#[derive(Debug)]
struct Edge {
    estimated: Option<Estimated>,
}

#[derive(Debug)]
struct Estimated {
    exchange_rate: Decimal,
    ln_exchange_rate: Decimal,
}

impl Edge {
    fn new(estimated: Option<Estimated>) -> Self {
        Self { estimated }
    }
}

struct IndexTokenState {
    node: Node,
    markets: HashSet<Pubkey>,
}

struct CollateralTokenState {
    ix: TokenIx,
    markets: HashSet<Pubkey>,
}

struct MarketState {
    market: MarketModel,
    long_edge: EdgeIndex,
    short_edge: EdgeIndex,
}

impl MarketState {
    fn new(market: MarketModel, long_edge: EdgeIndex, short_edge: EdgeIndex) -> Self {
        Self {
            market,
            long_edge,
            short_edge,
        }
    }
}

/// Market Graph.
#[derive(Default)]
pub struct MarketGraph {
    index_tokens: HashMap<Pubkey, IndexTokenState>,
    collateral_tokens: HashMap<Pubkey, CollateralTokenState>,
    markets: HashMap<Pubkey, MarketState>,
    graph: Graph,
    config: MarketGraphConfig,
}

#[derive(Default)]
struct MarketGraphConfig {
    value: u128,
}

impl MarketGraphConfig {
    fn estimate(
        &self,
        market: &MarketModel,
        is_from_long_side: bool,
        prices: Option<Prices<u128>>,
    ) -> Option<Estimated> {
        if self.value == 0 {
            #[cfg(tracing)]
            {
                tracing::trace!("estimation failed with zero input value");
            }
            return None;
        }
        let prices = prices?;
        let mut market = market.clone();
        let token_in_amount = self
            .value
            .checked_div(prices.collateral_token_price(is_from_long_side).min)?;
        let swap = market
            .swap(is_from_long_side, token_in_amount, prices)
            .inspect_err(|err| {
                #[cfg(tracing)]
                {
                    tracing::trace!("estimation failed when creating swap: {err}");
                }
                _ = err;
            })
            .ok()?
            .execute()
            .inspect_err(|err| {
                #[cfg(tracing)]
                {
                    tracing::trace!("estimation failed when executing swap: {err}");
                }
                _ = err;
            })
            .ok()?;
        let token_out_value = swap
            .token_out_amount()
            .checked_mul(prices.collateral_token_price(!is_from_long_side).max)?;
        if token_out_value == 0 {
            #[cfg(tracing)]
            {
                tracing::trace!("estimation failed with zero output value");
            }
            return None;
        }
        let exchange_rate = div_to_factor::<_, { crate::constants::MARKET_DECIMALS }>(
            &token_out_value,
            &self.value,
            false,
        )?;
        let exchange_rate = fixed::unsigned_value_to_decimal(exchange_rate);
        let ln_exchange_rate = exchange_rate.checked_ln()?;
        Some(Estimated {
            exchange_rate,
            ln_exchange_rate,
        })
    }
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
                let long_edge = self
                    .graph
                    .add_edge(long_token_ix, short_token_ix, Edge::new(None));
                let short_edge =
                    self.graph
                        .add_edge(short_token_ix, long_token_ix, Edge::new(None));
                e.insert(MarketState::new(market, long_edge, short_edge));
                self.update_estimated(Some(&key));
                true
            }
            Entry::Occupied(mut e) => {
                let state = e.get_mut();
                state.market = market;
                self.update_estimated(Some(&key));
                false
            }
        }
    }

    fn update_estimated(&mut self, only: Option<&Pubkey>) {
        let markets = only
            .map(|token| Either::Left(self.markets.get(token).into_iter()))
            .unwrap_or_else(|| Either::Right(self.markets.values()));
        for state in markets {
            let prices = self.get_prices(&state.market.meta);
            let long_edge = self
                .graph
                .edge_weight_mut(state.long_edge)
                .expect("internal: inconsistent market map");
            long_edge.estimated = self.config.estimate(&state.market, true, prices);
            let short_edge = self
                .graph
                .edge_weight_mut(state.short_edge)
                .expect("internal: inconsistent market map");
            short_edge.estimated = self.config.estimate(&state.market, false, prices);
        }
    }

    /// Update token price.
    ///
    /// Return `true` if the token exists.
    pub fn update_token_price(&mut self, token: &Pubkey, price: &Price<u128>) {
        if let Some(state) = self.index_tokens.get_mut(token) {
            state.node.price = Some(*price);
        }
        if let Some(state) = self.collateral_tokens.get(token) {
            self.graph
                .node_weight_mut(state.ix)
                .expect("internal: inconsistent token map")
                .price = Some(*price);
        }
        let related_markets_for_index_token = self
            .index_tokens
            .get(token)
            .map(|state| state.markets.iter())
            .into_iter()
            .flatten();
        let related_markets_for_collateral_token = self
            .collateral_tokens
            .get(token)
            .map(|state| state.markets.iter())
            .into_iter()
            .flatten();
        let related_markets = related_markets_for_index_token
            .chain(related_markets_for_collateral_token)
            .copied()
            .collect::<HashSet<_>>();
        for market_token in related_markets {
            self.update_estimated(Some(&market_token));
        }
    }

    /// Update value for the estimation.
    pub fn update_value(&mut self, value: u128) {
        self.config.value = value;
        self.update_estimated(None);
    }

    fn insert_collateral_token(&mut self, token: Pubkey, market_token: Pubkey) -> TokenIx {
        match self.collateral_tokens.entry(token) {
            Entry::Vacant(e) => {
                let ix = self.graph.add_node(Node::new(token));
                let state = CollateralTokenState {
                    ix,
                    markets: HashSet::from([market_token]),
                };
                e.insert(state);
                ix
            }
            Entry::Occupied(mut e) => {
                e.get_mut().markets.insert(market_token);
                e.get().ix
            }
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
        let long_token_ix =
            self.insert_collateral_token(meta.long_token_mint, meta.market_token_mint);
        let short_token_ix =
            self.insert_collateral_token(meta.short_token_mint, meta.market_token_mint);
        (long_token_ix, short_token_ix)
    }

    fn get_token_node(&self, token: &Pubkey) -> Option<&Node> {
        if let Some(state) = self.index_tokens.get(token) {
            Some(&state.node)
        } else {
            let state = self.collateral_tokens.get(token)?;
            self.graph.node_weight(state.ix)
        }
    }

    fn get_price(&self, token: &Pubkey) -> Option<Price<u128>> {
        self.get_token_node(token).and_then(|node| node.price)
    }

    fn get_prices(&self, meta: &MarketMeta) -> Option<Prices<u128>> {
        let index_token_price = self.get_price(&meta.index_token_mint)?;
        let long_token_price = self.get_price(&meta.long_token_mint)?;
        let short_token_price = self.get_price(&meta.short_token_mint)?;
        Some(Prices {
            index_token_price,
            long_token_price,
            short_token_price,
        })
    }

    /// Get market by its market token.
    pub fn get_market(&self, market_token: &Pubkey) -> Option<&MarketModel> {
        Some(&self.markets.get(market_token)?.market)
    }

    /// Get all markets.
    pub fn markets(&self) -> impl Iterator<Item = &MarketModel> {
        self.markets.values().map(|state| &state.market)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gmsol_programs::gmsol_store::accounts::Market;
    use petgraph::dot::Dot;

    use crate::{
        constants,
        utils::{test::setup_fmt_tracing, zero_copy::try_deserialize_zero_copy_from_base64},
    };

    use super::*;

    fn get_market_updates() -> Vec<(String, u64)> {
        const DATA: &str = include_str!("test_data/markets.csv");
        DATA.trim()
            .split('\n')
            .enumerate()
            .map(|(idx, data)| {
                let (market, supply) = data
                    .split_once(',')
                    .unwrap_or_else(|| panic!("[{idx}] invalid data"));
                (
                    market.to_string(),
                    supply
                        .parse()
                        .unwrap_or_else(|_| panic!("[{idx}] invalid supply format")),
                )
            })
            .collect()
    }

    fn get_price_updates() -> Vec<(i64, Pubkey, Price<u128>)> {
        const DATA: &str = include_str!("test_data/prices.csv");
        DATA.trim()
            .split('\n')
            .enumerate()
            .map(|(idx, data)| {
                let mut data = data.split(',');
                let ts = data.next().unwrap_or_else(|| panic!("[{idx}] missing ts"));
                let token = data
                    .next()
                    .unwrap_or_else(|| panic!("[{idx}] missing token"));
                let min = data
                    .next()
                    .unwrap_or_else(|| panic!("[{idx}] missing min price"));
                let max = data
                    .next()
                    .unwrap_or_else(|| panic!("[{idx}] missing max price"));
                (
                    ts.parse()
                        .unwrap_or_else(|_| panic!("[{idx}] invalid ts format")),
                    token
                        .parse()
                        .unwrap_or_else(|_| panic!("[{idx}] invalid token format")),
                    Price {
                        min: min
                            .parse()
                            .unwrap_or_else(|_| panic!("[{idx}] invalid min price format")),
                        max: max
                            .parse()
                            .unwrap_or_else(|_| panic!("[{idx}] invalid max price format")),
                    },
                )
            })
            .collect()
    }

    #[test]
    fn create_and_update_market_graph() -> crate::Result<()> {
        let _tracing = setup_fmt_tracing("info");

        let mut graph = MarketGraph::default();
        let updates = get_market_updates();
        let prices = get_price_updates();
        let mut market_tokens = HashSet::<Pubkey>::default();

        // Update markets.
        for (data, supply) in updates {
            let market = try_deserialize_zero_copy_from_base64::<Market>(&data)?.0;
            market_tokens.insert(market.meta.market_token_mint);
            graph.insert_market(MarketModel::from_parts(Arc::new(market), supply));
        }

        // Update prices.
        for (_, token, price) in prices {
            graph.update_token_price(&token, &price);
        }

        // Update value.
        graph.update_value(10 * constants::MARKET_USD_UNIT);

        let num_markets = graph.markets().count();
        assert_eq!(num_markets, market_tokens.len());
        for market_token in market_tokens {
            let market = graph.get_market(&market_token).expect("must exist");
            assert_eq!(market.meta.market_token_mint, market_token);
        }
        println!("{:?}", Dot::new(&graph.graph));
        Ok(())
    }
}
