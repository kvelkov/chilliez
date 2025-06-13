// src/dex/path_finder.rs

use crate::dex::quoting_engine::QuotingEngineOperations;
use crate::dex::quote::{DexClient, Quote};
use crate::dex::opportunity::{HopInfo, MultiHopArbOpportunity};
use crate::utils::{DexType, PoolInfo, PoolToken}; // Added PoolToken
use anyhow::{anyhow, Context, Result}; // Added anyhow macro
use dashmap::DashMap;
use log::{debug, error, info, warn};
use petgraph::prelude::DiGraphMap; // Changed import for DiGraphMap
use petgraph::algo::find_negative_cycle;
use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

/// Configuration for the arbitrage discovery process.
#[derive(Debug, Clone)]
pub struct ArbitrageDiscoveryConfig {
    pub discovery_interval: Duration,
    pub min_profit_bps_threshold: u32, // Minimum basis points profit to consider an opportunity
    pub max_hops_for_opportunity: usize, // Max hops in a valid opportunity (after translation)
    // pub reference_trade_amount_usd: f64, // For future use
    // pub graph_update_interval: Duration, // This might be separate
}

/// `PathFinder` is responsible for constructing and maintaining a market graph
/// from pool data and using it to find arbitrage opportunities.
#[derive(Clone)]
pub struct PathFinder {
    pool_cache: Arc<DashMap<Pubkey, Arc<PoolInfo>>>,
    quoting_engine: Arc<dyn QuotingEngineOperations + Send + Sync>, // Changed to trait object
    client_map: HashMap<String, Arc<dyn DexClient>>, // For direct DexClient access
    market_graph: Arc<RwLock<DiGraphMap<Pubkey, f64>>>, // Nodes: Token Mint, Edge Weight: -ln(rate)
    opportunity_sender: broadcast::Sender<MultiHopArbOpportunity>,
    config: ArbitrageDiscoveryConfig, // Added config
    last_graph_update_time: Arc<RwLock<SystemTime>>,
}

impl PathFinder {
    /// Creates a new `PathFinder`.
    pub fn new(
        pool_cache: Arc<DashMap<Pubkey, Arc<PoolInfo>>>,
        quoting_engine: Arc<dyn QuotingEngineOperations + Send + Sync>, // Changed to trait object
        dex_clients: Vec<Arc<dyn DexClient>>, // Pass full list of DexClients
        opportunity_sender: broadcast::Sender<MultiHopArbOpportunity>,
        config: ArbitrageDiscoveryConfig,
    ) -> Self {
        let mut client_map = HashMap::new();
        for client in &dex_clients {
            client_map.insert(client.get_name().to_string(), Arc::clone(client));
        }

        info!(
            "PathFinder initialized with {} DEX clients for rate calculation.",
            client_map.len()
        );

        Self {
            pool_cache,
            quoting_engine,
            client_map,
            market_graph: Arc::new(RwLock::new(DiGraphMap::new())),
            opportunity_sender,
            config,
            last_graph_update_time: Arc::new(RwLock::new(SystemTime::UNIX_EPOCH)), // Initialize to a past time
        }
    }

    /// Get a receiver for opportunities discovered by PathFinder
    pub fn subscribe_to_opportunities(&self) -> broadcast::Receiver<MultiHopArbOpportunity> {
        self.opportunity_sender.subscribe()
    }

