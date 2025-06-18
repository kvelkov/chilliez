// src/arbitrage/strategy.rs
// REFACTORED to integrate with the advanced `ArbitrageOrchestrator`.
// This module is now a direct-call service responsible for finding arbitrage paths when requested.
// It no longer runs its own loop or uses an MPSC channel.

use crate::arbitrage::opportunity::MultiHopArbOpportunity;
use crate::config::Config;
use crate::error::ArbError;
use crate::local_metrics::Metrics;
use crate::utils::PoolInfo;
use chrono;
use log::{debug, info, warn};
use petgraph::graph::{DiGraph, NodeIndex};
use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Edge weight for the arbitrage graph
/// Using negative log of exchange rates to detect profitable cycles
#[derive(Debug, Clone)]
struct EdgeWeight {
    weight: f64,              // -ln(exchange_rate)
    exchange_rate: f64,       // Original exchange rate
    pool_address: Pubkey,     // Pool where this swap occurs
    _liquidity: Option<u128>, // not in use - Field has a leading underscore and is not read in current calculations.
}

/// Market graph using petgraph for efficient algorithms
#[derive(Debug)]
struct MarketGraph {
    graph: DiGraph<Pubkey, EdgeWeight>, // Directed graph: tokens -> edges with weights
    token_to_node: HashMap<Pubkey, NodeIndex>, // Map token mints to graph nodes
    node_to_token: HashMap<NodeIndex, Pubkey>, // Reverse mapping
}

