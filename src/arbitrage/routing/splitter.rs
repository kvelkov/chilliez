// src/arbitrage/routing/splitter.rs
//! Route Splitter Module for Optimal Trade Distribution
//!
//! This module implements intelligent route splitting algorithms that divide
//! large trades across multiple pools and DEXs to minimize price impact
//! and maximize execution efficiency.

use anyhow::Result;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

#[cfg(test)]
use solana_sdk::pubkey::Pubkey;

use super::pathfinder::RoutePath;
#[cfg(test)]
use super::pathfinder::RouteStep;
use super::{RouteRequest, RoutingPriority};

/// Configuration for route splitting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitterConfig {
    pub min_split_amount: f64,
    pub max_splits: usize,
    pub liquidity_utilization_ratio: f64, // Max percentage of pool liquidity to use
    pub price_impact_threshold: f64,      // Split if impact exceeds this
    pub parallel_execution_enabled: bool,
    pub gas_optimization_enabled: bool,
}

impl Default for SplitterConfig {
    fn default() -> Self {
        Self {
            min_split_amount: 100.0, // Minimum $100 per split
            max_splits: 5,
            liquidity_utilization_ratio: 0.05, // Use max 5% of pool liquidity
            price_impact_threshold: 0.03,      // 3% price impact threshold
            parallel_execution_enabled: true,
            gas_optimization_enabled: true,
        }
    }
}

/// Strategy for splitting routes
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SplitStrategy {
    /// Equal distribution across all available routes
    EqualWeight,
    /// Weight by liquidity availability
    LiquidityWeighted,
    /// Weight by price impact minimization
    ImpactOptimized,
    /// Weight by gas cost efficiency
    GasOptimized,
    /// Custom allocation percentages
    Custom,
    /// Dynamic optimization based on market conditions
    Dynamic,
}

/// A split route representing a portion of the total trade
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitRoute {
    pub id: String,
    pub route_path: RoutePath,
    pub allocation_percentage: f64,
    pub amount_in: f64,
    pub expected_amount_out: f64,
    pub execution_priority: u8, // 0 = highest priority
    pub can_execute_parallel: bool,
    pub dependency_routes: Vec<String>, // Routes that must execute before this one
    pub gas_estimate: u64,
    pub estimated_slippage: f64,
}

/// Optimal allocation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimalSplit {
    pub split_routes: Vec<SplitRoute>,
    pub total_expected_output: f64,
    pub total_gas_cost: u64,
    pub total_execution_time: std::time::Duration,
    pub improvement_over_single: f64, // Percentage improvement
    pub confidence_score: f64,
    pub parallel_execution_groups: Vec<Vec<String>>, // Groups of routes that can execute in parallel
}

/// Statistics for split performance monitoring
#[derive(Debug, Clone, Serialize)]
pub struct SplitterStats {
    pub total_splits_generated: usize,
    pub successful_splits: usize,
    pub average_improvement: f64,
    pub average_splits_per_trade: f64,
    pub gas_savings_percentage: f64,
    pub strategy_performance: HashMap<String, StrategyStats>,
}

/// Performance stats for each splitting strategy
#[derive(Debug, Clone, Serialize)]
pub struct StrategyStats {
    pub usage_count: usize,
    pub success_rate: f64,
    pub average_improvement: f64,
    pub average_gas_efficiency: f64,
}

/// Main route splitter implementation
#[derive(Debug)]
pub struct RouteSplitter {
    config: SplitterConfig,
    stats: SplitterStats,
}

impl RouteSplitter {
    /// Create a new route splitter
    pub fn new() -> Self {
        Self {
            config: SplitterConfig::default(),
            stats: SplitterStats {
                total_splits_generated: 0,
                successful_splits: 0,
                average_improvement: 0.0,
                average_splits_per_trade: 0.0,
                gas_savings_percentage: 0.0,
                strategy_performance: HashMap::new(),
            },
        }
    }

    /// Create with custom configuration
    pub fn with_config(config: SplitterConfig) -> Self {
        Self {
            config,
            stats: SplitterStats {
                total_splits_generated: 0,
                successful_splits: 0,
                average_improvement: 0.0,
                average_splits_per_trade: 0.0,
                gas_savings_percentage: 0.0,
                strategy_performance: HashMap::new(),
            },
        }
    }