    /// Updates the in-memory market graph based on the current pool cache.
    /// Nodes are token mints, and edges represent swap rates (-ln(rate)).
    pub async fn update_market_graph(&self) -> Result<()> {
        info!("Starting market graph update...");
        let mut graph = self.market_graph.write().await;
        graph.clear(); // Simple strategy: clear and rebuild.

        let mut edges_added = 0;
        let mut pools_processed = 0;

        for entry in self.pool_cache.iter() {
            let pool_info_arc = entry.value();
            let pool_info = &**pool_info_arc;
            pools_processed += 1;

            // Ensure nodes for both tokens exist
            graph.add_node(pool_info.token_a.mint);
            graph.add_node(pool_info.token_b.mint);

            let dex_type_str = format!("{:?}", pool_info.dex_type);
            let dex_client = match self.client_map.get(&dex_type_str) {
                Some(client) => client,
                None => {
                    warn!("No DexClient found for DEX type {:?} (pool {}) during graph update. Skipping.",
                          pool_info.dex_type, pool_info.address);
                    continue;
                }
            };

            // Calculate rate for A -> B
            let ref_amount_a = 10u64.pow(pool_info.token_a.decimals as u32); // 1 full unit of token A
            if ref_amount_a > 0 {
                match dex_client.calculate_onchain_quote(pool_info, ref_amount_a) {
                    Ok(quote_a_to_b) => {
                        // Ensure the quote is for A -> B direction
                        if quote_a_to_b.input_token == pool_info.token_a.symbol &&
                           quote_a_to_b.output_token == pool_info.token_b.symbol &&
                           quote_a_to_b.output_amount > 0 {
                            let rate_a_to_b = quote_a_to_b.output_amount as f64 / ref_amount_a as f64;
                            if rate_a_to_b > 0.0 {
                                let weight = -rate_a_to_b.ln();
                                graph.add_edge(pool_info.token_a.mint, pool_info.token_b.mint, weight);
                                edges_added += 1;
                                debug!("Graph edge: {} -> {} via {} (Pool: {}), Rate: {:.6}, Weight: {:.6}",
                                       pool_info.token_a.symbol, pool_info.token_b.symbol, dex_client.get_name(), pool_info.address, rate_a_to_b, weight);
                            }
                        }
                    }
                    Err(e) => {
                        debug!("Could not get quote A->B for pool {}: {}", pool_info.address, e);
                    }
                }
            }

            // Calculate rate for B -> A
            let ref_amount_b = 10u64.pow(pool_info.token_b.decimals as u32); // 1 full unit of token B
            if ref_amount_b > 0 {
                 match dex_client.calculate_onchain_quote(pool_info, ref_amount_b) {
                    Ok(quote_b_to_a) => {
                        // Ensure the quote is for B -> A direction
                        if quote_b_to_a.input_token == pool_info.token_b.symbol &&
                           quote_b_to_a.output_token == pool_info.token_a.symbol &&
                           quote_b_to_a.output_amount > 0 {
                            let rate_b_to_a = quote_b_to_a.output_amount as f64 / ref_amount_b as f64;
                            if rate_b_to_a > 0.0 {
                                let weight = -rate_b_to_a.ln();
                                graph.add_edge(pool_info.token_b.mint, pool_info.token_a.mint, weight);
                                edges_added += 1;
                                debug!("Graph edge: {} -> {} via {} (Pool: {}), Rate: {:.6}, Weight: {:.6}",
                                       pool_info.token_b.symbol, pool_info.token_a.symbol, dex_client.get_name(), pool_info.address, rate_b_to_a, weight);
                            }
                        }
                    }
                    Err(e) => {
                        debug!("Could not get quote B->A for pool {}: {}", pool_info.address, e);
                    }
                }
            }
        }

        let mut last_update_w = self.last_graph_update_time.write().await;
        *last_update_w = SystemTime::now();
        
        info!("Market graph update complete. Processed {} pools. Nodes: {}, Edges: {}. Last updated: {:?}",
              pools_processed, graph.node_count(), graph.edge_count(), *last_update_w);
        Ok(())
    }

    /// Runs a persistent task that periodically updates the market graph.
    pub async fn run_graph_updater_task(self: Arc<Self>, update_interval: Duration) {
        info!("Starting graph updater task with interval {:?}", update_interval);
        let mut interval = tokio::time::interval(update_interval);
        loop {
            interval.tick().await;
            if let Err(e) = self.update_market_graph().await {
                error!("Error updating market graph: {}", e);
            }
        }
    }

    /// Runs a persistent task that periodically discovers and broadcasts arbitrage opportunities.
    pub async fn run_arbitrage_discovery_task(self: Arc<Self>) {
        info!(
            "Starting arbitrage discovery task with interval {:?}",
            self.config.discovery_interval
        );
        let mut interval = tokio::time::interval(self.config.discovery_interval);
        loop {
            interval.tick().await;
            if let Err(e) = self.detect_and_broadcast_opportunities().await {
                error!("Error during arbitrage detection: {}", e);
            }
        }
    }

