use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    fmt,
};

use either::Either;
use gmsol_model::{
    action::swap::SwapReport,
    price::{Price, Prices},
    MarketAction, SwapMarketMutExt,
};
use gmsol_programs::{gmsol_store::types::MarketMeta, model::MarketModel};
use petgraph::{
    graph::{EdgeIndex, NodeIndex},
    prelude::StableDiGraph,
    visit::{EdgeRef, IntoNodeIdentifiers, NodeIndexable},
};
use rust_decimal::{Decimal, MathematicalOps};
use solana_sdk::pubkey::Pubkey;

use crate::{
    builders::order::{CreateOrderKind, CreateOrderParams},
    market_graph::simulation::order::{OrderSimulation, OrderSimulationBuilder},
};

use self::estimation::SwapEstimation;

pub use self::{
    config::MarketGraphConfig, error::MarketGraphError, estimation::SwapEstimationParams,
};

/// Estimation.
pub mod estimation;

/// Config.
pub mod config;

/// Error type.
pub mod error;

/// Execution simulation.
pub mod simulation;

type Graph = StableDiGraph<Node, Edge>;

/// Order Simulation Builder.
pub type OrderSimulationBuilderForGraph<'a> = OrderSimulationBuilder<
    'a,
    (
        (&'a MarketGraph,),
        (CreateOrderKind,),
        (&'a CreateOrderParams,),
        (&'a Pubkey,),
        (),
        (),
        (),
        (),
    ),
>;

#[derive(Debug)]
struct Node {
    #[allow(dead_code)]
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
    market_token: Pubkey,
    estimated: Option<SwapEstimation>,
}

impl Edge {
    fn new(market_token: Pubkey, estimated: Option<SwapEstimation>) -> Self {
        Self {
            market_token,
            estimated,
        }
    }

    fn cost(&self) -> Option<Decimal> {
        Some(-self.estimated.as_ref()?.ln_exchange_rate)
    }
}

#[derive(Debug)]
struct IndexTokenState {
    node: Node,
    markets: HashSet<Pubkey>,
}

#[derive(Debug)]
struct CollateralTokenState {
    ix: NodeIndex,
    markets: HashSet<Pubkey>,
}

#[derive(Debug)]
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
#[derive(Debug)]
pub struct MarketGraph {
    index_tokens: HashMap<Pubkey, IndexTokenState>,
    collateral_tokens: HashMap<Pubkey, CollateralTokenState>,
    markets: HashMap<Pubkey, MarketState>,
    graph: Graph,
    config: MarketGraphConfig,
}

type Distances = Vec<Option<Decimal>>;
type Predecessors = Vec<Option<(NodeIndex, Pubkey)>>;

impl Default for MarketGraph {
    fn default() -> Self {
        Self::with_config(MarketGraphConfig::default())
    }
}

impl MarketGraph {
    /// Create from the given [`MarketGraphConfig`].
    pub fn with_config(config: MarketGraphConfig) -> Self {
        Self {
            index_tokens: Default::default(),
            collateral_tokens: Default::default(),
            markets: Default::default(),
            graph: Default::default(),
            config,
        }
    }

    /// Insert or update a market.
    ///
    /// Return `true` if the market is newly inserted.
    pub fn insert_market(&mut self, market: MarketModel) -> bool {
        let key = market.meta.market_token_mint;
        let (long_token_ix, short_token_ix) = self.insert_tokens_with_meta(&market.meta);
        match self.markets.entry(key) {
            Entry::Vacant(e) => {
                let long_edge =
                    self.graph
                        .add_edge(long_token_ix, short_token_ix, Edge::new(key, None));
                let short_edge =
                    self.graph
                        .add_edge(short_token_ix, long_token_ix, Edge::new(key, None));
                e.insert(MarketState::new(market, long_edge, short_edge));
                self.update_estimation(Some(&key));
                true
            }
            Entry::Occupied(mut e) => {
                let state = e.get_mut();
                state.market = market;
                self.update_estimation(Some(&key));
                false
            }
        }
    }

    fn update_estimation(&mut self, only: Option<&Pubkey>) {
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
            self.update_estimation(Some(&market_token));
        }
    }

    /// Update value for the estimation.
    pub fn update_value(&mut self, value: u128) {
        let mut config = self.config;
        config.swap_estimation_params.value = value;
        self.update_config(config, true);
    }

    /// Update base cost.
    pub fn update_base_cost(&mut self, base_cost: u128) {
        let mut config = self.config;
        config.swap_estimation_params.base_cost = base_cost;
        self.update_config(config, true);
    }

    /// Update max steps.
    pub fn update_max_steps(&mut self, max_steps: usize) {
        self.update_config(
            MarketGraphConfig {
                max_steps,
                ..self.config
            },
            false,
        );
    }

    /// Update config.
    fn update_config(&mut self, config: MarketGraphConfig, should_update_estimation: bool) {
        self.config = config;
        if should_update_estimation {
            self.update_estimation(None);
        }
    }

    fn insert_collateral_token(&mut self, token: Pubkey, market_token: Pubkey) -> NodeIndex {
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

    fn insert_tokens_with_meta(&mut self, meta: &MarketMeta) -> (NodeIndex, NodeIndex) {
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

    /// Get all market tokens.
    pub fn market_tokens(&self) -> impl Iterator<Item = &Pubkey> {
        self.markets.keys()
    }

    /// Get all index tokens.
    pub fn index_tokens(&self) -> impl Iterator<Item = &Pubkey> {
        self.index_tokens.keys()
    }

    fn to_index(&self, ix: NodeIndex) -> usize {
        self.graph.to_index(ix)
    }

    /// Bellman-Ford algorithm with a maximum step limit.
    ///
    /// It computes the shortest paths in the subgraph reachable from the source
    /// within at most `max_steps` steps.
    fn bellman_ford(&self, source: &Pubkey) -> crate::Result<(Distances, Predecessors)> {
        let source = self
            .collateral_tokens
            .get(source)
            .ok_or_else(|| crate::Error::custom("the source is not a known collateral token"))?
            .ix;

        let g = &self.graph;
        let max_steps = self.config.max_steps;
        let mut predecessors = vec![None; g.node_bound()];
        let mut distances = vec![None; g.node_bound()];
        distances[self.to_index(source)] = Some(Decimal::ZERO);

        let mut result_distances = None;

        for steps in 1..self.graph.node_count() {
            let mut did_update = false;
            for i in g.node_identifiers() {
                for edge in g.edges(i) {
                    let j = edge.target();
                    let Some(w) = edge.weight().cost() else {
                        continue;
                    };
                    let Some(d) = distances[self.to_index(i)] else {
                        continue;
                    };
                    if distances[self.to_index(j)]
                        .map(|current| d + w < current)
                        .unwrap_or(true)
                    {
                        distances[self.to_index(j)] = distances[self.to_index(i)].map(|d| d + w);

                        // Only update predecessors if the current step is within `max_steps`.
                        if steps <= max_steps {
                            predecessors[self.to_index(j)] = Some((i, edge.weight().market_token));
                        }

                        did_update = true;
                    }
                }
            }

            if !did_update {
                break;
            }

            // Cache the result within the `max_steps`.
            if steps == max_steps {
                result_distances = Some(distances.clone());
            }
        }

        // Check for negative weight cycle.
        for i in g.node_identifiers() {
            for edge in g.edges(i) {
                let j = edge.target();
                let Some(w) = edge.weight().cost() else {
                    continue;
                };
                let Some(d) = distances[self.to_index(i)] else {
                    continue;
                };
                if distances[self.to_index(j)]
                    .map(|jd| d + w < jd)
                    .unwrap_or(true)
                {
                    return Err(MarketGraphError::NegativeCycle.into());
                }
            }
        }

        Ok((result_distances.unwrap_or(distances), predecessors))
    }

    fn dfs(&self, source: &Pubkey) -> crate::Result<(Distances, Predecessors)> {
        let source = self
            .collateral_tokens
            .get(source)
            .ok_or_else(|| crate::Error::custom("the source is not a known collateral token"))?
            .ix;

        let g = &self.graph;
        let mut predecessors = vec![None; g.node_bound()];
        let mut distances = vec![None; g.node_bound()];

        let mut visited = HashSet::<EdgeIndex>::new();

        self.dfs_recursive(
            source,
            Some(Decimal::ZERO),
            None,
            0,
            &mut visited,
            &mut distances,
            &mut predecessors,
        );

        Ok((distances, predecessors))
    }

    #[allow(clippy::too_many_arguments)]
    fn dfs_recursive(
        &self,
        current: NodeIndex,
        distance: Option<Decimal>,
        predecessor: Option<(NodeIndex, Pubkey)>,
        steps: usize,
        visited: &mut HashSet<EdgeIndex>,
        distances: &mut Distances,
        predecessors: &mut Predecessors,
    ) {
        let i = current;
        if steps > self.config.max_steps {
            return;
        }
        let Some(d) = distance else {
            return;
        };
        let best_d = distances[self.to_index(i)];
        if best_d.map(|best| d >= best).unwrap_or(false) {
            return;
        }
        distances[self.to_index(i)] = Some(d);
        predecessors[self.to_index(i)] = predecessor;

        for edge in self.graph.edges(i) {
            let edge_ix = edge.id();
            if visited.contains(&edge_ix) {
                continue;
            }
            visited.insert(edge_ix);
            let j = edge.target();
            self.dfs_recursive(
                j,
                edge.weight().cost().map(|w| w + d),
                Some((i, edge.weight().market_token)),
                steps + 1,
                visited,
                distances,
                predecessors,
            );
            visited.remove(&edge_ix);
        }
    }

    /// Find the best swap path for the given source and target.
    pub fn best_swap_paths(
        &self,
        source: &Pubkey,
        skip_bellman_ford: bool,
    ) -> crate::Result<BestSwapPaths<'_>> {
        let (distances, predecessors, arbitrage_exists) = if skip_bellman_ford {
            let results = self.dfs(source)?;
            (results.0, results.1, None)
        } else {
            match self.bellman_ford(source) {
                Ok(results) => (results.0, results.1, Some(false)),
                // Fallback to DFS.
                Err(crate::Error::MarketGraph(MarketGraphError::NegativeCycle)) => {
                    let results = self.dfs(source)?;
                    (results.0, results.1, Some(true))
                }
                Err(err) => return Err(err),
            }
        };

        Ok(BestSwapPaths {
            graph: self,
            source: *source,
            distances,
            predecessors,
            arbitrage_exists,
        })
    }

    /// Swap along the provided path with the given price updater.
    pub fn swap_along_path_with_price_updater(
        &self,
        path: &[Pubkey],
        source_token: &Pubkey,
        mut amount: u128,
        mut price_updater: impl FnMut(&MarketMeta, &mut Prices<u128>) -> crate::Result<()>,
    ) -> crate::Result<SwapOutput> {
        let mut current_token = *source_token;

        let mut reports = Vec::with_capacity(path.len());
        for market_token in path {
            let mut market = self
                .get_market(market_token)
                .ok_or_else(|| {
                    crate::Error::custom(format!(
                        "[swap] market `{market_token}` not found in the graph"
                    ))
                })?
                .clone();
            let meta = &market.meta;
            if meta.long_token_mint == meta.short_token_mint {
                return Err(crate::Error::custom(format!(
                    "[swap] `{market_token} is not a swappable market"
                )));
            }
            let is_token_in_long = if meta.long_token_mint == current_token {
                current_token = meta.short_token_mint;
                true
            } else if meta.short_token_mint == current_token {
                current_token = meta.long_token_mint;
                false
            } else {
                return Err(crate::Error::custom(format!(
                    "[swap] invalid swap step. Current step: {market_token}"
                )));
            };
            let mut prices = self.get_prices(meta).ok_or_else(|| {
                crate::Error::custom(format!("[swap] prices for {market_token} are not ready"))
            })?;
            (price_updater)(meta, &mut prices)?;
            let report = market.swap(is_token_in_long, amount, prices)?.execute()?;
            amount = *report.token_out_amount();
            reports.push(report);
        }

        Ok(SwapOutput {
            output_token: current_token,
            amount,
            reports,
        })
    }

    /// Swap along the provided path.
    pub fn swap_along_path(
        &self,
        path: &[Pubkey],
        source_token: &Pubkey,
        amount: u128,
    ) -> crate::Result<SwapOutput> {
        self.swap_along_path_with_price_updater(path, source_token, amount, |_meta, _prices| Ok(()))
    }

    /// Create a builder for order simulation.
    pub fn simulate_order<'a>(
        &'a self,
        kind: CreateOrderKind,
        params: &'a CreateOrderParams,
        collateral_or_swap_out_token: &'a Pubkey,
    ) -> OrderSimulationBuilderForGraph<'a> {
        OrderSimulation::builder()
            .graph(self)
            .kind(kind)
            .params(params)
            .collateral_or_swap_out_token(collateral_or_swap_out_token)
    }
}

/// Best Swap Paths.
pub struct BestSwapPaths<'a> {
    graph: &'a MarketGraph,
    source: Pubkey,
    distances: Distances,
    predecessors: Predecessors,
    arbitrage_exists: Option<bool>,
}