    /// Generate optimal route splits for a large trade
    pub async fn generate_splits(
        &mut self,
        available_paths: &[RoutePath],
        request: &RouteRequest,
    ) -> Result<Vec<SplitRoute>> {
        self.stats.total_splits_generated += 1;

        // Check if splitting is beneficial
        if !self.should_split_route(request, available_paths) {
            debug!(
                "Route splitting not beneficial for trade size {}",
                (request.amount as f64)
            );
            return Ok(Vec::new());
        }

        info!(
            "Generating route splits for {} {} -> {}",
            (request.amount as f64),
            request.input_token,
            request.output_token
        );

        // Determine optimal splitting strategy
        let strategy = self.determine_optimal_strategy(request, available_paths);

        // Generate splits based on strategy
        let splits = match strategy {
            SplitStrategy::EqualWeight => {
                self.generate_equal_weight_splits(available_paths, request)
                    .await?
            }
            SplitStrategy::LiquidityWeighted => {
                self.generate_liquidity_weighted_splits(available_paths, request)
                    .await?
            }
            SplitStrategy::ImpactOptimized => {
                self.generate_impact_optimized_splits(available_paths, request)
                    .await?
            }
            SplitStrategy::GasOptimized => {
                self.generate_gas_optimized_splits(available_paths, request)
                    .await?
            }
            SplitStrategy::Dynamic => {
                self.generate_dynamic_splits(available_paths, request)
                    .await?
            }
            SplitStrategy::Custom => {
                warn!("Custom split strategy not implemented, using dynamic");
                self.generate_dynamic_splits(available_paths, request)
                    .await?
            }
        };

        // Validate and optimize splits
        let optimized_splits = self.optimize_splits(splits, request).await?;

        if !optimized_splits.is_empty() {
            self.stats.successful_splits += 1;
            self.update_strategy_stats(&strategy, &optimized_splits, request);
        }

        Ok(optimized_splits)
    }

    /// Create optimal split allocation
    pub async fn create_optimal_allocation(
        &self,
        split_routes: Vec<SplitRoute>,
        _total_amount: f64,
    ) -> Result<OptimalSplit> {
        if split_routes.is_empty() {
            return Err(anyhow::anyhow!("No split routes provided"));
        }

        let total_expected_output: f64 = split_routes
            .iter()
            .map(|split| split.expected_amount_out)
            .sum();

        let total_gas_cost: u64 = split_routes.iter().map(|split| split.gas_estimate).sum();

        // Calculate execution time considering parallel execution
        let execution_groups = self.create_execution_groups(&split_routes);
        let total_execution_time =
            self.calculate_total_execution_time(&execution_groups, &split_routes);

        // Calculate improvement over single best route
        let best_single_route = split_routes
            .iter()
            .max_by(|a, b| {
                a.expected_amount_out
                    .partial_cmp(&b.expected_amount_out)
                    .unwrap()
            })
            .unwrap();
        let improvement = ((total_expected_output - best_single_route.expected_amount_out)
            / best_single_route.expected_amount_out)
            * 100.0;

        let confidence_score = self.calculate_split_confidence(&split_routes);

        Ok(OptimalSplit {
            split_routes,
            total_expected_output,
            total_gas_cost,
            total_execution_time,
            improvement_over_single: improvement,
            confidence_score,
            parallel_execution_groups: execution_groups,
        })
    }

    /// Determine if a route should be split
    fn should_split_route(&self, request: &RouteRequest, paths: &[RoutePath]) -> bool {
        // Don't split if amount is too small
        if (request.amount as f64) < self.config.min_split_amount * 2.0 {
            return false;
        }

        // Don't split if only one path available
        if paths.len() < 2 {
            return false;
        }

        // Check if any path has high price impact
        let has_high_impact = paths.iter().any(|path| {
            let avg_impact = path.steps.iter().map(|step| step.price_impact).sum::<f64>()
                / path.steps.len() as f64;
            avg_impact > self.config.price_impact_threshold
        });

        // Split for high priority trades or high impact trades
        match request.speed_priority {
            RoutingPriority::SpeedOptimized => true,
            RoutingPriority::Balanced => has_high_impact || (request.amount as f64) > 10000.0,
            RoutingPriority::CostOptimized => has_high_impact || (request.amount as f64) > 50000.0,
            RoutingPriority::MevProtected => true, // Always split for MEV protection
        }
    }