    /// Detects arbitrage opportunities using Bellman-Ford on the market graph and broadcasts them.
    async fn detect_and_broadcast_opportunities(&self) -> Result<()> {
        debug!("Running arbitrage detection cycle...");
        let graph_lock = self.market_graph.read().await;
        if graph_lock.node_count() == 0 || graph_lock.edge_count() == 0 {
            debug!("Market graph is empty. Skipping arbitrage detection.");
            return Ok(());
        }

        let nodes: Vec<Pubkey> = graph_lock.nodes().collect();
        let mut processed_normalized_cycles = HashSet::new();
        let mut opportunities_found_this_cycle = 0;

        for start_node_candidate in nodes {
            if let Some(cycle_nodes) = find_negative_cycle(&*graph_lock, start_node_candidate) {
                if cycle_nodes.len() <= 1 || cycle_nodes.first() != cycle_nodes.last() {
                    // find_negative_cycle should return A -> B -> C -> A. If not, skip.
                    // Also, ensure it's not just a self-loop if that's not desired.
                    debug!("Invalid or trivial cycle found: {:?}", cycle_nodes);
                    continue;
                }

                // Normalize the cycle to handle duplicates
                let normalized_cycle_key = Self::normalize_cycle_path(&cycle_nodes);
                if processed_normalized_cycles.contains(&normalized_cycle_key) {
                    debug!("Duplicate cycle detected and skipped: {:?}", cycle_nodes);
                    continue;
                }
                processed_normalized_cycles.insert(normalized_cycle_key);

                debug!("Negative cycle detected: {:?}", cycle_nodes);
                match self.translate_cycle_to_opportunity(cycle_nodes.clone()).await {
                    Ok(Some(opportunity)) => {
                        info!("Profitable arbitrage opportunity found: ID {} with {} BPS profit", opportunity.id, opportunity.profit_bps);
                        if self.opportunity_sender.send(opportunity.clone()).is_err() {
                            warn!("No active subscribers for arbitrage opportunities. Opportunity ID: {} dropped.", opportunity.id);
                        }
                        opportunities_found_this_cycle += 1;
                    }
                    Ok(None) => {
                        debug!("Detected cycle {:?} did not translate to a profitable opportunity after detailed quoting.", cycle_nodes);
                    }
                    Err(e) => {
                        error!("Error translating cycle {:?} to opportunity: {}", cycle_nodes, e);
                    }
                }
            }
        }
        if opportunities_found_this_cycle > 0 {
            info!("Arbitrage detection cycle finished. Found {} new opportunities.", opportunities_found_this_cycle);
        } else {
            debug!("Arbitrage detection cycle finished. No new opportunities found.");
        }
        Ok(())
    }

    /// Normalizes a cycle path to its canonical representation.
    fn normalize_cycle_path(path: &[Pubkey]) -> String {
        if path.is_empty() || path.len() == 1 { // Path should end where it starts
            return path.iter().map(|p| p.to_string()).collect::<Vec<_>>().join("-");
        }
        // Remove the last element if it's same as first, for canonical representation of the cycle itself
        let mut unique_nodes_in_cycle = path.to_vec();
        if unique_nodes_in_cycle.first() == unique_nodes_in_cycle.last() && unique_nodes_in_cycle.len() > 1 {
            unique_nodes_in_cycle.pop();
        }

        let min_node = unique_nodes_in_cycle.iter().min_by_key(|p| p.to_string()).unwrap_or(&unique_nodes_in_cycle[0]);
        let start_index = unique_nodes_in_cycle.iter().position(|&p| p == *min_node).unwrap_or(0);
        
        let mut normalized = Vec::with_capacity(unique_nodes_in_cycle.len());
        for i in 0..unique_nodes_in_cycle.len() {
            normalized.push(unique_nodes_in_cycle[(start_index + i) % unique_nodes_in_cycle.len()]);
        }
        // Add the start node again to represent the full cycle for ID generation
        normalized.push(*min_node); 
        normalized.iter().map(|p| p.to_string()).collect::<Vec<_>>().join("-")
    }