impl fmt::Debug for BestSwapPaths<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BestSwapPaths")
            .field("source", &self.source)
            .field("distances", &self.distances)
            .field("predecessors", &self.predecessors)
            .field("arbitrage_exists", &self.arbitrage_exists)
            .finish()
    }
}

impl BestSwapPaths<'_> {
    /// Get the source.
    pub fn source(&self) -> &Pubkey {
        &self.source
    }

    /// Get swap estimation params.
    pub fn params(&self) -> &SwapEstimationParams {
        &self.graph.config.swap_estimation_params
    }

    /// Return whether there is an arbitrage opportunity.
    ///
    /// Return `None` if it is unknown.
    pub fn arbitrage_exists(&self) -> Option<bool> {
        self.arbitrage_exists
    }

    /// Get best swap path to the target.
    pub fn to(&self, target: &Pubkey) -> (Option<Decimal>, Vec<Pubkey>) {
        let Self {
            graph,
            distances,
            predecessors,
            source,
            ..
        } = self;

        let Some(target_state) = graph.collateral_tokens.get(target) else {
            return (None, vec![]);
        };

        let target_ix = target_state.ix;
        let target_ix = graph.to_index(target_ix);

        let distance = distances[target_ix];

        if *source == *target {
            return (distance, vec![]);
        }

        let mut path = vec![];
        let mut current = predecessors[target_ix];
        let mut steps = 0;
        while let Some((predecessor, market_token)) = current.as_ref() {
            steps += 1;
            if steps > graph.config.max_steps {
                return (None, vec![]);
            }
            path.push(*market_token);
            current = predecessors[graph.to_index(*predecessor)];
        }

        path.reverse();

        (
            if path.is_empty() {
                // Since `target != source`, an empty path means there's no valid distance.
                None
            } else {
                distance.map(|d| (-d).exp())
            },
            path,
        )
    }
}