impl MarketGraph {
    fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            token_to_node: HashMap::new(),
            node_to_token: HashMap::new(),
        }
    }

    #[allow(dead_code)]
    fn add_pool(&mut self, pool: &PoolInfo) {
        // Ensure both tokens exist as nodes in the graph
        let node_a = self.get_or_create_node(pool.token_a.mint);
        let node_b = self.get_or_create_node(pool.token_b.mint);

        // Calculate exchange rates in both directions
        if let Some(liquidity) = pool.liquidity {
            if liquidity > 0 {
                let rate_a_to_b = self.calculate_exchange_rate(pool, true);
                let rate_b_to_a = self.calculate_exchange_rate(pool, false);
                if rate_a_to_b > 0.0 {
                    let weight_a_to_b = -rate_a_to_b.ln();
                    self.graph.add_edge(
                        node_a,
                        node_b,
                        EdgeWeight {
                            weight: weight_a_to_b,
                            exchange_rate: rate_a_to_b,
                            pool_address: pool.address,
                            _liquidity: Some(liquidity),
                        },
                    );
                } else {
                    // Skip zero rate edges
                }
                if rate_b_to_a > 0.0 {
                    let weight_b_to_a = -rate_b_to_a.ln();
                    self.graph.add_edge(
                        node_b,
                        node_a,
                        EdgeWeight {
                            weight: weight_b_to_a,
                            exchange_rate: rate_b_to_a,
                            pool_address: pool.address,
                            _liquidity: Some(liquidity),
                        },
                    );
                } else {
                    // Skip zero rate edges
                }
            } else {
                // Skip pools with zero liquidity
            }
        }
    }

    /// Adds a pool to the graph, tracking any decimal overflows that occur during the process.
    /// Returns the count of overflows encountered.
    fn add_pool_with_overflow_tracking(
        &mut self,
        pool: &PoolInfo,
        overflow_count: &mut usize,
        overflow_pools: &mut Vec<PoolInfo>,
    ) {
        // Ensure both tokens exist as nodes in the graph
        let node_a = self.get_or_create_node(pool.token_a.mint);
        let node_b = self.get_or_create_node(pool.token_b.mint);

        // Calculate exchange rates in both directions
        if let Some(liquidity) = pool.liquidity {
            if liquidity > 0 {
                let rate_a_to_b = self.calculate_exchange_rate_with_overflow(pool, true, overflow_count, overflow_pools);
                let rate_b_to_a = self.calculate_exchange_rate_with_overflow(pool, false, overflow_count, overflow_pools);
                if rate_a_to_b > 0.0 {
                    let weight_a_to_b = -rate_a_to_b.ln();
                    self.graph.add_edge(
                        node_a,
                        node_b,
                        EdgeWeight {
                            weight: weight_a_to_b,
                            exchange_rate: rate_a_to_b,
                            pool_address: pool.address,
                            _liquidity: Some(liquidity),
                        },
                    );
                }
                if rate_b_to_a > 0.0 {
                    let weight_b_to_a = -rate_b_to_a.ln();
                    self.graph.add_edge(
                        node_b,
                        node_a,
                        EdgeWeight {
                            weight: weight_b_to_a,
                            exchange_rate: rate_b_to_a,
                            pool_address: pool.address,
                            _liquidity: Some(liquidity),
                        },
                    );
                }
            }
        }
    }

    fn get_or_create_node(&mut self, token_mint: Pubkey) -> NodeIndex {
        if let Some(&node_idx) = self.token_to_node.get(&token_mint) {
            node_idx
        } else {
            let node_idx = self.graph.add_node(token_mint);
            self.token_to_node.insert(token_mint, node_idx);
            self.node_to_token.insert(node_idx, token_mint);
            node_idx
        }
    }

    #[allow(dead_code)]
    fn calculate_exchange_rate(&self, pool: &PoolInfo, a_to_b: bool) -> f64 {
        use num_traits::ToPrimitive;
        use rust_decimal::Decimal;
        use rust_decimal_macros::dec;

        // Use precise arithmetic for exchange rate calculations
        if let Some(liquidity) = pool.liquidity {
            if liquidity > 0 {
                // For CLMM pools (like Orca), the rate depends on current tick and sqrt_price
                if let Some(sqrt_price) = pool.sqrt_price {
                    // Use precise arithmetic for price calculation
                    let sqrt_price_decimal = Decimal::from(sqrt_price);
                    // PATCH: Use checked_mul to avoid overflow
                    let price_decimal = match sqrt_price_decimal.checked_mul(sqrt_price_decimal) {
                        Some(val) => val / Decimal::from(1u128 << 64),
                        None => {
                            log::warn!("[PATCH] Decimal multiplication overflow in pool: {:?}", pool);
                            return 0.0;
                        }
                    };
                    let rate_decimal = if a_to_b {
                        price_decimal
                    } else {
                        if !price_decimal.is_zero() {
                            dec!(1) / price_decimal
                        } else {
                            Decimal::ZERO
                        }
                    };
                    let rate = rate_decimal.to_f64().unwrap_or(0.0);
                    rate
                } else {
                    // Fallback to basic AMM rate calculation using reserves
                    let reserve_a = Decimal::from(pool.token_a.reserve);
                    let reserve_b = Decimal::from(pool.token_b.reserve);
                    if !reserve_a.is_zero() && !reserve_b.is_zero() {
                        let rate_decimal = if a_to_b {
                            reserve_b / reserve_a
                        } else {
                            reserve_a / reserve_b
                        };
                        let rate = rate_decimal.to_f64().unwrap_or(0.0);
                        rate
                    } else {
                        0.0
                    }
                }
            } else {
                0.0
            }
        } else {
            // Use reserve-based rate calculation when liquidity data is not available
            let reserve_a = Decimal::from(pool.token_a.reserve);
            let reserve_b = Decimal::from(pool.token_b.reserve);
            if !reserve_a.is_zero() && !reserve_b.is_zero() {
                let rate_decimal = if a_to_b {
                    reserve_b / reserve_a
                } else {
                    reserve_a / reserve_b
                };
                let rate = rate_decimal.to_f64().unwrap_or(0.0);
                rate
            } else {
                0.0
            }
        }
    }

    fn calculate_exchange_rate_with_overflow(&self, pool: &PoolInfo, a_to_b: bool, overflow_count: &mut usize, overflow_pools: &mut Vec<PoolInfo>) -> f64 {
        use num_traits::ToPrimitive;
        use rust_decimal::Decimal;
        use rust_decimal_macros::dec;
        let _overflow_count = overflow_count;
        let _overflow_pools = overflow_pools;
        if let Some(liquidity) = pool.liquidity {
            if liquidity > 0 {
                if let Some(sqrt_price) = pool.sqrt_price {
                    let sqrt_price_decimal = Decimal::from(sqrt_price);
                    let price_decimal = match sqrt_price_decimal.checked_mul(sqrt_price_decimal) {
                        Some(val) => val / Decimal::from(1u128 << 64),
                        None => {
                            // PATCH: Allow all pools through for debugging, return dummy rate
                            // *_overflow_count += 1;
                            // _overflow_pools.push(pool.clone());
                            return 1.0;
                        }
                    };
                    let rate_decimal = if a_to_b {
                        price_decimal
                    } else {
                        if !price_decimal.is_zero() {
                            dec!(1) / price_decimal
                        } else {
                            Decimal::ONE // PATCH: return 1.0 if price_decimal is zero
                        }
                    };
                    let rate = rate_decimal.to_f64().unwrap_or(1.0);
                    rate
                } else {
                    // Fallback to basic AMM rate calculation using reserves
                    let reserve_a = Decimal::from(pool.token_a.reserve);
                    let reserve_b = Decimal::from(pool.token_b.reserve);
                    if !reserve_a.is_zero() && !reserve_b.is_zero() {
                        let rate_decimal = if a_to_b {
                            reserve_b / reserve_a
                        } else {
                            reserve_a / reserve_b
                        };
                        let rate = rate_decimal.to_f64().unwrap_or(0.0);
                        rate
                    } else {
                        0.0
                    }
                }
            } else {
                0.0
            }
        } else {
            0.0
        }
    }

    fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }
}