    /// Translates a detected cycle of token mints into a detailed `MultiHopArbOpportunity`.
    async fn translate_cycle_to_opportunity(&self, cycle_path: Vec<Pubkey>) -> Result<Option<MultiHopArbOpportunity>> {
        if cycle_path.len() < 3 || cycle_path.first() != cycle_path.last() { // e.g. A->B->A needs 3 nodes
            return Ok(None);
        }
        if cycle_path.len() -1 > self.config.max_hops_for_opportunity {
             debug!("Cycle path length {} exceeds max_hops_for_opportunity {}. Skipping.", cycle_path.len() -1, self.config.max_hops_for_opportunity);
            return Ok(None);
        }

        let start_token_mint = cycle_path[0];
        let mut initial_amount_atomic = 0u64;

        // Find decimals for the start_token_mint
        let mut start_token_decimals: Option<u8> = None;
        for pool_arc_ref in self.pool_cache.iter() {
            let pool = pool_arc_ref.value();
            if pool.token_a.mint == start_token_mint { start_token_decimals = Some(pool.token_a.decimals); break; }
            if pool.token_b.mint == start_token_mint { start_token_decimals = Some(pool.token_b.decimals); break; }
        }
        let decimals = start_token_decimals.ok_or_else(|| anyhow!("Could not find decimals for start token {}", start_token_mint))?;
        initial_amount_atomic = 10u64.pow(decimals as u32); // 1 full unit

        let mut current_amount_atomic = initial_amount_atomic;
        let mut actual_hops: Vec<HopInfo> = Vec::new();

        for i in 0..(cycle_path.len() - 1) {
            let input_hop_mint = cycle_path[i];
            let output_hop_mint = cycle_path[i + 1];

            match self.quoting_engine.calculate_best_quote(input_hop_mint, output_hop_mint, current_amount_atomic).await? {
                Some(quote_for_hop) => {
                    let pool_address = quote_for_hop.route.first().cloned().unwrap_or_default(); // Assuming single pool route from best_quote
                    let dex_type_str = quote_for_hop.dex; // Dex name string from quote
                    // We need to find the PoolInfo to get the DexType enum variant.
                    // This is a bit indirect. Ideally, Quote would carry DexType.
                    let pool_info_for_dex_type = self.pool_cache.get(&pool_address).map(|entry| entry.value().dex_type.clone());
                    
                    actual_hops.push(HopInfo {
                        pool_address,
                        dex_type: pool_info_for_dex_type.unwrap_or(DexType::Unknown(dex_type_str)),
                        input_mint: input_hop_mint,
                        output_mint: output_hop_mint,
                        input_amount: current_amount_atomic,
                        output_amount: quote_for_hop.output_amount,
                    });
                    current_amount_atomic = quote_for_hop.output_amount;
                }
                None => {
                    debug!("Path broken at hop {} -> {}: No quote found for amount {}", input_hop_mint, output_hop_mint, current_amount_atomic);
                    return Ok(None);
                }
            }
        }

        let final_output_amount_atomic = current_amount_atomic;
        if final_output_amount_atomic <= initial_amount_atomic {
            return Ok(None);
        }

        let profit_atomic = final_output_amount_atomic - initial_amount_atomic;
        let profit_bps = (profit_atomic as f64 / initial_amount_atomic as f64 * 10000.0) as u32;

        if profit_bps < self.config.min_profit_bps_threshold {
            debug!("Opportunity profit {} BPS below threshold {}. Path: {:?}", profit_bps, self.config.min_profit_bps_threshold, cycle_path);
            return Ok(None);
        }

        Ok(Some(MultiHopArbOpportunity {
            id: Uuid::new_v4().to_string(),
            hops: actual_hops,
            start_token_mint,
            end_token_mint: cycle_path.last().cloned().unwrap_or_default(), // Should be same as start_token_mint
            initial_input_amount: initial_amount_atomic,
            final_output_amount: final_output_amount_atomic,
            profit_bps,
            estimated_profit_usd: None, // Requires price oracle
            risk_score: None, // Requires risk model
        }))
    }

