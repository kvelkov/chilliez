// src/arbitrage/routing/pathfinder.rs
//! PathFinder Module for Multi-Hop Route Discovery
//!
//! This module implements various pathfinding algorithms to discover optimal
//! trading routes across multiple DEXs and pools.

use anyhow::Result;
use serde::{Deserialize, Serialize};
// Removed unused import
use log::{info, warn};
use solana_sdk::pubkey::Pubkey;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use super::graph::{RouteEdge, RoutingGraph};

/// Configuration for pathfinding algorithms
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathfinderConfig {
    pub max_hops: usize,
    pub max_routes: usize,
    pub min_liquidity_threshold: f64,
    pub max_weight_threshold: f64,
    pub algorithm: PathfinderAlgorithm,
    pub timeout_ms: u64,
    pub enable_parallel_search: bool,
    pub diversification_factor: f64,
}

impl Default for PathfinderConfig {
    fn default() -> Self {
        Self {
            max_hops: 4,
            max_routes: 50,
            min_liquidity_threshold: 1000.0,
            max_weight_threshold: 1.0,
            algorithm: PathfinderAlgorithm::Dijkstra,
            timeout_ms: 500,
            enable_parallel_search: true,
            diversification_factor: 0.1,
        }
    }
}

impl PathfinderConfig {
    pub fn from_router_config(router_config: &super::SmartRouterConfig) -> Self {
        Self {
            max_hops: router_config.pathfinder.max_hops,
            max_routes: router_config.pathfinder.max_routes,
            min_liquidity_threshold: router_config.pathfinder.min_liquidity_threshold,
            max_weight_threshold: router_config.pathfinder.max_weight_threshold,
            timeout_ms: router_config.pathfinder.timeout_ms,
            ..Default::default()
        }
    }
}

/// Available pathfinding algorithms
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PathfinderAlgorithm {
    /// Dijkstra's shortest path algorithm
    Dijkstra,
    /// A* algorithm with heuristics
    AStar,
    /// Bellman-Ford for negative weights
    BellmanFord,
    /// Breadth-first search for unweighted graphs
    BFS,
    /// K-shortest paths algorithm
    KShortest,
    /// Custom multi-objective algorithm
    MultiObjective,
}

/// A complete route path through multiple pools
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutePath {
    pub steps: Vec<RouteStep>,
    pub total_fees: f64,
    pub total_weight: f64,
    pub expected_output: f64,
    pub estimated_gas_cost: u64,
    pub estimated_gas_fee: Option<u64>, // For compatibility
    pub execution_time_estimate: Option<Duration>,
    pub confidence_score: f64,
    pub liquidity_score: f64,
    pub dex_diversity_score: f64,
    pub price_impact: Option<f64>, // For compatibility
}

/// A single step in a route path
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteStep {
    pub pool_address: Pubkey,
    pub dex_type: crate::utils::DexType,
    pub from_token: Pubkey,
    pub to_token: Pubkey,
    pub amount_in: f64,
    pub amount_out: f64,
    pub fee_bps: u16,
    pub pool_liquidity: f64,
    pub price_impact: f64,
    pub execution_order: usize,
    pub slippage_tolerance: Option<f64>, // For compatibility
}

/// Node for pathfinding algorithms
#[derive(Debug, Clone)]
struct PathNode {
    token: Pubkey,
    distance: f64,
    path: Vec<RouteEdge>,
    estimated_output: f64,
}

impl PartialEq for PathNode {
    fn eq(&self, other: &Self) -> bool {
        self.distance.partial_cmp(&other.distance) == Some(std::cmp::Ordering::Equal)
    }
}

impl Eq for PathNode {}

impl PartialOrd for PathNode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        other.distance.partial_cmp(&self.distance) // Reverse for min-heap
    }
}

impl Ord for PathNode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap_or(std::cmp::Ordering::Equal)
    }
}

/// Statistics for pathfinding performance
#[derive(Debug, Clone, Serialize)]
pub struct PathfinderStats {
    pub total_searches: usize,
    pub successful_searches: usize,
    pub average_search_time: Duration,
    pub average_paths_found: f64,
    pub cache_hit_rate: f64,
    pub algorithm_performance: HashMap<String, AlgorithmStats>,
}

/// Performance statistics for a specific algorithm
#[derive(Debug, Clone, Serialize)]
pub struct AlgorithmStats {
    pub usage_count: usize,
    pub success_rate: f64,
    pub average_time: Duration,
    pub average_quality: f64,
}