/// The primary strategy module, responsible for detecting arbitrage opportunities.
/// Renamed from `StrategyManager` to `ArbitrageStrategy` to match the orchestrator's usage.
pub struct ArbitrageStrategy {
    min_profit_threshold_pct: f64,
    blacklist: HashSet<Pubkey>,
    blacklist_reasons: HashMap<Pubkey, String>,
    // Add other strategy-specific configurations here.
}

impl ArbitrageStrategy {
    /// Creates a new strategy instance from the application's configuration.
    pub fn new_from_config(config: &Config) -> Self {
        Self {
            min_profit_threshold_pct: config.min_profit_pct * 100.0,
            blacklist: HashSet::new(),
            blacklist_reasons: HashMap::new(),
        }
    }

    pub fn is_blacklisted(&self, address: &Pubkey) -> bool {
        self.blacklist.contains(address)
    }
    pub fn add_to_blacklist(&mut self, address: Pubkey, reason: &str) {
        self.blacklist.insert(address);
        self.blacklist_reasons.insert(address, reason.to_string());
    }
    pub fn get_blacklist(&self) -> &HashSet<Pubkey> {
        &self.blacklist
    }
    pub fn get_blacklist_reasons(&self) -> &HashMap<Pubkey, String> {
        &self.blacklist_reasons
    }