    /// Determine optimal splitting strategy
    fn determine_optimal_strategy(
        &self,
        request: &RouteRequest,
        paths: &[RoutePath],
    ) -> SplitStrategy {
        // Use dynamic strategy for high-priority trades
        if matches!(
            request.speed_priority,
            RoutingPriority::SpeedOptimized | RoutingPriority::Balanced
        ) {
            return SplitStrategy::Dynamic;
        }

        // Choose strategy based on trade characteristics
        let total_liquidity: f64 = paths.iter().map(|path| path.liquidity_score).sum();

        let avg_gas_cost: f64 = paths
            .iter()
            .map(|path| path.estimated_gas_cost as f64)
            .sum::<f64>()
            / paths.len() as f64;

        // Gas-optimize for smaller trades
        if (request.amount as f64) < 5000.0 && avg_gas_cost > 200_000.0 {
            SplitStrategy::GasOptimized
        }
        // Impact-optimize for large trades
        else if (request.amount as f64) > 50000.0 {
            SplitStrategy::ImpactOptimized
        }
        // Liquidity-weight for balanced trades
        else if total_liquidity > 1_000_000.0 {
            SplitStrategy::LiquidityWeighted
        }
        // Default to dynamic
        else {
            SplitStrategy::Dynamic
        }
    }

    /// Generate equal weight splits
    async fn generate_equal_weight_splits(
        &self,
        paths: &[RoutePath],
        request: &RouteRequest,
    ) -> Result<Vec<SplitRoute>> {
        let num_splits = (paths.len()).min(self.config.max_splits);
        let amount_per_split = (request.amount as f64) / num_splits as f64;

        let mut splits = Vec::new();
        for (i, path) in paths.iter().take(num_splits).enumerate() {
            let split = SplitRoute {
                id: format!("equal_split_{}", i),
                route_path: self.scale_path_for_amount(path, amount_per_split),
                allocation_percentage: 100.0 / num_splits as f64,
                amount_in: amount_per_split,
                expected_amount_out: path.expected_output
                    * (amount_per_split / (request.amount as f64)),
                execution_priority: i as u8,
                can_execute_parallel: true,
                dependency_routes: Vec::new(),
                gas_estimate: path.estimated_gas_cost,
                estimated_slippage: self.estimate_slippage_for_amount(path, amount_per_split),
            };
            splits.push(split);
        }

        Ok(splits)
    }

    /// Generate liquidity-weighted splits
    async fn generate_liquidity_weighted_splits(
        &self,
        paths: &[RoutePath],
        request: &RouteRequest,
    ) -> Result<Vec<SplitRoute>> {
        let total_liquidity: f64 = paths.iter().map(|p| p.liquidity_score).sum();
        if total_liquidity <= 0.0 {
            return self.generate_equal_weight_splits(paths, request).await;
        }

        let mut splits = Vec::new();
        let num_splits = paths.len().min(self.config.max_splits);

        for (i, path) in paths.iter().take(num_splits).enumerate() {
            let weight = path.liquidity_score / total_liquidity;
            let allocation_percentage = weight * 100.0;
            let amount_in = (request.amount as f64) * weight;

            // Skip if amount is too small
            if amount_in < self.config.min_split_amount {
                continue;
            }

            let split = SplitRoute {
                id: format!("liquidity_split_{}", i),
                route_path: self.scale_path_for_amount(path, amount_in),
                allocation_percentage,
                amount_in,
                expected_amount_out: path.expected_output * weight,
                execution_priority: i as u8,
                can_execute_parallel: self.can_execute_parallel(path, paths),
                dependency_routes: Vec::new(),
                gas_estimate: path.estimated_gas_cost,
                estimated_slippage: self.estimate_slippage_for_amount(path, amount_in),
            };
            splits.push(split);
        }

        Ok(splits)
    }