/// Main pathfinder implementation
#[derive(Debug, Clone)]
pub struct PathFinder {
    config: PathfinderConfig,
    stats: PathfinderStats,
    path_cache: HashMap<(Pubkey, Pubkey), (Vec<RoutePath>, Instant)>,
    last_cleanup: Instant,
}

impl PathFinder {
    /// Create a new PathFinder with the given configuration
    pub fn new(config: PathfinderConfig) -> Self {
        Self {
            config,
            stats: PathfinderStats {
                total_searches: 0,
                successful_searches: 0,
                average_search_time: Duration::from_millis(0),
                average_paths_found: 0.0,
                cache_hit_rate: 0.0,
                algorithm_performance: HashMap::new(),
            },
            path_cache: HashMap::new(),
            last_cleanup: Instant::now(),
        }
    }

    /// Find all possible paths between two tokens
    pub async fn find_all_paths(
        &mut self,
        graph: &RoutingGraph,
        from_token: &str,
        to_token: &str,
        amount_in: f64,
    ) -> Result<Vec<RoutePath>> {
        let start_time = Instant::now();
        self.stats.total_searches += 1;

        // Parse token addresses (this would normally be done by a token resolver)
        let from_pubkey = self.resolve_token_address(from_token)?;
        let to_pubkey = self.resolve_token_address(to_token)?;

        // Check cache first
        let cache_key = (from_pubkey, to_pubkey);
        if let Some((cached_paths, cache_time)) = self.path_cache.get(&cache_key) {
            if cache_time.elapsed() < Duration::from_secs(30) {
                // 30-second cache
                let cached_paths_clone = cached_paths.clone();
                self.update_cache_stats(true);
                return Ok(self.adapt_paths_for_amount(cached_paths_clone, amount_in));
            }
        }
        self.update_cache_stats(false);

        // Find paths using the configured algorithm
        let paths = match self.config.algorithm {
            PathfinderAlgorithm::Dijkstra => {
                self.dijkstra_search(graph, from_pubkey, to_pubkey, amount_in)
                    .await?
            }
            PathfinderAlgorithm::AStar => {
                self.astar_search(graph, from_pubkey, to_pubkey, amount_in)
                    .await?
            }
            PathfinderAlgorithm::BellmanFord => {
                self.bellman_ford_search(graph, from_pubkey, to_pubkey, amount_in)
                    .await?
            }
            PathfinderAlgorithm::BFS => {
                self.bfs_search(graph, from_pubkey, to_pubkey, amount_in)
                    .await?
            }
            PathfinderAlgorithm::KShortest => {
                self.k_shortest_search(graph, from_pubkey, to_pubkey, amount_in)
                    .await?
            }
            PathfinderAlgorithm::MultiObjective => {
                self.multi_objective_search(graph, from_pubkey, to_pubkey, amount_in)
                    .await?
            }
        };

        // Cache the results
        self.path_cache
            .insert(cache_key, (paths.clone(), Instant::now()));

        // Clean up old cache entries periodically
        if self.last_cleanup.elapsed() > Duration::from_secs(300) {
            // 5 minutes
            self.cleanup_cache();
        }

        // Update statistics
        if !paths.is_empty() {
            self.stats.successful_searches += 1;
        }

        let search_time = start_time.elapsed();
        let algorithm = self.config.algorithm.clone();
        let paths_for_stats = paths.clone();
        self.update_algorithm_stats(&algorithm, search_time, !paths.is_empty(), &paths_for_stats);

        info!(
            "Found {} paths from {} to {} in {:?}",
            paths.len(),
            from_token,
            to_token,
            search_time
        );

        Ok(paths)
    }

    /// Find the shortest path between two tokens
    pub async fn find_shortest_path(
        &mut self,
        graph: &RoutingGraph,
        from_token: &str,
        to_token: &str,
        amount_in: f64,
    ) -> Result<Option<RoutePath>> {
        let paths = self
            .find_all_paths(graph, from_token, to_token, amount_in)
            .await?;

        // Return path with best expected output
        Ok(paths
            .into_iter()
            .max_by(|a, b| a.expected_output.partial_cmp(&b.expected_output).unwrap()))
    }