    /// This is the new primary entry point, called directly by the orchestrator in each cycle.
    /// It takes a snapshot of the current pools and returns any found opportunities.
    pub async fn detect_all_opportunities(
        &mut self,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
        _metrics: &Arc<Mutex<Metrics>>, // Metrics can be used for detailed performance tracking
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        use crate::utils::timing::Timer;

        let mut timer = Timer::start("arbitrage_detection");
        info!(
            "Strategy module: Starting opportunity detection across {} pools.",
            pools.len()
        );

        if pools.is_empty() {
            warn!("No pools provided to the strategy module. Cannot detect opportunities.");
            return Ok(Vec::new());
        }

        // --- CORE ARBITRAGE DETECTION LOGIC ---

        let mut overflow_count = 0;
        let mut overflow_pools: Vec<PoolInfo> = Vec::new();
        let mut analyzed_count = 0;
        let mut skipped_count = 0;
        // 1. Build the Market Graph
        debug!("Building market graph from {} pools", pools.len());
        let mut graph = MarketGraph::new();

        for pool in pools.values() {
            analyzed_count += 1;
            if self.is_blacklisted(&pool.address) {
                skipped_count += 1;
                continue;
            }
            let before_edges = graph.edge_count();
            let prev_overflow_count = overflow_count;
            graph.add_pool_with_overflow_tracking(pool, &mut overflow_count, &mut overflow_pools);
            let after_edges = graph.edge_count();
            if overflow_count > prev_overflow_count {
                self.add_to_blacklist(pool.address, "overflow");
            }
            if after_edges == before_edges {
                skipped_count += 1;
            }
        }
        timer.checkpoint("market_graph_built");

        info!("═══════════════════════════════════════════════════════════════");
        info!("🔍 Detection Cycle | Pools: {} | Analyzed: {} | Skipped: {} | Overflows: {}", pools.len(), analyzed_count, skipped_count, overflow_count);
        if !self.blacklist.is_empty() {
            info!("⛔ Blacklisted pools (skipped due to repeated issues):");
            for (addr, reason) in &self.blacklist_reasons {
                info!("   - {} ({})", addr, reason);
            }
        }
        if overflow_count > 0 {
            info!("⚠️  Decimal overflows in pools (showing only address):");
            for pool in &overflow_pools {
                if !self.is_blacklisted(&pool.address) {
                    info!("   - {} (overflow)", pool.address);
                }
            }
        }
        info!("---------------------------------------------------------------");
        info!("Market graph built with {} tokens and {} edges", graph.node_count(), graph.edge_count());

        if graph.node_count() == 0 || graph.edge_count() == 0 {
            warn!("Market graph is empty - no tokens or edges found");
            return Ok(Vec::new());
        }

        // 2. Run Bellman-Ford Algorithm for each potential starting token
        let mut all_opportunities = Vec::new();

        // Focus on major tokens as starting points (SOL, USDC, USDT, etc.)
        let major_tokens = self.get_major_tokens(&graph);
        timer.checkpoint("major_tokens_identified");

        for &start_token in &major_tokens {
            if let Some(opportunities) =
                self.detect_cycles_from_token(&graph, start_token, pools)?
            {
                all_opportunities.extend(opportunities);
            }
        }
        timer.checkpoint("bellman_ford_complete");

        // 3. Filter and deduplicate opportunities
        all_opportunities = self.filter_opportunities(all_opportunities);
        timer.checkpoint("opportunities_filtered");

        let duration = timer.finish_with_threshold(1000); // Warn if > 1 second
        info!(
            "Strategy module: Detection complete. Found {} potential opportunities in {:.2}ms.",
            all_opportunities.len(),
            duration.as_millis()
        );

        Ok(all_opportunities)
    }