    /// Starts all background tasks for the PathFinder.
    pub async fn start_services(self: Arc<Self>, graph_update_interval: Duration) {
        let graph_updater_self = Arc::clone(&self);
        tokio::spawn(async move {
            graph_updater_self.run_graph_updater_task(graph_update_interval).await;
        });

        let arbitrage_discoverer_self = Arc::clone(&self);
        tokio::spawn(async move {
            arbitrage_discoverer_self.run_arbitrage_discovery_task().await;
        });
        info!("PathFinder services (graph updater, arbitrage discovery) started.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dex::quoting_engine::{AdvancedQuotingEngine, QuotingEngineOperations}; // Import new trait
    use crate::utils::DexType;
    use crate::dex::quote::Quote;
    use tokio::sync::broadcast;

    // Mock DexClient for PathFinder tests
    #[derive(Clone)]
    struct MockPfDexClient { name: String }
    #[async_trait::async_trait]
    impl DexClient for MockPfDexClient {
        fn get_name(&self) -> &str { &self.name }
        fn calculate_onchain_quote(&self, pool: &PoolInfo, input_amount: u64) -> Result<crate::dex::quote::Quote> {
            // Simple mock: if input is token_a, output token_b with 1.1x rate, if input token_b, output token_a with 0.9x rate
            let (input_sym, output_sym, output_amt) = if input_amount == 10u64.pow(pool.token_a.decimals as u32) {
                (pool.token_a.symbol.clone(), pool.token_b.symbol.clone(), (input_amount as f64 * 1.01) as u64)
            } else if input_amount == 10u64.pow(pool.token_b.decimals as u32) {
                (pool.token_b.symbol.clone(), pool.token_a.symbol.clone(), (input_amount as f64 * 0.99) as u64)
            } else {
                return Err(anyhow::anyhow!("Mock quote for specific input amount not found"));
            };
            Ok(crate::dex::quote::Quote {
                input_token: input_sym, output_token: output_sym, input_amount, output_amount: output_amt,
                dex: self.name.clone(), route: vec![pool.address], slippage_estimate: Some(0.0),
            })
        }
        fn get_swap_instruction(&self, _swap_info: &crate::dex::quote::SwapInfo) -> Result<solana_sdk::instruction::Instruction> { Err(anyhow::anyhow!("mock")) }
        async fn discover_pools(&self) -> Result<Vec<PoolInfo>> { Ok(vec![]) }
    }

    #[tokio::test]
    async fn test_update_market_graph_simple() {
        let pool_cache = Arc::new(DashMap::new());
        let (opp_tx, _opp_rx) = broadcast::channel(10);

        let sol_mint = Pubkey::new_unique();
        let usdc_mint = Pubkey::new_unique();

        let pool1_addr = Pubkey::new_unique();
        let pool1 = Arc::new(PoolInfo {
            address: pool1_addr, name: "SOL-USDC-Raydium".to_string(), dex_type: DexType::Raydium,
            token_a: crate::utils::PoolToken { mint: sol_mint, symbol: "SOL".to_string(), decimals: 9, reserve: 1000 * 10u64.pow(9) },
            token_b: crate::utils::PoolToken { mint: usdc_mint, symbol: "USDC".to_string(), decimals: 6, reserve: 100000 * 10u64.pow(6) },
            ..Default::default()
        });
        pool_cache.insert(pool1_addr, Arc::clone(&pool1));

        let mock_dex_clients: Vec<Arc<dyn DexClient>> = vec![
            Arc::new(MockPfDexClient { name: "Raydium".to_string() }),
        ];
        
        // Mock for QuotingEngineOperations
        struct MockQuotingEngine;
        #[async_trait::async_trait]
        impl QuotingEngineOperations for MockQuotingEngine {
            async fn calculate_best_quote(&self, _input_mint: Pubkey, _output_mint: Pubkey, _amount_in: u64) -> Result<Option<Quote>> {
                // This mock won't be called by update_market_graph directly,
                // as update_market_graph uses its internal client_map and DexClient trait.
                // This mock is more relevant for testing translate_cycle_to_opportunity.
                Ok(None) 
            }
        }
        let mock_quoting_engine_trait_obj: Arc<dyn QuotingEngineOperations + Send + Sync> = Arc::new(MockQuotingEngine);


        // Removed redundant PathFinder::new call that was causing an error.
        let config = ArbitrageDiscoveryConfig {
            discovery_interval: Duration::from_secs(10),
            min_profit_bps_threshold: 1, // 0.01%
            max_hops_for_opportunity: 5,
        };

        let path_finder = PathFinder::new(pool_cache, mock_quoting_engine_trait_obj, mock_dex_clients, opp_tx, config);
        path_finder.update_market_graph().await.unwrap();

        let graph = path_finder.market_graph.read().await;
        assert_eq!(graph.node_count(), 2); // SOL and USDC
        assert_eq!(graph.edge_count(), 2); // SOL -> USDC and USDC -> SOL

        // Verify edge weights (example, actual values depend on mock quote logic)
        let sol_usdc_weight = graph.edge_weight(sol_mint, usdc_mint).unwrap();
        let usdc_sol_weight = graph.edge_weight(usdc_mint, sol_mint).unwrap();
        
        // Based on MockPfDexClient: SOL -> USDC rate 1.01, USDC -> SOL rate 0.99
        assert!((*sol_usdc_weight - (-1.01f64.ln())).abs() < 1e-9);
        assert!((*usdc_sol_weight - (-0.99f64.ln())).abs() < 1e-9);
    }

    #[test]
    fn test_normalize_cycle_path() {
        let mint1 = Pubkey::new_unique(); // Lexicographically smallest
        let mint2 = Pubkey::new_unique();
        let mint3 = Pubkey::new_unique();

        let path1 = vec![mint1, mint2, mint3, mint1];
        let path2 = vec![mint2, mint3, mint1, mint2];
        let path3 = vec![mint3, mint1, mint2, mint3];

        let norm1 = PathFinder::normalize_cycle_path(&path1);
        let norm2 = PathFinder::normalize_cycle_path(&path2);
        let norm3 = PathFinder::normalize_cycle_path(&path3);

        assert_eq!(norm1, norm2);
        assert_eq!(norm2, norm3);
        assert!(norm1.starts_with(&mint1.to_string()));
    }

    #[tokio::test]
    async fn test_translate_cycle_to_opportunity_profitable() {
        let sol_mint = Pubkey::new_unique();
        let usdc_mint = Pubkey::new_unique();
        let ray_mint = Pubkey::new_unique();

        let pool_cache = Arc::new(DashMap::new());
        // SOL-USDC Pool
        let pool_su = Arc::new(PoolInfo { address: Pubkey::new_unique(), name: "SU".into(), dex_type: DexType::Raydium,
            token_a: PoolToken {mint: sol_mint, symbol: "SOL".into(), decimals: 9, reserve: 1000 * 10u64.pow(9)},
            token_b: PoolToken {mint: usdc_mint, symbol: "USDC".into(), decimals: 6, reserve: 100000 * 10u64.pow(6)}, ..Default::default()});
        pool_cache.insert(pool_su.address, pool_su.clone());
        // USDC-RAY Pool
        let pool_ur = Arc::new(PoolInfo { address: Pubkey::new_unique(), name: "UR".into(), dex_type: DexType::Raydium,
            token_a: PoolToken {mint: usdc_mint, symbol: "USDC".into(), decimals: 6, reserve: 100000 * 10u64.pow(6)},
            token_b: PoolToken {mint: ray_mint, symbol: "RAY".into(), decimals: 6, reserve: 500000 * 10u64.pow(6)}, ..Default::default()});
        pool_cache.insert(pool_ur.address, pool_ur.clone());
        // RAY-SOL Pool
        let pool_rs = Arc::new(PoolInfo { address: Pubkey::new_unique(), name: "RS".into(), dex_type: DexType::Raydium,
            token_a: PoolToken {mint: ray_mint, symbol: "RAY".into(), decimals: 6, reserve: 500000 * 10u64.pow(6)},
            token_b: PoolToken {mint: sol_mint, symbol: "SOL".into(), decimals: 9, reserve: 1000 * 10u64.pow(9)}, ..Default::default()});
        pool_cache.insert(pool_rs.address, pool_rs.clone());

        // Mock QuotingEngineOperations for this test
        struct ProfitableMockQuotingEngine {
            sol_mint: Pubkey,
            usdc_mint: Pubkey,
            ray_mint: Pubkey,
        }

        #[async_trait::async_trait]
        impl QuotingEngineOperations for ProfitableMockQuotingEngine {
            async fn calculate_best_quote(&self, input_mint: Pubkey, output_mint: Pubkey, amount_in: u64) -> Result<Option<Quote>> {
                // SOL -> USDC (1 SOL = 100 USDC)
                if input_mint == self.sol_mint && output_mint == self.usdc_mint {
                    return Ok(Some(Quote { input_token: "SOL".into(), output_token: "USDC".into(), input_amount: amount_in, output_amount: amount_in * 100 / (10u64.pow(9-6)), dex: "MockDex".into(), route: vec![Pubkey::new_unique()], slippage_estimate: None }));
                }
                // USDC -> RAY (1 USDC = 0.5 RAY)
                if input_mint == self.usdc_mint && output_mint == self.ray_mint {
                     return Ok(Some(Quote { input_token: "USDC".into(), output_token: "RAY".into(), input_amount: amount_in, output_amount: amount_in / 2, dex: "MockDex".into(), route: vec![Pubkey::new_unique()], slippage_estimate: None }));
                }
                // RAY -> SOL (1 RAY = 2.02 SOL, to make it profitable: 0.5 RAY = 1.01 SOL)
                // If 1 SOL = 100 USDC, 100 USDC = 50 RAY, then 1 SOL = 50 RAY.
                // We need 50 RAY to become > 1 SOL. Let's say 50 RAY = 1.01 SOL (atomic)
                if input_mint == self.ray_mint && output_mint == self.sol_mint {
                    // input amount_in is RAY (6 decimals)
                    // output amount is SOL (9 decimals)
                    // if amount_in is 50 RAY (50 * 10^6), output should be 1.01 SOL (1.01 * 10^9)
                    return Ok(Some(Quote { input_token: "RAY".into(), output_token: "SOL".into(), input_amount: amount_in, output_amount: amount_in * 101 / 50 * 10u64.pow(9-6) / 100, dex: "MockDex".into(), route: vec![Pubkey::new_unique()], slippage_estimate: None }));
                }
                Ok(None)
            }
        }
        let mock_quoting_engine_trait_obj: Arc<dyn QuotingEngineOperations + Send + Sync> = Arc::new(ProfitableMockQuotingEngine {
            sol_mint,
            usdc_mint,
            ray_mint,
        });

        let (opp_tx, _opp_rx) = broadcast::channel(10);
        let config = ArbitrageDiscoveryConfig {
            discovery_interval: Duration::from_secs(10),
            min_profit_bps_threshold: 1, // 0.01%
            max_hops_for_opportunity: 3,
        };
        let path_finder = PathFinder::new(pool_cache.clone(), mock_quoting_engine_trait_obj, vec![], opp_tx, config.clone());

        // Create a cycle: SOL -> USDC -> RAY -> SOL
        let cycle = vec![sol_mint, usdc_mint, ray_mint, sol_mint];
        
        let result = path_finder.translate_cycle_to_opportunity(cycle).await;
        
        assert!(result.is_ok());
        if let Ok(Some(opp)) = result {
            assert!(opp.profit_bps >= config.min_profit_bps_threshold, "Expected profit_bps {} to be >= {}", opp.profit_bps, config.min_profit_bps_threshold);
            assert_eq!(opp.hops.len(), 3);
        } else {
            panic!("Expected a profitable opportunity, but got None or Error: {:?}", result);
        }
    }
}