    /// Generate impact-optimized splits
    async fn generate_impact_optimized_splits(
        &self,
        paths: &[RoutePath],
        request: &RouteRequest,
    ) -> Result<Vec<SplitRoute>> {
        let mut splits = Vec::new();
        let mut remaining_amount = request.amount as f64;

        // Sort paths by price impact (ascending)
        let mut sorted_paths = paths.to_vec();
        sorted_paths.sort_by(|a, b| {
            let impact_a = a.steps.iter().map(|s| s.price_impact).sum::<f64>();
            let impact_b = b.steps.iter().map(|s| s.price_impact).sum::<f64>();
            impact_a
                .partial_cmp(&impact_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        for (i, path) in sorted_paths.iter().enumerate() {
            if splits.len() >= self.config.max_splits
                || remaining_amount < self.config.min_split_amount
            {
                break;
            }

            // Calculate optimal amount for this path to minimize price impact
            let optimal_amount = self.calculate_optimal_amount_for_impact(path, remaining_amount);

            if optimal_amount >= self.config.min_split_amount {
                let allocation_percentage = (optimal_amount / (request.amount as f64)) * 100.0;

                let split = SplitRoute {
                    id: format!("impact_split_{}", i),
                    route_path: self.scale_path_for_amount(path, optimal_amount),
                    allocation_percentage,
                    amount_in: optimal_amount,
                    expected_amount_out: path.expected_output
                        * (optimal_amount / (request.amount as f64)),
                    execution_priority: i as u8,
                    can_execute_parallel: true,
                    dependency_routes: Vec::new(),
                    gas_estimate: path.estimated_gas_cost,
                    estimated_slippage: self.estimate_slippage_for_amount(path, optimal_amount),
                };

                splits.push(split);
                remaining_amount -= optimal_amount;
            }
        }

        Ok(splits)
    }

    /// Generate gas-optimized splits
    async fn generate_gas_optimized_splits(
        &self,
        paths: &[RoutePath],
        request: &RouteRequest,
    ) -> Result<Vec<SplitRoute>> {
        // Sort paths by gas efficiency (output per gas unit)
        let mut paths_with_efficiency: Vec<(usize, &RoutePath, f64)> = paths
            .iter()
            .enumerate()
            .map(|(i, path)| {
                let efficiency = path.expected_output / path.estimated_gas_cost as f64;
                (i, path, efficiency)
            })
            .collect();

        paths_with_efficiency
            .sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

        let mut splits = Vec::new();
        let mut remaining_amount = request.amount as f64;

        // Allocate to most gas-efficient paths first
        for (i, (_, path, _)) in paths_with_efficiency.iter().enumerate() {
            if splits.len() >= self.config.max_splits
                || remaining_amount < self.config.min_split_amount
            {
                break;
            }

            // Use larger allocations for more efficient paths
            let base_allocation = remaining_amount / (paths_with_efficiency.len() - i) as f64;
            let amount_in = base_allocation.min(remaining_amount);

            if amount_in >= self.config.min_split_amount {
                let allocation_percentage = (amount_in / (request.amount as f64)) * 100.0;

                let split = SplitRoute {
                    id: format!("gas_split_{}", i),
                    route_path: self.scale_path_for_amount(path, amount_in),
                    allocation_percentage,
                    amount_in,
                    expected_amount_out: path.expected_output
                        * (amount_in / (request.amount as f64)),
                    execution_priority: i as u8,
                    can_execute_parallel: true,
                    dependency_routes: Vec::new(),
                    gas_estimate: path.estimated_gas_cost,
                    estimated_slippage: self.estimate_slippage_for_amount(path, amount_in),
                };

                splits.push(split);
                remaining_amount -= amount_in;
            }
        }

        Ok(splits)
    }

    /// Generate dynamic splits using multiple criteria
    async fn generate_dynamic_splits(
        &self,
        paths: &[RoutePath],
        request: &RouteRequest,
    ) -> Result<Vec<SplitRoute>> {
        // Calculate composite scores for each path
        let mut path_scores: Vec<(usize, &RoutePath, f64)> = paths
            .iter()
            .enumerate()
            .map(|(i, path)| {
                let score = self.calculate_composite_score(path, request);
                (i, path, score)
            })
            .collect();

        // Sort by composite score (descending)
        path_scores.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

        let mut splits = Vec::new();
        let total_score: f64 = path_scores.iter().map(|(_, _, score)| score).sum();

        if total_score <= 0.0 {
            return self.generate_equal_weight_splits(paths, request).await;
        }

        for (i, (_, path, score)) in path_scores.iter().enumerate() {
            if splits.len() >= self.config.max_splits {
                break;
            }

            let weight = score / total_score;
            let amount_in = (request.amount as f64) * weight;

            if amount_in >= self.config.min_split_amount {
                let allocation_percentage = weight * 100.0;

                let split = SplitRoute {
                    id: format!("dynamic_split_{}", i),
                    route_path: self.scale_path_for_amount(path, amount_in),
                    allocation_percentage,
                    amount_in,
                    expected_amount_out: path.expected_output * weight,
                    execution_priority: i as u8,
                    can_execute_parallel: self.can_execute_parallel(path, paths),
                    dependency_routes: Vec::new(),
                    gas_estimate: path.estimated_gas_cost,
                    estimated_slippage: self.estimate_slippage_for_amount(path, amount_in),
                };

                splits.push(split);
            }
        }

        Ok(splits)
    }

    /// Calculate composite score for dynamic splitting
    fn calculate_composite_score(&self, path: &RoutePath, request: &RouteRequest) -> f64 {
        let weights = match request.speed_priority {
            RoutingPriority::SpeedOptimized => [0.4, 0.3, 0.2, 0.1], // [output, speed, liquidity, gas]
            RoutingPriority::Balanced => [0.35, 0.25, 0.25, 0.15],
            RoutingPriority::CostOptimized => [0.3, 0.2, 0.3, 0.2],
            RoutingPriority::MevProtected => [0.25, 0.15, 0.35, 0.25],
        };

        let output_score = path.expected_output / (request.amount as f64);
        let speed_score = 1.0
            / (path
                .execution_time_estimate
                .unwrap_or(Duration::from_millis(500))
                .as_millis() as f64
                / 1000.0);
        let liquidity_score = path.liquidity_score;
        let gas_score = 1.0 / (path.estimated_gas_cost as f64 / 100_000.0);

        let scores = [output_score, speed_score, liquidity_score, gas_score];

        weights.iter().zip(scores.iter()).map(|(w, s)| w * s).sum()
    }

    /// Scale a path for a different amount
    fn scale_path_for_amount(&self, path: &RoutePath, new_amount: f64) -> RoutePath {
        let original_amount = path.steps.first().map(|s| s.amount_in).unwrap_or(1.0);
        let scale_factor = new_amount / original_amount;

        let mut scaled_path = path.clone();
        for step in &mut scaled_path.steps {
            step.amount_in *= scale_factor;
            step.amount_out *= scale_factor;
        }

        scaled_path.expected_output *= scale_factor;
        scaled_path.total_fees *= scale_factor;

        scaled_path
    }

    /// Estimate slippage for a specific amount
    fn estimate_slippage_for_amount(&self, path: &RoutePath, amount: f64) -> f64 {
        // Simplified slippage estimation
        let total_liquidity = path.liquidity_score;
        if total_liquidity <= 0.0 {
            return 0.1; // 10% default for unknown liquidity
        }

        let utilization = amount / total_liquidity;
        (utilization * utilization).min(0.5) // Max 50% slippage
    }

    /// Calculate optimal amount to minimize price impact
    fn calculate_optimal_amount_for_impact(&self, path: &RoutePath, max_amount: f64) -> f64 {
        let min_liquidity = path
            .steps
            .iter()
            .map(|step| step.pool_liquidity)
            .fold(f64::INFINITY, f64::min);

        if min_liquidity.is_finite() && min_liquidity > 0.0 {
            // Use a percentage of the smallest pool's liquidity
            let optimal = min_liquidity * self.config.liquidity_utilization_ratio;
            optimal.min(max_amount)
        } else {
            max_amount * 0.5 // Conservative fallback
        }
    }

    /// Check if a path can execute in parallel with others
    fn can_execute_parallel(&self, path: &RoutePath, all_paths: &[RoutePath]) -> bool {
        if !self.config.parallel_execution_enabled {
            return false;
        }

        // Check for pool overlap that would prevent parallel execution
        let path_pools: std::collections::HashSet<_> =
            path.steps.iter().map(|step| step.pool_address).collect();

        // If this path shares pools with others, it might not be parallelizable
        let has_overlap = all_paths.iter().any(|other_path| {
            if std::ptr::eq(path, other_path) {
                return false;
            }

            other_path
                .steps
                .iter()
                .any(|step| path_pools.contains(&step.pool_address))
        });

        !has_overlap
    }

    /// Optimize splits after generation
    async fn optimize_splits(
        &self,
        mut splits: Vec<SplitRoute>,
        request: &RouteRequest,
    ) -> Result<Vec<SplitRoute>> {
        if splits.is_empty() {
            return Ok(splits);
        }

        // Remove splits that are too small
        splits.retain(|split| split.amount_in >= self.config.min_split_amount);

        // Rebalance if total allocation doesn't match request amount
        let total_allocated: f64 = splits.iter().map(|s| s.amount_in).sum();
        if (total_allocated - (request.amount as f64)).abs() > 0.01 {
            let scale_factor = (request.amount as f64) / total_allocated;
            for split in &mut splits {
                split.amount_in *= scale_factor;
                split.expected_amount_out *= scale_factor;
                split.allocation_percentage *= scale_factor;
            }
        }

        // Sort by execution priority
        splits.sort_by_key(|split| split.execution_priority);

        // Update parallel execution flags
        for i in 0..splits.len() {
            splits[i].can_execute_parallel = self.config.parallel_execution_enabled
                && !self.has_dependency_conflicts(&splits[i], &splits);
        }

        Ok(splits)
    }

    /// Check for dependency conflicts
    fn has_dependency_conflicts(&self, split: &SplitRoute, all_splits: &[SplitRoute]) -> bool {
        let split_pools: std::collections::HashSet<_> = split
            .route_path
            .steps
            .iter()
            .map(|step| step.pool_address)
            .collect();

        all_splits.iter().any(|other| {
            if other.id == split.id {
                return false;
            }

            other
                .route_path
                .steps
                .iter()
                .any(|step| split_pools.contains(&step.pool_address))
        })
    }

    /// Create execution groups for parallel processing
    fn create_execution_groups(&self, splits: &[SplitRoute]) -> Vec<Vec<String>> {
        let mut groups = Vec::new();
        let mut assigned = std::collections::HashSet::new();

        for split in splits {
            if assigned.contains(&split.id) {
                continue;
            }

            if split.can_execute_parallel {
                let mut group = vec![split.id.clone()];
                assigned.insert(split.id.clone());

                // Find other splits that can execute in parallel with this one
                for other in splits {
                    if !assigned.contains(&other.id)
                        && other.can_execute_parallel
                        && !self.routes_conflict(&split.route_path, &other.route_path)
                    {
                        group.push(other.id.clone());
                        assigned.insert(other.id.clone());
                    }
                }
                groups.push(group);
            } else {
                groups.push(vec![split.id.clone()]);
                assigned.insert(split.id.clone());
            }
        }

        groups
    }

    /// Check if two routes conflict (share pools)
    fn routes_conflict(&self, route_a: &RoutePath, route_b: &RoutePath) -> bool {
        let pools_a: std::collections::HashSet<_> =
            route_a.steps.iter().map(|step| step.pool_address).collect();

        route_b
            .steps
            .iter()
            .any(|step| pools_a.contains(&step.pool_address))
    }

    /// Calculate total execution time considering parallel execution
    fn calculate_total_execution_time(
        &self,
        execution_groups: &[Vec<String>],
        splits: &[SplitRoute],
    ) -> std::time::Duration {
        let mut total_time = std::time::Duration::from_millis(0);

        for group in execution_groups {
            let group_time = group
                .iter()
                .filter_map(|split_id| splits.iter().find(|s| &s.id == split_id))
                .map(|split| split.route_path.execution_time_estimate)
                .max()
                .unwrap_or(Some(std::time::Duration::from_millis(0)));

            total_time += group_time.unwrap_or(Duration::from_millis(0));
        }

        total_time
    }

    /// Calculate confidence score for splits
    fn calculate_split_confidence(&self, splits: &[SplitRoute]) -> f64 {
        if splits.is_empty() {
            return 0.0;
        }

        let avg_confidence = splits
            .iter()
            .map(|split| split.route_path.confidence_score)
            .sum::<f64>()
            / splits.len() as f64;

        let diversification_bonus = if splits.len() > 1 { 0.1 } else { 0.0 };

        (avg_confidence + diversification_bonus).min(1.0)
    }

    /// Update strategy performance statistics
    fn update_strategy_stats(
        &mut self,
        strategy: &SplitStrategy,
        splits: &[SplitRoute],
        request: &RouteRequest,
    ) {
        let strategy_name = format!("{:?}", strategy);
        let stats = self
            .stats
            .strategy_performance
            .entry(strategy_name)
            .or_insert(StrategyStats {
                usage_count: 0,
                success_rate: 0.0,
                average_improvement: 0.0,
                average_gas_efficiency: 0.0,
            });

        stats.usage_count += 1;
        let count = stats.usage_count as f64;

        // Calculate improvement (simplified)
        let total_output: f64 = splits.iter().map(|s| s.expected_amount_out).sum();
        let improvement = (total_output - (request.amount as f64)) / (request.amount as f64);
        stats.average_improvement =
            (stats.average_improvement * (count - 1.0) + improvement) / count;

        // Update gas efficiency
        let total_gas: u64 = splits.iter().map(|s| s.gas_estimate).sum();
        let gas_efficiency = total_output / total_gas as f64;
        stats.average_gas_efficiency =
            (stats.average_gas_efficiency * (count - 1.0) + gas_efficiency) / count;
    }

    /// Get splitter statistics for monitoring
    pub fn get_stats(&self) -> &SplitterStats {
        &self.stats
    }

    /// Split a single route into multiple optimal sub-routes
    pub async fn split_route(
        &mut self,
        route: &RoutePath,
        amount: &u64,
        _strategy: SplitStrategy,
    ) -> Result<Option<OptimalSplit>> {
        // For now, convert single route to available paths format
        let available_paths = vec![route.clone()];

        // Create a simplified RouteRequest from parameters
        let request = RouteRequest {
            input_token: route
                .steps
                .first()
                .map(|s| s.from_token.to_string())
                .unwrap_or_default(),
            output_token: route
                .steps
                .last()
                .map(|s| s.to_token.to_string())
                .unwrap_or_default(),
            amount: *amount,
            max_slippage: None,
            max_price_impact: None,
            preferred_dexs: None,
            excluded_dexs: None,
            max_hops: None,
            enable_splitting: true,
            speed_priority: RoutingPriority::Balanced,
            timestamp: SystemTime::now(),
            constraints: crate::arbitrage::routing::smart_router::RouteConstraints::default(),
            min_amount_out: None,
        };

        // Generate splits
        let splits = self.generate_splits(&available_paths, &request).await?;

        if splits.is_empty() {
            return Ok(None);
        }

        // Create optimal allocation from splits
        self.create_optimal_allocation(splits, *amount as f64)
            .await
            .map(Some)
    }
}

impl Default for RouteSplitter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arbitrage::routing::pathfinder::RoutePath;
    use crate::utils::DexType;

    fn create_test_route_path(expected_output: f64, gas_cost: u64) -> RoutePath {
        RoutePath {
            steps: vec![RouteStep {
                pool_address: Pubkey::new_unique(),
                dex_type: DexType::Raydium,
                from_token: Pubkey::new_unique(),
                to_token: Pubkey::new_unique(),
                amount_in: 1000.0,
                amount_out: expected_output,
                fee_bps: 25,
                pool_liquidity: 100_000.0,
                price_impact: 0.01,
                execution_order: 0,
                slippage_tolerance: Some(0.01),
            }],
            total_fees: 2.5,
            total_weight: 0.1,
            expected_output,
            estimated_gas_cost: gas_cost,
            estimated_gas_fee: Some(gas_cost),
            execution_time_estimate: Some(std::time::Duration::from_millis(500)),
            confidence_score: 0.8,
            liquidity_score: 0.7,
            dex_diversity_score: 1.0,
            price_impact: Some(0.01),
        }
    }

    fn create_test_request() -> RouteRequest {
        RouteRequest {
            input_token: "SOL".to_string(),
            output_token: "USDC".to_string(),
            amount: 10000,
            max_slippage: Some(0.05),
            max_price_impact: None,
            preferred_dexs: None,
            excluded_dexs: None,
            max_hops: Some(3),
            enable_splitting: true,
            speed_priority: RoutingPriority::Balanced,
            timestamp: SystemTime::now(),
            constraints: crate::arbitrage::routing::smart_router::RouteConstraints::default(),
            min_amount_out: Some(9000),
        }
    }

    #[test]
    fn test_route_splitter_creation() {
        let splitter = RouteSplitter::new();
        assert_eq!(splitter.stats.total_splits_generated, 0);
    }

    #[test]
    fn test_should_split_route() {
        let splitter = RouteSplitter::new();
        let paths = vec![
            create_test_route_path(990.0, 150_000),
            create_test_route_path(985.0, 160_000),
        ];

        let request = create_test_request();
        assert!(splitter.should_split_route(&request, &paths));

        // Test with small amount
        let small_request = RouteRequest {
            amount: 50,
            ..request
        };
        assert!(!splitter.should_split_route(&small_request, &paths));
    }

    #[tokio::test]
    async fn test_equal_weight_splits() {
        let splitter = RouteSplitter::new();
        let paths = vec![
            create_test_route_path(990.0, 150_000),
            create_test_route_path(985.0, 160_000),
        ];

        let request = create_test_request();
        let splits = splitter
            .generate_equal_weight_splits(&paths, &request)
            .await
            .unwrap();

        assert_eq!(splits.len(), 2);
        assert!((splits[0].allocation_percentage - 50.0).abs() < 0.01);
        assert!((splits[1].allocation_percentage - 50.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_liquidity_weighted_splits() {
        let splitter = RouteSplitter::new();
        let mut path1 = create_test_route_path(990.0, 150_000);
        let mut path2 = create_test_route_path(985.0, 160_000);

        path1.liquidity_score = 0.8;
        path2.liquidity_score = 0.4;

        let paths = vec![path1, path2];
        let request = create_test_request();

        let splits = splitter
            .generate_liquidity_weighted_splits(&paths, &request)
            .await
            .unwrap();

        assert_eq!(splits.len(), 2);
        // First split should have higher allocation due to higher liquidity
        assert!(splits[0].allocation_percentage > splits[1].allocation_percentage);
    }

    #[tokio::test]
    async fn test_generate_splits() {
        let mut splitter = RouteSplitter::new();
        let paths = vec![
            create_test_route_path(990.0, 150_000),
            create_test_route_path(985.0, 160_000),
            create_test_route_path(980.0, 140_000),
        ];

        let request = create_test_request();
        let splits = splitter.generate_splits(&paths, &request).await.unwrap();

        assert!(!splits.is_empty());
        assert!(splits.len() <= splitter.config.max_splits);

        // Verify total allocation
        let total_allocation: f64 = splits.iter().map(|s| s.amount_in).sum();
        assert!((total_allocation - (request.amount as f64)).abs() < 1.0);
    }

    #[tokio::test]
    async fn test_optimal_allocation() {
        let splitter = RouteSplitter::new();
        let splits = vec![
            SplitRoute {
                id: "split1".to_string(),
                route_path: create_test_route_path(495.0, 150_000),
                allocation_percentage: 50.0,
                amount_in: 5000.0,
                expected_amount_out: 495.0,
                execution_priority: 0,
                can_execute_parallel: true,
                dependency_routes: Vec::new(),
                gas_estimate: 150_000,
                estimated_slippage: 0.01,
            },
            SplitRoute {
                id: "split2".to_string(),
                route_path: create_test_route_path(490.0, 160_000),
                allocation_percentage: 50.0,
                amount_in: 5000.0,
                expected_amount_out: 490.0,
                execution_priority: 1,
                can_execute_parallel: true,
                dependency_routes: Vec::new(),
                gas_estimate: 160_000,
                estimated_slippage: 0.015,
            },
        ];

        let allocation = splitter
            .create_optimal_allocation(splits, 10000.0)
            .await
            .unwrap();

        assert_eq!(allocation.split_routes.len(), 2);
        assert!((allocation.total_expected_output - 985.0).abs() < 0.01);
        assert_eq!(allocation.total_gas_cost, 310_000);
        assert!(!allocation.parallel_execution_groups.is_empty());
    }

    #[test]
    fn test_composite_score_calculation() {
        let splitter = RouteSplitter::new();
        let path = create_test_route_path(990.0, 150_000);
        let request = create_test_request();

        let score = splitter.calculate_composite_score(&path, &request);
        assert!(score > 0.0);
    }

    #[test]
    fn test_execution_groups() {
        let splitter = RouteSplitter::new();
        let splits = vec![
            SplitRoute {
                id: "split1".to_string(),
                route_path: create_test_route_path(495.0, 150_000),
                allocation_percentage: 33.3,
                amount_in: 3333.0,
                expected_amount_out: 495.0,
                execution_priority: 0,
                can_execute_parallel: true,
                dependency_routes: Vec::new(),
                gas_estimate: 150_000,
                estimated_slippage: 0.01,
            },
            SplitRoute {
                id: "split2".to_string(),
                route_path: create_test_route_path(490.0, 160_000),
                allocation_percentage: 33.3,
                amount_in: 3333.0,
                expected_amount_out: 490.0,
                execution_priority: 1,
                can_execute_parallel: true,
                dependency_routes: Vec::new(),
                gas_estimate: 160_000,
                estimated_slippage: 0.015,
            },
        ];

        let groups = splitter.create_execution_groups(&splits);
        assert!(!groups.is_empty());

        // Since routes don't conflict, they should be in the same group
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 2);
    }
}