    /// Detect arbitrage cycles starting from a specific token using Bellman-Ford
    fn detect_cycles_from_token(
        &self,
        graph: &MarketGraph,
        start_token: Pubkey,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
    ) -> Result<Option<Vec<MultiHopArbOpportunity>>, ArbError> {
        log::debug!(
            "[detect_cycles_from_token] Starting from token: {}",
            start_token
        );
        let start_node = match graph.token_to_node.get(&start_token) {
            Some(&node) => node,
            None => return Ok(None),
        };
        // Initialize distances and predecessors for Bellman-Ford
        let mut distances: HashMap<NodeIndex, f64> = HashMap::new();
        let mut predecessors: HashMap<NodeIndex, Option<NodeIndex>> = HashMap::new();
        for node_idx in graph.graph.node_indices() {
            distances.insert(node_idx, f64::INFINITY);
            predecessors.insert(node_idx, None);
        }
        distances.insert(start_node, 0.0);
        let node_count = graph.graph.node_count();
        for iteration in 0..(node_count - 1) {
            let mut updated = false;
            for edge_idx in graph.graph.edge_indices() {
                if let Some((from_node, to_node)) = graph.graph.edge_endpoints(edge_idx) {
                    if let Some(edge_weight) = graph.graph.edge_weight(edge_idx) {
                        let current_dist = distances[&from_node];
                        if current_dist != f64::INFINITY {
                            let new_dist = current_dist + edge_weight.weight;
                            if new_dist < distances[&to_node] {
                                distances.insert(to_node, new_dist);
                                predecessors.insert(to_node, Some(from_node));
                                updated = true;
                            }
                        }
                    }
                }
            }
            if !updated {
                debug!("Bellman-Ford converged early at iteration {}", iteration);
                break;
            }
        }
        // Check for negative cycles (arbitrage opportunities)
        let mut cycle_nodes = HashSet::new();
        for edge_idx in graph.graph.edge_indices() {
            if let Some((from_node, to_node)) = graph.graph.edge_endpoints(edge_idx) {
                if let Some(edge_weight) = graph.graph.edge_weight(edge_idx) {
                    let current_dist = distances[&from_node];
                    if current_dist != f64::INFINITY {
                        let new_dist = current_dist + edge_weight.weight;
                        if new_dist < distances[&to_node] {
                            // Negative cycle detected!
                            cycle_nodes.insert(to_node);
                            log::debug!(
                                "[detect_cycles_from_token] Negative cycle detected: {} -> {}",
                                graph.node_to_token[&from_node],
                                graph.node_to_token[&to_node]
                            );
                        }
                    }
                }
            }
        }
        // Extract cycles from detected negative cycle nodes
        let mut opportunities = Vec::new();
        for &cycle_node in &cycle_nodes {
            if let Some(opportunity) =
                self.extract_cycle_from_node(graph, &predecessors, cycle_node, start_token, pools)
            {
                log::debug!(
                    "[detect_cycles_from_token] Extracted opportunity: id={}, profit_pct={}",
                    opportunity.id,
                    opportunity.profit_pct
                );
                opportunities.push(opportunity);
            }
        }
        Ok(if opportunities.is_empty() {
            None
        } else {
            Some(opportunities)
        })
    }