    /// Find k shortest paths between two tokens  
    pub async fn find_k_shortest_paths(
        &mut self,
        graph: &RoutingGraph,
        from_token: &str,
        to_token: &str,
        amount_in: f64,
        k: usize,
    ) -> Result<Vec<RoutePath>> {
        let mut paths = self
            .find_all_paths(graph, from_token, to_token, amount_in)
            .await?;

        // Sort by expected output (descending) and take top k
        paths.sort_by(|a, b| b.expected_output.partial_cmp(&a.expected_output).unwrap());
        paths.truncate(k);

        Ok(paths)
    }

    /// Find paths with specific constraints
    pub async fn find_paths_with_constraints(
        &mut self,
        graph: &RoutingGraph,
        from_token: &str,
        to_token: &str,
        amount_in: f64,
        constraints: &PathConstraints,
    ) -> Result<Vec<RoutePath>> {
        let all_paths = self
            .find_all_paths(graph, from_token, to_token, amount_in)
            .await?;

        // Filter paths based on constraints
        let filtered_paths: Vec<RoutePath> = all_paths
            .into_iter()
            .filter(|path| {
                // Check max hops constraint
                if let Some(max_hops) = constraints.max_hops {
                    if path.steps.len() > max_hops {
                        return false;
                    }
                }

                // Check max gas cost constraint
                if let Some(max_gas) = constraints.max_gas_cost {
                    if path.estimated_gas_cost > max_gas {
                        return false;
                    }
                }

                // Check execution time constraint
                if let Some(max_time) = constraints.max_execution_time {
                    if let Some(exec_time) = path.execution_time_estimate {
                        if exec_time > max_time {
                            return false;
                        }
                    }
                }

                // Check minimum output constraint
                if let Some(min_output) = constraints.min_output_amount {
                    if path.expected_output < min_output {
                        return false;
                    }
                }

                // Check required DEXs
                if let Some(ref required_dexs) = constraints.required_dexs {
                    let path_dexs: std::collections::HashSet<String> = path
                        .steps
                        .iter()
                        .map(|step| step.dex_type.to_string())
                        .collect();
                    if !required_dexs.iter().all(|dex| path_dexs.contains(dex)) {
                        return false;
                    }
                }

                // Check forbidden DEXs
                if let Some(ref forbidden_dexs) = constraints.forbidden_dexs {
                    let path_uses_forbidden = path
                        .steps
                        .iter()
                        .any(|step| forbidden_dexs.contains(&step.dex_type.to_string()));
                    if path_uses_forbidden {
                        return false;
                    }
                }

                true
            })
            .collect();

        Ok(filtered_paths)
    }

