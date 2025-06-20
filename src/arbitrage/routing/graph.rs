// src/arbitrage/routing/graph.rs
//! Routing Graph Module for Multi-Hop DEX Routing
//!
//! This module provides a graph-based representation of liquidity pools across
//! multiple DEXs, enabling efficient pathfinding and route optimization.

use anyhow::Result;
use dashmap::DashMap;
use log::{debug, info};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;

// Removed unused imports
use crate::dex::discovery::PoolDiscoveryService;
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{DexType, PoolInfo};
use solana_sdk::pubkey::Pubkey;

/// A node in the routing graph representing a token
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenNode {
    pub mint: Pubkey,
    pub symbol: String,
    pub decimals: u8,
    pub is_verified: bool,
    pub total_liquidity_usd: f64,
    pub pool_count: usize,
}

/// A pool node in the routing graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolNode {
    pub address: Pubkey,
    pub dex_type: DexType,
    pub token_a: Pubkey,
    pub token_b: Pubkey,
    pub liquidity_usd: f64,
    pub fee_bps: u16,
    pub last_update: SystemTime,
    pub is_active: bool,
    pub volume_24h: f64,
    pub apy: f64,
}

/// An edge representing a trading route between tokens through a pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteEdge {
    pub pool_address: Pubkey,
    pub dex_type: DexType,
    pub from_token: Pubkey,
    pub to_token: Pubkey,
    pub weight: f64, // Lower weight = better route (based on fees, liquidity, etc.)
    pub liquidity: f64,
    pub fee_bps: u16,
    pub estimated_gas: u64,
    pub execution_time_ms: u64,
}

/// Liquidity pool wrapper for graph operations
#[derive(Debug, Clone)]
pub struct LiquidityPool {
    pub info: Arc<PoolInfo>,
    pub metrics: PoolMetrics,
    pub health: PoolHealth,
}

/// Pool performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolMetrics {
    pub volume_24h: f64,
    pub volume_7d: f64,
    pub fee_revenue_24h: f64,
    pub price_impact_avg: f64,
    pub success_rate: f64,
    pub average_trade_size: f64,
}

/// Pool health indicators
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolHealth {
    pub is_active: bool,
    pub last_successful_trade: Option<SystemTime>,
    pub error_count_24h: u32,
    pub liquidity_depth_score: f64, // 0.0 to 1.0
    pub spread_quality_score: f64,  // 0.0 to 1.0
    pub overall_score: f64,         // 0.0 to 1.0
}

impl Default for PoolMetrics {
    fn default() -> Self {
        Self {
            volume_24h: 0.0,
            volume_7d: 0.0,
            fee_revenue_24h: 0.0,
            price_impact_avg: 0.0,
            success_rate: 1.0,
            average_trade_size: 0.0,
        }
    }
}

impl Default for PoolHealth {
    fn default() -> Self {
        Self {
            is_active: true,
            last_successful_trade: None,
            error_count_24h: 0,
            liquidity_depth_score: 0.5,
            spread_quality_score: 0.5,
            overall_score: 0.5,
        }
    }
}

/// Main routing graph structure
#[derive(Clone)]
pub struct RoutingGraph {
    /// Token nodes indexed by mint address
    tokens: Arc<DashMap<Pubkey, TokenNode>>,
    /// Pool nodes indexed by pool address
    pools: Arc<DashMap<Pubkey, LiquidityPool>>,
    /// Adjacency list: token mint -> list of connected edges
    adjacency: Arc<DashMap<Pubkey, Vec<RouteEdge>>>,
    /// Pool discovery service for data updates
    discovery_service: Option<Arc<PoolDiscoveryService>>,
    /// RPC client for on-chain data
    rpc_client: Option<Arc<SolanaRpcClient>>,
    /// Graph statistics
    stats: Arc<RwLock<GraphStats>>,
    /// Last update timestamp
    last_update: Arc<RwLock<SystemTime>>,
}

/// Graph statistics for monitoring
#[derive(Debug, Clone, Serialize)]
pub struct GraphStats {
    pub token_count: usize,
    pub pool_count: usize,
    pub edge_count: usize,
    pub total_liquidity_usd: f64,
    pub active_pool_percentage: f64,
    pub average_path_length: f64,
    pub connectivity_score: f64,
    pub last_update_duration: Duration,
}