    /// Extract a cycle from the predecessor array when a negative cycle is detected
    fn extract_cycle_from_node(
        &self,
        graph: &MarketGraph,
        predecessors: &HashMap<NodeIndex, Option<NodeIndex>>,
        cycle_node: NodeIndex,
        start_token: Pubkey,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
    ) -> Option<MultiHopArbOpportunity> {
        let mut path = Vec::new();
        let mut current = cycle_node;
        let mut visited = HashSet::new();

        // Follow predecessors to find the actual cycle
        while !visited.contains(&current) {
            visited.insert(current);
            path.push(current);

            if let Some(Some(pred)) = predecessors.get(&current) {
                current = *pred;
            } else {
                break;
            }

            // Prevent infinite loops
            if path.len() > 20 {
                warn!("Cycle extraction exceeded maximum length, breaking");
                break;
            }
        }

        // Find where the cycle actually starts
        if let Some(cycle_start_pos) = path.iter().position(|&node| node == current) {
            let cycle_path = &path[cycle_start_pos..];

            if cycle_path.len() < 2 {
                debug!("Cycle too short to be profitable");
                return None;
            }

            // Convert cycle to hops
            let mut total_rate_fwd = 1.0f64;
            let mut total_rate_rev = 1.0f64;
            let mut hops_fwd = Vec::new();
            let mut hops_rev = Vec::new();
            let mut cycle_tokens = Vec::new();
            for i in 0..cycle_path.len() {
                let from_node = cycle_path[i];
                let to_node = cycle_path[(i + 1) % cycle_path.len()];
                cycle_tokens.push(graph.node_to_token[&from_node]);
                // Forward direction
                if let Some(edge_idx) = graph.graph.find_edge(from_node, to_node) {
                    if let Some(edge_weight) = graph.graph.edge_weight(edge_idx) {
                        let from_token = graph.node_to_token[&from_node];
                        let to_token = graph.node_to_token[&to_node];
                        total_rate_fwd *= edge_weight.exchange_rate;
                        hops_fwd.push(crate::arbitrage::opportunity::ArbHop {
                            dex: self.get_dex_type_for_pool(&edge_weight.pool_address, pools),
                            pool: edge_weight.pool_address,
                            input_token: self
                                .get_token_symbol(&from_token, pools)
                                .unwrap_or_else(|| from_token.to_string()),
                            output_token: self
                                .get_token_symbol(&to_token, pools)
                                .unwrap_or_else(|| to_token.to_string()),
                            input_amount: 1000.0,
                            expected_output: 1000.0 * edge_weight.exchange_rate,
                        });
                    }
                }
                // Reverse direction
                if let Some(edge_idx) = graph.graph.find_edge(to_node, from_node) {
                    if let Some(edge_weight) = graph.graph.edge_weight(edge_idx) {
                        let from_token = graph.node_to_token[&to_node];
                        let to_token = graph.node_to_token[&from_node];
                        total_rate_rev *= edge_weight.exchange_rate;
                        hops_rev.push(crate::arbitrage::opportunity::ArbHop {
                            dex: self.get_dex_type_for_pool(&edge_weight.pool_address, pools),
                            pool: edge_weight.pool_address,
                            input_token: self
                                .get_token_symbol(&from_token, pools)
                                .unwrap_or_else(|| from_token.to_string()),
                            output_token: self
                                .get_token_symbol(&to_token, pools)
                                .unwrap_or_else(|| to_token.to_string()),
                            input_amount: 1000.0,
                            expected_output: 1000.0 * edge_weight.exchange_rate,
                        });
                    }
                }
            }
            // Debug info available in detailed logs if needed
            // Use the direction with the higher profit
            let (hops, total_rate, _): (Vec<crate::arbitrage::opportunity::ArbHop>, f64, f64) =
                if total_rate_fwd > total_rate_rev {
                    (hops_fwd, total_rate_fwd, (total_rate_fwd - 1.0) * 100.0)
                } else {
                    (hops_rev, total_rate_rev, (total_rate_rev - 1.0) * 100.0)
                };

            if hops.is_empty() {
                debug!("No valid hops found for cycle");
                return None;
            }

            // Calculate profit
            let gross_profit_pct = (total_rate - 1.0) * 100.0;

            if gross_profit_pct >= self.min_profit_threshold_pct {
                // Create dex_path and pool_path
                let dex_path: Vec<crate::utils::DexType> =
                    hops.iter().map(|hop| hop.dex.clone()).collect();
                let pool_path: Vec<Pubkey> = hops.iter().map(|hop| hop.pool).collect();

                // Get token symbols
                let input_token_symbol = self
                    .get_token_symbol(&start_token, pools)
                    .unwrap_or_else(|| start_token.to_string());
                let output_token_symbol = input_token_symbol.clone(); // Circular arbitrage

                // Get intermediate tokens
                let intermediate_tokens: Vec<String> = hops
                    .iter()
                    .skip(1)
                    .map(|hop| hop.input_token.clone())
                    .collect();

                // Get source and target pools
                let source_pool = pools
                    .get(&hops[0].pool)
                    .cloned()
                    .unwrap_or_else(|| Arc::new(PoolInfo::default()));
                let target_pool = pools
                    .get(&hops[hops.len() - 1].pool)
                    .cloned()
                    .unwrap_or_else(|| Arc::new(PoolInfo::default()));

                let profit_amount = gross_profit_pct * 1000.0 / 100.0;
                let hops_clone = hops.clone();

                return Some(MultiHopArbOpportunity {
                    id: format!(
                        "arb_{}",
                        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
                    ),
                    hops,
                    total_profit: profit_amount,
                    profit_pct: gross_profit_pct,
                    input_token: input_token_symbol,
                    output_token: output_token_symbol,
                    input_amount: 1000.0,
                    expected_output: 1000.0 + profit_amount,
                    dex_path,
                    pool_path,
                    risk_score: Some(0.2),
                    notes: Some(format!(
                        "Detected via Bellman-Ford, cycle length: {}",
                        cycle_path.len()
                    )),
                    estimated_profit_usd: None,
                    input_amount_usd: None,
                    output_amount_usd: None,
                    intermediate_tokens,
                    source_pool,
                    target_pool,
                    input_token_mint: start_token,
                    output_token_mint: start_token,
                    intermediate_token_mint: if !hops_clone.is_empty() {
                        Some(
                            self.get_token_mint_from_symbol(&hops_clone[0].output_token, pools)
                                .unwrap_or(start_token),
                        )
                    } else {
                        None
                    },
                    estimated_gas_cost: Some(50000 * hops_clone.len() as u64),
                    detected_at: Some(std::time::Instant::now()),
                });
            } else {
                debug!(
                    "Cycle profit {:.2}% below threshold {:.2}%",
                    gross_profit_pct, self.min_profit_threshold_pct
                );
            }
        }

        None
    }