    /// Dijkstra's shortest path algorithm
    async fn dijkstra_search(
        &self,
        graph: &RoutingGraph,
        from_token: Pubkey,
        to_token: Pubkey,
        amount_in: f64,
    ) -> Result<Vec<RoutePath>> {
        let mut heap = BinaryHeap::new();
        let mut distances = HashMap::new();
        let mut visited = HashSet::new();
        let mut paths = Vec::new();

        // Initialize
        heap.push(PathNode {
            token: from_token,
            distance: 0.0,
            path: Vec::new(),
            estimated_output: amount_in,
        });
        distances.insert(from_token, 0.0);

        while let Some(current) = heap.pop() {
            if visited.contains(&current.token) {
                continue;
            }

            visited.insert(current.token);

            // Found target
            if current.token == to_token && !current.path.is_empty() {
                let route_path = self.build_route_path(current.path, amount_in).await?;
                paths.push(route_path);

                if paths.len() >= self.config.max_routes {
                    break;
                }
                continue;
            }

            // Explore neighbors
            let neighbors = graph.get_neighbors(current.token);
            for neighbor_token in neighbors {
                if visited.contains(&neighbor_token) {
                    continue;
                }

                if let Some(edge) = graph.get_best_direct_route(current.token, neighbor_token) {
                    // Skip if liquidity is too low
                    if edge.liquidity < self.config.min_liquidity_threshold {
                        continue;
                    }

                    let new_distance = current.distance + edge.weight;

                    // Skip if weight is too high
                    if new_distance > self.config.max_weight_threshold {
                        continue;
                    }

                    // Skip if path is too long
                    if current.path.len() >= self.config.max_hops {
                        continue;
                    }

                    if new_distance < *distances.get(&neighbor_token).unwrap_or(&f64::INFINITY) {
                        distances.insert(neighbor_token, new_distance);

                        let mut new_path = current.path.clone();
                        new_path.push(edge);

                        heap.push(PathNode {
                            token: neighbor_token,
                            distance: new_distance,
                            path: new_path,
                            estimated_output: current.estimated_output * 0.997, // Rough estimate after fees
                        });
                    }
                }
            }
        }

        // Sort paths by quality
        paths.sort_by(|a, b| {
            let score_a = self.calculate_path_score(a);
            let score_b = self.calculate_path_score(b);
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(paths)
    }

    /// A* search with heuristics
    async fn astar_search(
        &self,
        graph: &RoutingGraph,
        from_token: Pubkey,
        to_token: Pubkey,
        amount_in: f64,
    ) -> Result<Vec<RoutePath>> {
        // For now, use Dijkstra as A* requires good heuristics
        // TODO: Implement proper A* with token pair frequency heuristics
        warn!("A* algorithm not fully implemented, falling back to Dijkstra");
        self.dijkstra_search(graph, from_token, to_token, amount_in)
            .await
    }

    /// Bellman-Ford algorithm for handling negative weights
    async fn bellman_ford_search(
        &self,
        graph: &RoutingGraph,
        from_token: Pubkey,
        to_token: Pubkey,
        amount_in: f64,
    ) -> Result<Vec<RoutePath>> {
        // Bellman-Ford is less suitable for finding multiple paths
        // Use Dijkstra for now
        warn!("Bellman-Ford algorithm not optimal for pathfinding, using Dijkstra");
        self.dijkstra_search(graph, from_token, to_token, amount_in)
            .await
    }

    /// Breadth-first search for unweighted paths
    async fn bfs_search(
        &self,
        graph: &RoutingGraph,
        from_token: Pubkey,
        to_token: Pubkey,
        amount_in: f64,
    ) -> Result<Vec<RoutePath>> {
        let mut queue = VecDeque::new();
        let mut visited = HashSet::new();
        let mut paths = Vec::new();

        queue.push_back((from_token, Vec::new()));

        while let Some((current_token, current_path)) = queue.pop_front() {
            if current_path.len() >= self.config.max_hops {
                continue;
            }

            if current_token == to_token && !current_path.is_empty() {
                let route_path = self.build_route_path(current_path, amount_in).await?;
                paths.push(route_path);

                if paths.len() >= self.config.max_routes {
                    break;
                }
                continue;
            }

            let path_key = (current_token, current_path.len());
            if visited.contains(&path_key) {
                continue;
            }
            visited.insert(path_key);

            let neighbors = graph.get_neighbors(current_token);
            for neighbor_token in neighbors {
                if let Some(edge) = graph.get_best_direct_route(current_token, neighbor_token) {
                    if edge.liquidity >= self.config.min_liquidity_threshold {
                        let mut new_path = current_path.clone();
                        new_path.push(edge);
                        queue.push_back((neighbor_token, new_path));
                    }
                }
            }
        }

        // Sort paths by hop count (BFS naturally finds shorter paths first)
        paths.sort_by_key(|path| path.steps.len());

        Ok(paths)
    }

    /// K-shortest paths algorithm
    async fn k_shortest_search(
        &self,
        graph: &RoutingGraph,
        from_token: Pubkey,
        to_token: Pubkey,
        amount_in: f64,
    ) -> Result<Vec<RoutePath>> {
        // Use Dijkstra with multiple path tracking
        let all_paths = graph.find_paths(from_token, to_token, self.config.max_hops);

        let mut route_paths = Vec::new();
        for path in all_paths.into_iter().take(self.config.max_routes) {
            let route_path = self.build_route_path(path, amount_in).await?;
            route_paths.push(route_path);
        }

        Ok(route_paths)
    }

    /// Multi-objective search considering multiple criteria
    async fn multi_objective_search(
        &self,
        graph: &RoutingGraph,
        from_token: Pubkey,
        to_token: Pubkey,
        amount_in: f64,
    ) -> Result<Vec<RoutePath>> {
        // Find paths using multiple criteria and combine results
        let mut all_paths = Vec::new();

        // Dijkstra for shortest weighted path
        let dijkstra_paths = self
            .dijkstra_search(graph, from_token, to_token, amount_in)
            .await?;
        all_paths.extend(dijkstra_paths);

        // BFS for shortest hop count
        let bfs_paths = self
            .bfs_search(graph, from_token, to_token, amount_in)
            .await?;
        all_paths.extend(bfs_paths);

        // Remove duplicates and sort by combined score
        all_paths.sort_by(|a, b| {
            let score_a = self.calculate_multi_objective_score(a);
            let score_b = self.calculate_multi_objective_score(b);
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Remove duplicates based on path similarity
        all_paths.dedup_by(|a, b| self.paths_are_similar(a, b));

        // Take the best paths
        all_paths.truncate(self.config.max_routes);

        Ok(all_paths)
    }

    /// Build a RoutePath from a vector of RouteEdges
    async fn build_route_path(
        &self,
        edges: Vec<RouteEdge>,
        initial_amount: f64,
    ) -> Result<RoutePath> {
        let mut steps = Vec::new();
        let mut current_amount = initial_amount;
        let mut total_fees = 0.0;
        let mut total_weight = 0.0;
        let mut total_gas = 0u64;
        let mut total_time = Duration::from_millis(0);

        for (i, edge) in edges.iter().enumerate() {
            let fee_amount = current_amount * (edge.fee_bps as f64 / 10000.0);
            let amount_out = current_amount - fee_amount;

            // Calculate price impact (simplified)
            let price_impact = self.calculate_price_impact(current_amount, edge.liquidity);

            let step = RouteStep {
                pool_address: edge.pool_address,
                dex_type: edge.dex_type.clone(),
                from_token: edge.from_token,
                to_token: edge.to_token,
                amount_in: current_amount,
                amount_out,
                fee_bps: edge.fee_bps,
                pool_liquidity: edge.liquidity,
                price_impact,
                execution_order: i,
                slippage_tolerance: Some(0.01), // Default 1%
            };

            steps.push(step);
            current_amount = amount_out;
            total_fees += fee_amount;
            total_weight += edge.weight;
            total_gas += edge.estimated_gas;
            total_time += Duration::from_millis(edge.execution_time_ms);
        }

        let confidence_score = self.calculate_confidence_score(&steps);
        let liquidity_score = self.calculate_liquidity_score(&steps);
        let dex_diversity_score = self.calculate_dex_diversity_score(&steps);

        Ok(RoutePath {
            steps,
            total_fees,
            total_weight,
            expected_output: current_amount,
            estimated_gas_cost: total_gas,
            estimated_gas_fee: Some(total_gas),
            execution_time_estimate: Some(total_time),
            confidence_score,
            liquidity_score,
            dex_diversity_score,
            price_impact: Some(total_weight * 0.01), // Rough approximation
        })
    }

    /// Calculate price impact for a trade
    fn calculate_price_impact(&self, amount: f64, liquidity: f64) -> f64 {
        if liquidity <= 0.0 {
            return 1.0; // 100% impact for zero liquidity
        }

        // Simplified constant product formula impact
        let impact_factor = amount / liquidity;
        impact_factor.min(1.0)
    }

    /// Calculate overall path score
    fn calculate_path_score(&self, path: &RoutePath) -> f64 {
        let output_factor = path.expected_output / 100.0; // Normalize
        let fee_penalty = path.total_fees / 100.0;
        let confidence_bonus = path.confidence_score;
        let liquidity_bonus = path.liquidity_score;

        output_factor + confidence_bonus + liquidity_bonus - fee_penalty
    }

    /// Calculate multi-objective score
    fn calculate_multi_objective_score(&self, path: &RoutePath) -> f64 {
        let weights = [0.4, 0.2, 0.2, 0.1, 0.1]; // [output, confidence, liquidity, diversity, speed]
        let scores = [
            path.expected_output / 1000.0, // Normalize output
            path.confidence_score,
            path.liquidity_score,
            path.dex_diversity_score,
            1.0 / (path
                .execution_time_estimate
                .unwrap_or(Duration::from_millis(500))
                .as_millis() as f64
                / 1000.0), // Speed score
        ];

        weights.iter().zip(scores.iter()).map(|(w, s)| w * s).sum()
    }

    /// Check if two paths are similar (for deduplication)
    fn paths_are_similar(&self, path_a: &RoutePath, path_b: &RoutePath) -> bool {
        if path_a.steps.len() != path_b.steps.len() {
            return false;
        }

        let same_pools = path_a
            .steps
            .iter()
            .zip(path_b.steps.iter())
            .all(|(step_a, step_b)| step_a.pool_address == step_b.pool_address);

        same_pools
    }

    /// Calculate confidence score based on liquidity and historical data
    fn calculate_confidence_score(&self, steps: &[RouteStep]) -> f64 {
        if steps.is_empty() {
            return 0.0;
        }

        let avg_liquidity =
            steps.iter().map(|s| s.pool_liquidity).sum::<f64>() / steps.len() as f64;
        let liquidity_score = (avg_liquidity.ln() / 30.0).min(1.0).max(0.0);

        let avg_impact = steps.iter().map(|s| s.price_impact).sum::<f64>() / steps.len() as f64;
        let impact_score = (1.0 - avg_impact).max(0.0);

        (liquidity_score + impact_score) / 2.0
    }

    /// Calculate liquidity score
    fn calculate_liquidity_score(&self, steps: &[RouteStep]) -> f64 {
        if steps.is_empty() {
            return 0.0;
        }

        let min_liquidity = steps
            .iter()
            .map(|s| s.pool_liquidity)
            .fold(f64::INFINITY, f64::min);
        (min_liquidity.ln() / 25.0).min(1.0).max(0.0)
    }

    /// Calculate DEX diversity score
    fn calculate_dex_diversity_score(&self, steps: &[RouteStep]) -> f64 {
        if steps.is_empty() {
            return 0.0;
        }

        let unique_dexs: HashSet<_> = steps.iter().map(|s| &s.dex_type).collect();
        let diversity_ratio = unique_dexs.len() as f64 / steps.len() as f64;
        diversity_ratio
    }

    /// Adapt cached paths for a different amount
    fn adapt_paths_for_amount(&self, paths: Vec<RoutePath>, new_amount: f64) -> Vec<RoutePath> {
        // For now, just scale the amounts proportionally
        // In production, this would recalculate based on actual AMM curves
        paths
            .into_iter()
            .map(|mut path| {
                let scale_factor =
                    new_amount / path.steps.first().map(|s| s.amount_in).unwrap_or(1.0);

                for step in &mut path.steps {
                    step.amount_in *= scale_factor;
                    step.amount_out *= scale_factor;
                }

                path.expected_output *= scale_factor;
                path.total_fees *= scale_factor;

                path
            })
            .collect()
    }

    /// Update cache hit/miss statistics
    fn update_cache_stats(&mut self, hit: bool) {
        let total = self.stats.total_searches as f64;
        if total > 0.0 {
            if hit {
                self.stats.cache_hit_rate =
                    (self.stats.cache_hit_rate * (total - 1.0) + 1.0) / total;
            } else {
                self.stats.cache_hit_rate = self.stats.cache_hit_rate * (total - 1.0) / total;
            }
        }
    }

    /// Update algorithm performance statistics
    fn update_algorithm_stats(
        &mut self,
        algorithm: &PathfinderAlgorithm,
        search_time: Duration,
        success: bool,
        paths: &[RoutePath],
    ) {
        // Calculate average quality first (using immutable self)
        let avg_quality = if !paths.is_empty() {
            paths
                .iter()
                .map(|p| self.calculate_path_score(p))
                .sum::<f64>()
                / paths.len() as f64
        } else {
            0.0
        };

        // Now update stats (using mutable self)
        let algo_name = format!("{:?}", algorithm);
        let stats = self
            .stats
            .algorithm_performance
            .entry(algo_name)
            .or_insert(AlgorithmStats {
                usage_count: 0,
                success_rate: 0.0,
                average_time: Duration::from_millis(0),
                average_quality: 0.0,
            });

        stats.usage_count += 1;
        let count = stats.usage_count as f64;

        // Update success rate
        stats.success_rate =
            (stats.success_rate * (count - 1.0) + if success { 1.0 } else { 0.0 }) / count;

        // Update average time
        let old_time_ms = stats.average_time.as_millis() as f64;
        let new_time_ms = (old_time_ms * (count - 1.0) + search_time.as_millis() as f64) / count;
        stats.average_time = Duration::from_millis(new_time_ms as u64);

        // Update average quality
        if !paths.is_empty() {
            stats.average_quality = (stats.average_quality * (count - 1.0) + avg_quality) / count;
        }
    }

    /// Clean up old cache entries
    fn cleanup_cache(&mut self) {
        let cutoff_time = Instant::now() - Duration::from_secs(300); // 5 minutes
        self.path_cache
            .retain(|_, (_, timestamp)| *timestamp > cutoff_time);
        self.last_cleanup = Instant::now();
    }

    /// Resolve token symbol to Pubkey (placeholder implementation)
    fn resolve_token_address(&self, token_symbol: &str) -> Result<Pubkey> {
        // This would normally query a token registry
        // For now, create deterministic addresses for testing
        match token_symbol.to_uppercase().as_str() {
            "SOL" => Ok(solana_sdk::pubkey!(
                "So11111111111111111111111111111111111111112"
            )),
            "USDC" => Ok(solana_sdk::pubkey!(
                "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
            )),
            "USDT" => Ok(solana_sdk::pubkey!(
                "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
            )),
            _ => {
                // Generate a deterministic address for unknown tokens
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                token_symbol.hash(&mut hasher);
                let hash = hasher.finish();
                let bytes = hash.to_le_bytes();
                let mut pubkey_bytes = [0u8; 32];
                pubkey_bytes[0..8].copy_from_slice(&bytes);
                Ok(Pubkey::new_from_array(pubkey_bytes))
            }
        }
    }

    /// Get recent route count for monitoring
    pub fn recent_route_count(&self) -> usize {
        self.stats.total_searches
    }

    /// Get success rate for monitoring
    pub fn success_rate(&self) -> f64 {
        if self.stats.total_searches > 0 {
            self.stats.successful_searches as f64 / self.stats.total_searches as f64
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arbitrage::routing::graph::RoutingGraph;
    use crate::utils::{DexType, PoolInfo, PoolToken};

    fn create_test_graph() -> RoutingGraph {
        let graph = RoutingGraph::new();

        // Add some test pools for pathfinding
        let sol_mint = solana_sdk::pubkey!("So11111111111111111111111111111111111111112");
        let usdc_mint = solana_sdk::pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
        let usdt_mint = solana_sdk::pubkey!("Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB");

        // SOL-USDC pool
        let pool1 = PoolInfo {
            address: Pubkey::new_unique(),
            name: "SOL-USDC".to_string(),
            token_a: PoolToken {
                mint: sol_mint,
                symbol: "SOL".to_string(),
                decimals: 9,
                reserve: 1_000_000_000,
            },
            token_b: PoolToken {
                mint: usdc_mint,
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 100_000_000,
            },
            token_a_vault: Pubkey::new_unique(),
            token_b_vault: Pubkey::new_unique(),
            fee_numerator: Some(25),
            fee_denominator: Some(10000),
            fee_rate_bips: Some(25),
            last_update_timestamp: 0,
            dex_type: DexType::Raydium,
            liquidity: None,
            sqrt_price: None,
            tick_current_index: None,
            tick_spacing: None,
            tick_array_0: None,
            tick_array_1: None,
            tick_array_2: None,
            oracle: None,
        };

        // USDC-USDT pool
        let pool2 = PoolInfo {
            address: Pubkey::new_unique(),
            name: "USDC-USDT".to_string(),
            token_a: PoolToken {
                mint: usdc_mint,
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 2_000_000,
            },
            token_b: PoolToken {
                mint: usdt_mint,
                symbol: "USDT".to_string(),
                decimals: 6,
                reserve: 2_000_000,
            },
            token_a_vault: Pubkey::new_unique(),
            token_b_vault: Pubkey::new_unique(),
            fee_numerator: Some(5),
            fee_denominator: Some(10000),
            fee_rate_bips: Some(5),
            last_update_timestamp: 0,
            dex_type: DexType::Orca,
            liquidity: None,
            sqrt_price: None,
            tick_current_index: None,
            tick_spacing: None,
            tick_array_0: None,
            tick_array_1: None,
            tick_array_2: None,
            oracle: None,
        };

        graph.add_pool(std::sync::Arc::new(pool1)).unwrap();
        graph.add_pool(std::sync::Arc::new(pool2)).unwrap();

        graph
    }

    #[test]
    fn test_pathfinder_creation() {
        let config = PathfinderConfig::default();
        let pathfinder = PathFinder::new(config);

        assert_eq!(pathfinder.stats.total_searches, 0);
        assert_eq!(pathfinder.stats.successful_searches, 0);
    }

    #[tokio::test]
    async fn test_dijkstra_pathfinding() {
        let graph = create_test_graph();
        let config = PathfinderConfig {
            algorithm: PathfinderAlgorithm::Dijkstra,
            max_hops: 3,
            max_routes: 10,
            ..Default::default()
        };
        let mut pathfinder = PathFinder::new(config);

        let paths = pathfinder
            .find_all_paths(&graph, "SOL", "USDT", 100.0)
            .await
            .unwrap();

        assert!(!paths.is_empty());

        let first_path = &paths[0];
        assert_eq!(first_path.steps.len(), 2); // SOL -> USDC -> USDT
        assert!(first_path.expected_output > 0.0);
        assert!(first_path.total_fees > 0.0);
    }

    #[tokio::test]
    async fn test_bfs_pathfinding() {
        let graph = create_test_graph();
        let config = PathfinderConfig {
            algorithm: PathfinderAlgorithm::BFS,
            max_hops: 3,
            max_routes: 10,
            ..Default::default()
        };
        let mut pathfinder = PathFinder::new(config);

        let paths = pathfinder
            .find_all_paths(&graph, "SOL", "USDT", 100.0)
            .await
            .unwrap();

        assert!(!paths.is_empty());
    }

    #[test]
    fn test_token_resolution() {
        let config = PathfinderConfig::default();
        let pathfinder = PathFinder::new(config);

        let sol_address = pathfinder.resolve_token_address("SOL").unwrap();
        let usdc_address = pathfinder.resolve_token_address("USDC").unwrap();

        assert_eq!(
            sol_address,
            solana_sdk::pubkey!("So11111111111111111111111111111111111111112")
        );
        assert_eq!(
            usdc_address,
            solana_sdk::pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v")
        );
    }

    #[test]
    fn test_confidence_score_calculation() {
        let config = PathfinderConfig::default();
        let pathfinder = PathFinder::new(config);

        let steps = vec![RouteStep {
            pool_address: Pubkey::new_unique(),
            dex_type: DexType::Raydium,
            from_token: Pubkey::new_unique(),
            to_token: Pubkey::new_unique(),
            amount_in: 100.0,
            amount_out: 99.0,
            fee_bps: 25,
            pool_liquidity: 1_000_000.0,
            price_impact: 0.01,
            execution_order: 0,
            slippage_tolerance: Some(0.01),
        }];

        let confidence = pathfinder.calculate_confidence_score(&steps);
        assert!(confidence > 0.0);
        assert!(confidence <= 1.0);
    }

    #[test]
    fn test_dex_diversity_score() {
        let config = PathfinderConfig::default();
        let pathfinder = PathFinder::new(config);

        let steps = vec![
            RouteStep {
                pool_address: Pubkey::new_unique(),
                dex_type: DexType::Raydium,
                from_token: Pubkey::new_unique(),
                to_token: Pubkey::new_unique(),
                amount_in: 100.0,
                amount_out: 99.0,
                fee_bps: 25,
                pool_liquidity: 1_000_000.0,
                price_impact: 0.01,
                execution_order: 0,
                slippage_tolerance: Some(0.01),
            },
            RouteStep {
                pool_address: Pubkey::new_unique(),
                dex_type: DexType::Orca,
                from_token: Pubkey::new_unique(),
                to_token: Pubkey::new_unique(),
                amount_in: 99.0,
                amount_out: 98.0,
                fee_bps: 30,
                pool_liquidity: 500_000.0,
                price_impact: 0.02,
                execution_order: 1,
                slippage_tolerance: Some(0.01),
            },
        ];

        let diversity = pathfinder.calculate_dex_diversity_score(&steps);
        assert_eq!(diversity, 1.0); // Two different DEXs
    }

    #[tokio::test]
    async fn test_path_caching() {
        let graph = create_test_graph();
        let config = PathfinderConfig::default();
        let mut pathfinder = PathFinder::new(config);

        // First search
        let paths1 = pathfinder
            .find_all_paths(&graph, "SOL", "USDC", 100.0)
            .await
            .unwrap();

        // Second search (should hit cache)
        let paths2 = pathfinder
            .find_all_paths(&graph, "SOL", "USDC", 100.0)
            .await
            .unwrap();

        assert_eq!(paths1.len(), paths2.len());
        assert!(pathfinder.stats.cache_hit_rate > 0.0);
    }
}

/// Constraints for pathfinding
#[derive(Debug, Clone)]
pub struct PathConstraints {
    pub max_hops: Option<usize>,
    pub max_gas_cost: Option<u64>,
    pub max_execution_time: Option<Duration>,
    pub min_output_amount: Option<f64>,
    pub required_dexs: Option<Vec<String>>,
    pub forbidden_dexs: Option<Vec<String>>,
    pub max_price_impact: Option<f64>,
}

impl Default for PathConstraints {
    fn default() -> Self {
        Self {
            max_hops: None,
            max_gas_cost: None,
            max_execution_time: None,
            min_output_amount: None,
            required_dexs: None,
            forbidden_dexs: None,
            max_price_impact: None,
        }
    }
}