impl Default for GraphStats {
    fn default() -> Self {
        Self {
            token_count: 0,
            pool_count: 0,
            edge_count: 0,
            total_liquidity_usd: 0.0,
            active_pool_percentage: 0.0,
            average_path_length: 0.0,
            connectivity_score: 0.0,
            last_update_duration: Duration::from_secs(0),
        }
    }
}

impl RoutingGraph {
    /// Create a new routing graph
    pub fn new() -> Self {
        Self {
            tokens: Arc::new(DashMap::new()),
            pools: Arc::new(DashMap::new()),
            adjacency: Arc::new(DashMap::new()),
            discovery_service: None,
            rpc_client: None,
            stats: Arc::new(RwLock::new(GraphStats::default())),
            last_update: Arc::new(RwLock::new(SystemTime::now())),
        }
    }

    /// Create a new routing graph with discovery service
    pub fn with_discovery_service(discovery_service: Arc<PoolDiscoveryService>) -> Self {
        let mut graph = Self::new();
        graph.discovery_service = Some(discovery_service);
        graph
    }

    /// Set RPC client for real-time data updates
    pub fn set_rpc_client(&mut self, rpc_client: Arc<SolanaRpcClient>) {
        self.rpc_client = Some(rpc_client);
    }

    /// Add a token to the graph
    pub fn add_token(&self, mint: Pubkey, symbol: String, decimals: u8) -> Result<()> {
        let token_node = TokenNode {
            mint,
            symbol,
            decimals,
            is_verified: false, // TODO: Implement token verification
            total_liquidity_usd: 0.0,
            pool_count: 0,
        };

        self.tokens.insert(mint, token_node);
        debug!("Added token {} to routing graph", mint);
        Ok(())
    }

    /// Add a pool to the graph
    pub fn add_pool(&self, pool_info: Arc<PoolInfo>) -> Result<()> {
        let pool_address = pool_info.address;

        // Ensure tokens exist in the graph
        if !self.tokens.contains_key(&pool_info.token_a.mint) {
            self.add_token(
                pool_info.token_a.mint,
                pool_info.token_a.symbol.clone(),
                pool_info.token_a.decimals,
            )?;
        }

        if !self.tokens.contains_key(&pool_info.token_b.mint) {
            self.add_token(
                pool_info.token_b.mint,
                pool_info.token_b.symbol.clone(),
                pool_info.token_b.decimals,
            )?;
        }

        // Create pool node
        let liquidity_pool = LiquidityPool {
            info: pool_info.clone(),
            metrics: PoolMetrics::default(),
            health: PoolHealth::default(),
        };

        // Calculate edge weights
        let fee_bps = pool_info.get_fee_rate_bips();
        let liquidity_score = self.calculate_liquidity_score(&pool_info);
        let weight = self.calculate_edge_weight(fee_bps, liquidity_score);

        // Create bidirectional edges
        let edge_a_to_b = RouteEdge {
            pool_address,
            dex_type: pool_info.dex_type.clone(),
            from_token: pool_info.token_a.mint,
            to_token: pool_info.token_b.mint,
            weight,
            liquidity: liquidity_score,
            fee_bps,
            estimated_gas: self.estimate_gas_cost(&pool_info.dex_type),
            execution_time_ms: self.estimate_execution_time(&pool_info.dex_type),
        };

        let edge_b_to_a = RouteEdge {
            pool_address,
            dex_type: pool_info.dex_type.clone(),
            from_token: pool_info.token_b.mint,
            to_token: pool_info.token_a.mint,
            weight,
            liquidity: liquidity_score,
            fee_bps,
            estimated_gas: self.estimate_gas_cost(&pool_info.dex_type),
            execution_time_ms: self.estimate_execution_time(&pool_info.dex_type),
        };

        // Add edges to adjacency list
        self.adjacency
            .entry(pool_info.token_a.mint)
            .or_default()
            .push(edge_a_to_b);
        self.adjacency
            .entry(pool_info.token_b.mint)
            .or_default()
            .push(edge_b_to_a);

        // Store pool
        self.pools.insert(pool_address, liquidity_pool);

        debug!(
            "Added pool {} ({} -> {}) to routing graph",
            pool_address, pool_info.token_a.symbol, pool_info.token_b.symbol
        );
        Ok(())
    }