/// Swap output.
#[derive(Debug, Clone)]
pub struct SwapOutput {
    output_token: Pubkey,
    amount: u128,
    reports: Vec<SwapReport<u128, i128>>,
}

impl SwapOutput {
    /// Returns the output token.
    pub fn output_token(&self) -> &Pubkey {
        &self.output_token
    }

    /// Returns the output amount.
    pub fn amount(&self) -> u128 {
        self.amount
    }

    /// Returns the swap reports.
    pub fn reports(&self) -> &[SwapReport<u128, i128>] {
        &self.reports
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gmsol_programs::gmsol_store::accounts::Market;
    use petgraph::dot::Dot;

    use crate::{
        constants,
        market_graph::simulation::order::OrderSimulationOutput,
        utils::{test::setup_fmt_tracing, zero_copy::try_deserialize_zero_copy_from_base64},
    };

    use super::*;

    const USDC: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    const WSOL: &str = "So11111111111111111111111111111111111111112";
    const BOME: &str = "ukHH6c7mMyiWCf1b9pnWe25TSpkDDt3H5pQZgZ74J82";

    fn get_market_updates() -> Vec<(String, u64)> {
        const DATA: &str = include_str!("test_data/markets.csv");
        DATA.trim()
            .split('\n')
            .enumerate()
            .map(|(idx, data)| {
                let mut data = data.split(',');
                let _market_token = data
                    .next()
                    .unwrap_or_else(|| panic!("[{idx}] missing market_token"));
                let market = data
                    .next()
                    .unwrap_or_else(|| panic!("[{idx}] missing market data"));
                let supply = data
                    .next()
                    .unwrap_or_else(|| panic!("[{idx}] missing supply"));

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

    fn create_and_update_market_graph() -> crate::Result<(MarketGraph, HashSet<Pubkey>)> {
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

        Ok((graph, market_tokens))
    }

    #[test]
    fn basic() -> crate::Result<()> {
        let _tracing = setup_fmt_tracing("info");

        let (mut graph, market_tokens) = create_and_update_market_graph()?;

        // Update value.
        graph.update_value(10 * constants::MARKET_USD_UNIT);
        graph.update_base_cost(constants::MARKET_USD_UNIT / 100);

        let num_markets = graph.markets().count();
        assert_eq!(num_markets, market_tokens.len());
        for market_token in market_tokens {
            let market = graph.get_market(&market_token).expect("must exist");
            assert_eq!(market.meta.market_token_mint, market_token);
        }
        println!("{:?}", Dot::new(&graph.graph));
        Ok(())
    }

    #[test]
    fn best_swap_path() -> crate::Result<()> {
        let _tracing = setup_fmt_tracing("info");

        let usdc: Pubkey = USDC.parse().unwrap();
        let wsol: Pubkey = WSOL.parse().unwrap();
        let bome: Pubkey = BOME.parse().unwrap();

        let (mut g, _) = create_and_update_market_graph()?;

        g.update_value(constants::MARKET_USD_UNIT);

        for steps in 0..=5 {
            g.update_max_steps(steps);

            let paths = g.best_swap_paths(&wsol, false)?;
            let dfs_paths = g.best_swap_paths(&wsol, true)?;

            let (rate, best_path) = paths.to(&bome);
            let (dfs_rate, dfs_best_path) = dfs_paths.to(&bome);
            assert_eq!(rate, dfs_rate);
            assert_eq!(best_path, dfs_best_path);

            let (rate, best_path) = paths.to(&usdc);
            let (dfs_rate, dfs_best_path) = dfs_paths.to(&usdc);
            assert_eq!(rate, dfs_rate);
            assert_eq!(best_path, dfs_best_path);
        }

        Ok(())
    }

    #[test]
    fn position_order_simulation() -> crate::Result<()> {
        let _tracing = setup_fmt_tracing("info");

        let bome: Pubkey = BOME.parse().unwrap();
        let wsol: Pubkey = WSOL.parse().unwrap();
        let market_token: Pubkey = "BwN2FWixP5JyKjJNyD1YcRKN1XhgvFtnzrPrkfyb4DkW"
            .parse()
            .unwrap();

        let (mut g, _) = create_and_update_market_graph()?;

        g.update_value(constants::MARKET_USD_UNIT * 6);
        g.update_max_steps(5);

        let paths = g.best_swap_paths(&bome, false)?;
        let (_, best_path) = paths.to(&wsol);

        println!("{best_path:?}");

        let bome_price = 101468850000;
        let sol_price = 10821227000000;
        let amount = 5 * constants::MARKET_USD_UNIT / bome_price;
        let size = 100 * constants::MARKET_USD_UNIT;
        let params = CreateOrderParams::builder()
            .amount(amount)
            .is_long(true)
            .size(size)
            .market_token(market_token)
            .build();
        let output = g
            .simulate_order(CreateOrderKind::MarketIncrease, &params, &wsol)
            .pay_token(Some(&bome))
            .swap_path(&best_path)
            .build()
            .execute_with_options(Default::default())?;

        println!("{output:?}");

        g.update_value(constants::MARKET_USD_UNIT * 5);
        let paths = g.best_swap_paths(&wsol, false)?;
        let (_, best_path) = paths.to(&bome);
        println!("{best_path:?}");
        let size = 50 * constants::MARKET_USD_UNIT;
        let params = CreateOrderParams::builder()
            .amount(amount)
            .is_long(true)
            .size(size)
            .market_token(market_token)
            .trigger_price(sol_price * 101 / 100)
            .build();
        let OrderSimulationOutput::Increase { position, .. } = output else {
            unreachable!()
        };
        let output = g
            .simulate_order(CreateOrderKind::LimitDecrease, &params, &wsol)
            .receive_token(Some(&bome))
            .swap_path(&best_path)
            .position(Some(position.position_arc()))
            .build()
            .execute_with_options(Default::default());

        println!("{output:?}");

        Ok(())
    }

    #[test]
    fn swap_order_simulation() -> crate::Result<()> {
        let _tracing = setup_fmt_tracing("info");

        let bome: Pubkey = BOME.parse().unwrap();
        let wsol: Pubkey = WSOL.parse().unwrap();
        let market_token: Pubkey = "BwN2FWixP5JyKjJNyD1YcRKN1XhgvFtnzrPrkfyb4DkW"
            .parse()
            .unwrap();

        let (mut g, _) = create_and_update_market_graph()?;

        g.update_value(constants::MARKET_USD_UNIT * 6);
        g.update_max_steps(5);

        let paths = g.best_swap_paths(&bome, false)?;
        let (_, best_path) = paths.to(&wsol);

        println!("{best_path:?}");

        let bome_price = 101468850000;
        let sol_price = 10821227000000;
        let amount = 5 * constants::MARKET_USD_UNIT / bome_price;
        let params = CreateOrderParams::builder()
            .amount(amount)
            .is_long(true)
            .size(0)
            .market_token(market_token)
            .build();
        let output = g
            .simulate_order(CreateOrderKind::MarketSwap, &params, &wsol)
            .pay_token(Some(&bome))
            .swap_path(&best_path)
            .build()
            .execute_with_options(Default::default())?;

        println!("{output:?}");

        let amount = 5 * constants::MARKET_USD_UNIT / bome_price;
        let params = CreateOrderParams::builder()
            .amount(amount)
            .is_long(true)
            .size(0)
            .market_token(market_token)
            .min_output(amount * bome_price / sol_price * 102 / 100)
            .build();
        let output = g
            .simulate_order(CreateOrderKind::LimitSwap, &params, &wsol)
            .pay_token(Some(&bome))
            .swap_path(&best_path)
            .build()
            .execute_with_options(Default::default())?;

        println!("{output:?}");

        Ok(())
    }
}