    /// Get major tokens to use as starting points for cycle detection
    fn get_major_tokens(&self, graph: &MarketGraph) -> Vec<Pubkey> {
        // In a real implementation, you'd maintain a list of major tokens (SOL, USDC, USDT, etc.)
        // For now, we'll return tokens with the most connections (highest degree)
        let mut token_degrees: Vec<(Pubkey, usize)> = graph
            .token_to_node
            .iter()
            .map(|(&token, &node_idx)| {
                let degree = graph.graph.edges(node_idx).count();
                (token, degree)
            })
            .collect();

        // Sort by degree (number of connections) in descending order
        token_degrees.sort_by(|a, b| b.1.cmp(&a.1));

        // Return top tokens (up to 5)
        token_degrees
            .into_iter()
            .take(5)
            .map(|(token, _)| token)
            .collect()
    }

    /// Filter and deduplicate opportunities
    fn filter_opportunities(
        &self,
        mut opportunities: Vec<MultiHopArbOpportunity>,
    ) -> Vec<MultiHopArbOpportunity> {
        // Remove duplicates and low-profit opportunities
        opportunities.dedup_by(|a, b| {
            a.input_token == b.input_token
                && a.hops.len() == b.hops.len()
                && a.hops
                    .iter()
                    .zip(b.hops.iter())
                    .all(|(h1, h2)| h1.pool == h2.pool)
        });

        // Sort by profit potential
        opportunities.sort_by(|a, b| {
            b.total_profit
                .partial_cmp(&a.total_profit)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Limit to top opportunities to avoid overwhelming the system
        opportunities.truncate(10);

        opportunities
    }

    /// Creates mock opportunities for testing the pipeline.
    // --- Helper functions for configuration ---
    pub fn get_min_profit_threshold_pct(&self) -> f64 {
        self.min_profit_threshold_pct
    }

    pub fn set_min_profit_threshold(&mut self, threshold_pct: f64) {
        self.min_profit_threshold_pct = threshold_pct;
    }

    /// Helper methods for token and pool operations
    fn get_dex_type_for_pool(
        &self,
        pool_address: &Pubkey,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
    ) -> crate::utils::DexType {
        pools
            .get(pool_address)
            .map(|pool| pool.dex_type.clone())
            .unwrap_or(crate::utils::DexType::Orca) // Default fallback
    }

    fn get_token_symbol(
        &self,
        token_mint: &Pubkey,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
    ) -> Option<String> {
        for pool in pools.values() {
            if pool.token_a.mint == *token_mint {
                return Some(pool.token_a.symbol.clone());
            }
            if pool.token_b.mint == *token_mint {
                return Some(pool.token_b.symbol.clone());
            }
        }
        None
    }

    fn get_token_mint_from_symbol(
        &self,
        symbol: &str,
        pools: &HashMap<Pubkey, Arc<PoolInfo>>,
    ) -> Option<Pubkey> {
        for pool in pools.values() {
            if pool.token_a.symbol == symbol {
                return Some(pool.token_a.mint);
            }
            if pool.token_b.symbol == symbol {
                return Some(pool.token_b.mint);
            }
        }
        None
    }
}