    /// Remove a pool from the graph
    pub fn remove_pool(&self, pool_address: Pubkey) -> Result<()> {
        if let Some((_, pool)) = self.pools.remove(&pool_address) {
            let token_a = pool.info.token_a.mint;
            let token_b = pool.info.token_b.mint;

            // Remove edges from adjacency list
            if let Some(mut edges) = self.adjacency.get_mut(&token_a) {
                edges.retain(|edge| edge.pool_address != pool_address);
            }
            if let Some(mut edges) = self.adjacency.get_mut(&token_b) {
                edges.retain(|edge| edge.pool_address != pool_address);
            }

            debug!("Removed pool {} from routing graph", pool_address);
        }
        Ok(())
    }

    /// Find all possible paths between two tokens
    pub fn find_paths(
        &self,
        from_token: Pubkey,
        to_token: Pubkey,
        max_hops: usize,
    ) -> Vec<Vec<RouteEdge>> {
        let mut paths = Vec::new();
        let mut visited = HashSet::new();
        let mut current_path = Vec::new();

        self.dfs_paths(
            from_token,
            to_token,
            max_hops,
            &mut visited,
            &mut current_path,
            &mut paths,
        );

        // Sort paths by total weight (ascending = better)
        paths.sort_by(|a, b| {
            let weight_a: f64 = a.iter().map(|edge| edge.weight).sum();
            let weight_b: f64 = b.iter().map(|edge| edge.weight).sum();
            weight_a
                .partial_cmp(&weight_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        paths
    }

    /// Depth-first search for path finding
    fn dfs_paths(
        &self,
        current: Pubkey,
        target: Pubkey,
        max_hops: usize,
        visited: &mut HashSet<Pubkey>,
        current_path: &mut Vec<RouteEdge>,
        all_paths: &mut Vec<Vec<RouteEdge>>,
    ) {
        if current_path.len() >= max_hops {
            return;
        }

        if current == target && !current_path.is_empty() {
            all_paths.push(current_path.clone());
            return;
        }

        visited.insert(current);

        if let Some(edges) = self.adjacency.get(&current) {
            for edge in edges.iter() {
                if !visited.contains(&edge.to_token) {
                    current_path.push(edge.clone());
                    self.dfs_paths(
                        edge.to_token,
                        target,
                        max_hops,
                        visited,
                        current_path,
                        all_paths,
                    );
                    current_path.pop();
                }
            }
        }

        visited.remove(&current);
    }

    /// Update graph with latest pool data
    pub async fn update_liquidity(&mut self) -> Result<()> {
        let start_time = SystemTime::now();

        if let Some(discovery_service) = &self.discovery_service {
            let pools = discovery_service.get_all_cached_pools();

            for pool_info in pools {
                // Update existing pool or add new one
                if self.pools.contains_key(&pool_info.address) {
                    self.update_pool_data(&pool_info).await?;
                } else {
                    self.add_pool(pool_info)?;
                }
            }
        }

        self.update_stats().await?;

        let mut last_update = self.last_update.write().await;
        *last_update = SystemTime::now();

        info!(
            "Updated routing graph liquidity in {:?}",
            start_time.elapsed()
        );
        Ok(())
    }

    /// Update fee structures for all pools
    pub async fn update_fees(&mut self) -> Result<()> {
        // This would integrate with real-time fee data from DEXs
        // For now, we update weights based on current fee data

        for mut pool_entry in self.pools.iter_mut() {
            let pool = pool_entry.value_mut();
            let fee_bps = pool.info.get_fee_rate_bips();
            let liquidity_score = self.calculate_liquidity_score(&pool.info);
            let new_weight = self.calculate_edge_weight(fee_bps, liquidity_score);

            // Update edge weights in adjacency list
            self.update_edge_weights(pool.info.address, new_weight);
        }

        debug!("Updated routing graph fees");
        Ok(())
    }

    /// Update pool health status
    pub async fn update_health_status(&mut self) -> Result<()> {
        for mut pool_entry in self.pools.iter_mut() {
            let pool = pool_entry.value_mut();

            // Calculate health metrics
            pool.health.liquidity_depth_score = self.calculate_liquidity_depth_score(&pool.info);
            pool.health.spread_quality_score = self.calculate_spread_quality_score(&pool.info);
            pool.health.overall_score =
                (pool.health.liquidity_depth_score + pool.health.spread_quality_score) / 2.0;

            // Update pool activity status
            pool.health.is_active = pool.health.overall_score > 0.3;
        }

        debug!("Updated routing graph health status");
        Ok(())
    }

    /// Get neighboring tokens for a given token
    pub fn get_neighbors(&self, token: Pubkey) -> Vec<Pubkey> {
        if let Some(edges) = self.adjacency.get(&token) {
            edges.iter().map(|edge| edge.to_token).collect()
        } else {
            Vec::new()
        }
    }

    /// Get best direct route between two tokens
    pub fn get_best_direct_route(&self, from_token: Pubkey, to_token: Pubkey) -> Option<RouteEdge> {
        if let Some(edges) = self.adjacency.get(&from_token) {
            edges
                .iter()
                .filter(|edge| edge.to_token == to_token)
                .min_by(|a, b| {
                    a.weight
                        .partial_cmp(&b.weight)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .cloned()
        } else {
            None
        }
    }

    /// Calculate connectivity score for the graph
    pub fn connectivity_score(&self) -> f64 {
        let token_count = self.tokens.len() as f64;
        if token_count <= 1.0 {
            return 0.0;
        }

        let total_possible_edges = token_count * (token_count - 1.0);
        let actual_edges = self.adjacency.len() as f64;

        (actual_edges / total_possible_edges).min(1.0)
    }

    /// Get total pool count
    pub fn pool_count(&self) -> usize {
        self.pools.len()
    }

    /// Get total token count
    pub fn token_count(&self) -> usize {
        self.tokens.len()
    }

    /// Get average liquidity across all pools
    pub fn average_liquidity(&self) -> f64 {
        if self.pools.is_empty() {
            return 0.0;
        }

        let total_liquidity: f64 = self
            .pools
            .iter()
            .map(|entry| self.calculate_liquidity_score(&entry.value().info))
            .sum();

        total_liquidity / self.pools.len() as f64
    }

    /// Update pool-specific data
    async fn update_pool_data(&self, pool_info: &PoolInfo) -> Result<()> {
        if let Some(mut pool_entry) = self.pools.get_mut(&pool_info.address) {
            // Update the pool info
            pool_entry.info = Arc::new(pool_info.clone());

            // Recalculate metrics
            let liquidity_score = self.calculate_liquidity_score(pool_info);
            let fee_bps = pool_info.get_fee_rate_bips();
            let new_weight = self.calculate_edge_weight(fee_bps, liquidity_score);

            // Update edge weights
            self.update_edge_weights(pool_info.address, new_weight);
        }
        Ok(())
    }

    /// Update edge weights for a specific pool
    fn update_edge_weights(&self, pool_address: Pubkey, new_weight: f64) {
        for mut edges_entry in self.adjacency.iter_mut() {
            for edge in edges_entry.value_mut() {
                if edge.pool_address == pool_address {
                    edge.weight = new_weight;
                }
            }
        }
    }

    /// Calculate edge weight based on fees and liquidity
    fn calculate_edge_weight(&self, fee_bps: u16, liquidity_score: f64) -> f64 {
        let fee_factor = fee_bps as f64 / 10000.0; // Convert bps to decimal
        let liquidity_factor = 1.0 / (liquidity_score + 1.0); // Higher liquidity = lower weight

        // Combine factors (lower is better)
        fee_factor + liquidity_factor
    }

    /// Calculate liquidity score for a pool
    fn calculate_liquidity_score(&self, pool_info: &PoolInfo) -> f64 {
        // This is a simplified calculation
        // In production, this would use USD values and market prices
        let token_a_reserve = pool_info.token_a.reserve as f64;
        let token_b_reserve = pool_info.token_b.reserve as f64;

        // Geometric mean of reserves (better for unbalanced pools)
        (token_a_reserve * token_b_reserve).sqrt()
    }

    /// Calculate liquidity depth score
    fn calculate_liquidity_depth_score(&self, pool_info: &PoolInfo) -> f64 {
        let liquidity = self.calculate_liquidity_score(pool_info);
        // Normalize to 0-1 scale (using log scale for wide range)
        (liquidity.ln() / 30.0).clamp(0.0, 1.0) // Assumes max liquidity ~1e13
    }

    /// Calculate spread quality score
    fn calculate_spread_quality_score(&self, pool_info: &PoolInfo) -> f64 {
        let fee_bps = pool_info.get_fee_rate_bips() as f64;
        // Lower fees = higher quality
        (100.0 - fee_bps).max(0.0) / 100.0
    }

    /// Estimate gas cost for a DEX
    fn estimate_gas_cost(&self, dex_type: &DexType) -> u64 {
        match dex_type {
            DexType::Jupiter => 200_000,    // Jupiter aggregation
            DexType::Orca => 150_000,       // Whirlpool CLMM
            DexType::Raydium => 180_000,    // AMM V4
            DexType::Meteora => 170_000,    // Dynamic AMM/DLMM
            DexType::Lifinity => 160_000,   // Proactive MM
            DexType::Phoenix => 200_000,    // Order book
            DexType::Whirlpool => 150_000,  // Same as Orca
            DexType::Unknown(_) => 250_000, // Conservative estimate
        }
    }

    /// Estimate execution time for a DEX
    fn estimate_execution_time(&self, dex_type: &DexType) -> u64 {
        match dex_type {
            DexType::Jupiter => 2000,    // 2s for aggregation
            DexType::Orca => 800,        // 800ms for CLMM
            DexType::Raydium => 600,     // 600ms for AMM
            DexType::Meteora => 700,     // 700ms for Dynamic AMM
            DexType::Lifinity => 900,    // 900ms for PMM
            DexType::Phoenix => 1200,    // 1.2s for order book
            DexType::Whirlpool => 800,   // Same as Orca
            DexType::Unknown(_) => 3000, // Conservative estimate
        }
    }

    /// Update graph statistics
    async fn update_stats(&self) -> Result<()> {
        let mut stats = self.stats.write().await;

        stats.token_count = self.tokens.len();
        stats.pool_count = self.pools.len();
        stats.edge_count = self.adjacency.iter().map(|entry| entry.value().len()).sum();
        stats.total_liquidity_usd = self
            .pools
            .iter()
            .map(|entry| self.calculate_liquidity_score(&entry.value().info))
            .sum();
        stats.connectivity_score = self.connectivity_score();

        // Calculate active pool percentage
        let active_pools = self
            .pools
            .iter()
            .filter(|entry| entry.value().health.is_active)
            .count();
        stats.active_pool_percentage = if stats.pool_count > 0 {
            (active_pools as f64 / stats.pool_count as f64) * 100.0
        } else {
            0.0
        };

        // Calculate average path length (simplified)
        stats.average_path_length = if stats.token_count > 0 {
            stats.edge_count as f64 / stats.token_count as f64
        } else {
            0.0
        };

        Ok(())
    }
}

impl Default for RoutingGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for RoutingGraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RoutingGraph")
            .field("tokens_count", &self.tokens.len())
            .field("pools_count", &self.pools.len())
            .field("adjacency_count", &self.adjacency.len())
            .field("has_discovery_service", &self.discovery_service.is_some())
            .field("has_rpc_client", &self.rpc_client.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::{DexType, PoolToken};

    fn create_test_pool_info(
        address: Pubkey,
        token_a_mint: Pubkey,
        token_b_mint: Pubkey,
        dex_type: DexType,
    ) -> PoolInfo {
        PoolInfo {
            address,
            name: "Test Pool".to_string(),
            token_a: PoolToken {
                mint: token_a_mint,
                symbol: "TOKA".to_string(),
                decimals: 6,
                reserve: 1_000_000,
            },
            token_b: PoolToken {
                mint: token_b_mint,
                symbol: "TOKB".to_string(),
                decimals: 6,
                reserve: 2_000_000,
            },
            token_a_vault: Pubkey::new_unique(),
            token_b_vault: Pubkey::new_unique(),
            fee_numerator: Some(25),
            fee_denominator: Some(10000),
            fee_rate_bips: Some(25),
            last_update_timestamp: 0,
            dex_type,
            liquidity: None,
            sqrt_price: None,
            tick_current_index: None,
            tick_spacing: None,
            tick_array_0: None,
            tick_array_1: None,
            tick_array_2: None,
            oracle: None,
        }
    }

    #[test]
    fn test_routing_graph_creation() {
        let graph = RoutingGraph::new();
        assert_eq!(graph.pool_count(), 0);
        assert_eq!(graph.token_count(), 0);
    }

    #[test]
    fn test_add_token() {
        let graph = RoutingGraph::new();
        let token_mint = Pubkey::new_unique();

        graph.add_token(token_mint, "TEST".to_string(), 6).unwrap();
        assert_eq!(graph.token_count(), 1);
        assert!(graph.tokens.contains_key(&token_mint));
    }

    #[test]
    fn test_add_pool() {
        let graph = RoutingGraph::new();
        let pool_address = Pubkey::new_unique();
        let token_a_mint = Pubkey::new_unique();
        let token_b_mint = Pubkey::new_unique();

        let pool_info =
            create_test_pool_info(pool_address, token_a_mint, token_b_mint, DexType::Raydium);

        graph.add_pool(Arc::new(pool_info)).unwrap();

        assert_eq!(graph.pool_count(), 1);
        assert_eq!(graph.token_count(), 2);
        assert!(graph.pools.contains_key(&pool_address));
    }

    #[test]
    fn test_find_direct_path() {
        let graph = RoutingGraph::new();
        let token_a_mint = Pubkey::new_unique();
        let token_b_mint = Pubkey::new_unique();
        let pool_address = Pubkey::new_unique();

        let pool_info =
            create_test_pool_info(pool_address, token_a_mint, token_b_mint, DexType::Orca);

        graph.add_pool(Arc::new(pool_info)).unwrap();

        let best_route = graph.get_best_direct_route(token_a_mint, token_b_mint);
        assert!(best_route.is_some());

        let route = best_route.unwrap();
        assert_eq!(route.from_token, token_a_mint);
        assert_eq!(route.to_token, token_b_mint);
        assert_eq!(route.pool_address, pool_address);
    }

    #[test]
    fn test_multi_hop_paths() {
        let graph = RoutingGraph::new();

        // Create tokens: A -> B -> C
        let token_a = Pubkey::new_unique();
        let token_b = Pubkey::new_unique();
        let token_c = Pubkey::new_unique();

        // Create pools: A-B and B-C
        let pool_ab =
            create_test_pool_info(Pubkey::new_unique(), token_a, token_b, DexType::Raydium);
        let pool_bc = create_test_pool_info(Pubkey::new_unique(), token_b, token_c, DexType::Orca);

        graph.add_pool(Arc::new(pool_ab)).unwrap();
        graph.add_pool(Arc::new(pool_bc)).unwrap();

        // Find path from A to C
        let paths = graph.find_paths(token_a, token_c, 3);
        assert!(!paths.is_empty());

        let path = &paths[0];
        assert_eq!(path.len(), 2); // Two hops: A->B->C
        assert_eq!(path[0].from_token, token_a);
        assert_eq!(path[0].to_token, token_b);
        assert_eq!(path[1].from_token, token_b);
        assert_eq!(path[1].to_token, token_c);
    }

    #[test]
    fn test_connectivity_score() {
        let graph = RoutingGraph::new();

        // Empty graph
        assert_eq!(graph.connectivity_score(), 0.0);

        // Add tokens and pools
        let token_a = Pubkey::new_unique();
        let token_b = Pubkey::new_unique();

        let pool_info =
            create_test_pool_info(Pubkey::new_unique(), token_a, token_b, DexType::Raydium);

        graph.add_pool(Arc::new(pool_info)).unwrap();

        let score = graph.connectivity_score();
        assert!(score > 0.0);
        assert!(score <= 1.0);
    }

    #[tokio::test]
    async fn test_graph_stats_update() {
        let graph = RoutingGraph::new();

        // Add some pools
        for _i in 0..3 {
            let pool_info = create_test_pool_info(
                Pubkey::new_unique(),
                Pubkey::new_unique(),
                Pubkey::new_unique(),
                DexType::Raydium,
            );
            graph.add_pool(Arc::new(pool_info)).unwrap();
        }

        // Update stats
        graph.update_stats().await.unwrap();

        let stats = graph.stats.read().await;
        assert_eq!(stats.pool_count, 3);
        assert_eq!(stats.token_count, 6); // 2 tokens per pool
        assert!(stats.total_liquidity_usd > 0.0);
    }
}
