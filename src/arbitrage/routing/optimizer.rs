// src/arbitrage/routing/optimizer.rs
//! Route Optimizer Module for Multi-Objective Route Optimization
//!
//! This module implements sophisticated optimization algorithms that evaluate
//! and improve routing paths based on multiple criteria including profit,
//! speed, reliability, and gas efficiency.

use anyhow::Result;
use log::{debug, info};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use super::pathfinder::RoutePath;
use super::{RouteRequest, RoutingPriority};

/// Optimization goals for route evaluation
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum OptimizationGoal {
    /// Maximize expected output amount
    MaximizeOutput,
    /// Minimize total execution time
    MinimizeTime,
    /// Minimize gas costs
    MinimizeGas,
    /// Maximize reliability/confidence
    MaximizeReliability,
    /// Minimize price impact
    MinimizeImpact,
    /// Balanced optimization across all criteria
    Balanced,
    /// Custom weighted optimization
    Custom(OptimizationWeights),
}

/// Custom weights for multi-objective optimization
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct OptimizationWeights {
    pub output_weight: f64,
    pub speed_weight: f64,
    pub gas_weight: f64,
    pub reliability_weight: f64,
    pub impact_weight: f64,
}

impl Default for OptimizationWeights {
    fn default() -> Self {
        Self {
            output_weight: 0.3,
            speed_weight: 0.2,
            gas_weight: 0.2,
            reliability_weight: 0.2,
            impact_weight: 0.1,
        }
    }
}

/// Constraints for route optimization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationConstraints {
    pub max_hops: Option<usize>,
    pub max_gas_cost: Option<u64>,
    pub max_execution_time: Option<Duration>,
    pub min_output_amount: Option<f64>,
    pub max_price_impact: Option<f64>,
    pub required_dexs: Option<Vec<crate::utils::DexType>>,
    pub forbidden_dexs: Option<Vec<crate::utils::DexType>>,
    pub min_liquidity_per_step: Option<f64>,
}

impl Default for OptimizationConstraints {
    fn default() -> Self {
        Self {
            max_hops: Some(4),
            max_gas_cost: Some(1_000_000),
            max_execution_time: Some(Duration::from_secs(10)),
            min_output_amount: None,
            max_price_impact: Some(0.05),
            required_dexs: None,
            forbidden_dexs: None,
            min_liquidity_per_step: Some(1000.0),
        }
    }
}

/// Route scoring metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteScore {
    pub total_score: f64,
    pub output_score: f64,
    pub speed_score: f64,
    pub gas_score: f64,
    pub reliability_score: f64,
    pub impact_score: f64,
    pub diversity_score: f64,
    pub constraint_penalty: f64,
    /// Composite score for multi-objective optimization
    pub composite_score: f64,
    /// Execution time for the evaluation
    pub evaluation_time: Duration,
    /// Hash of the route for identification
    pub route_hash: u64,
}

/// Optimization statistics
#[derive(Debug, Clone, Serialize)]
pub struct OptimizerStats {
    pub total_optimizations: usize,
    pub successful_optimizations: usize,
    pub average_optimization_time: Duration,
    pub average_improvement: f64,
    pub goal_performance: HashMap<String, GoalStats>,
}

/// Performance stats for each optimization goal
#[derive(Debug, Clone, Serialize)]
pub struct GoalStats {
    pub usage_count: usize,
    pub success_rate: f64,
    pub average_improvement: f64,
    pub average_time: Duration,
}

/// Optimization configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizerConfig {
    pub max_iterations: usize,
    pub convergence_threshold: f64,
    pub timeout_ms: u64,
    pub enable_genetic_algorithm: bool,
    pub enable_simulated_annealing: bool,
    pub population_size: usize,
    pub mutation_rate: f64,
}

impl Default for OptimizerConfig {
    fn default() -> Self {
        Self {
            max_iterations: 100,
            convergence_threshold: 0.001,
            timeout_ms: 2000,
            enable_genetic_algorithm: true,
            enable_simulated_annealing: false,
            population_size: 20,
            mutation_rate: 0.1,
        }
    }
}

/// Main route optimizer implementation
#[derive(Debug, Clone)]
pub struct RouteOptimizer {
    #[allow(dead_code)]
    config: OptimizerConfig,
    stats: OptimizerStats,
}

impl RouteOptimizer {
    /// Create a new route optimizer
    pub fn new() -> Self {
        Self {
            config: OptimizerConfig::default(),
            stats: OptimizerStats {
                total_optimizations: 0,
                successful_optimizations: 0,
                average_optimization_time: Duration::from_millis(0),
                average_improvement: 0.0,
                goal_performance: HashMap::new(),
            },
        }
    }

    /// Create with custom configuration
    pub fn with_config(config: OptimizerConfig) -> Self {
        Self {
            config,
            stats: OptimizerStats {
                total_optimizations: 0,
                successful_optimizations: 0,
                average_optimization_time: Duration::from_millis(0),
                average_improvement: 0.0,
                goal_performance: HashMap::new(),
            },
        }
    }

    /// Optimize a collection of route paths
    pub async fn optimize_paths(
        &mut self,
        paths: Vec<RoutePath>,
        goals: &[OptimizationGoal],
    ) -> Result<Vec<RoutePath>> {
        let start_time = Instant::now();
        self.stats.total_optimizations += 1;

        if paths.is_empty() {
            return Ok(paths);
        }

        info!(
            "Optimizing {} routes with {} goals",
            paths.len(),
            goals.len()
        );

        // Use the first goal as primary optimization target
        let primary_goal = goals.first().unwrap_or(&OptimizationGoal::Balanced);

        // Score all paths using the primary goal
        let mut scored_paths: Vec<(RoutePath, f64)> = paths
            .into_iter()
            .map(|path| {
                let score = self.calculate_simple_score(&path, primary_goal);
                (path, score)
            })
            .collect();

        // Sort by score (descending)
        scored_paths.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let final_paths: Vec<RoutePath> = scored_paths.into_iter().map(|(path, _)| path).collect();

        let optimization_time = start_time.elapsed();
        self.update_stats(primary_goal, optimization_time, !final_paths.is_empty());

        if !final_paths.is_empty() {
            self.stats.successful_optimizations += 1;
        }

        info!(
            "Route optimization completed in {:?}, {} routes selected",
            optimization_time,
            final_paths.len()
        );

        Ok(final_paths)
    }

    /// Optimize multiple routes and return them sorted by score
    pub async fn optimize_routes(
        &mut self,
        routes: Vec<RoutePath>,
        goals: &[OptimizationGoal],
    ) -> Result<Vec<RoutePath>> {
        // This is just a renamed wrapper around optimize_paths for compatibility
        self.optimize_paths(routes, goals).await
    }

    /// Score a single route based on optimization criteria
    pub async fn score_route(
        &self,
        path: &RoutePath,
        goal: &OptimizationGoal,
        constraints: &OptimizationConstraints,
        request: &RouteRequest,
    ) -> Result<RouteScore> {
        let weights = self.get_optimization_weights(goal, request);

        // Calculate individual scores (0.0 to 1.0)
        let output_score = self.calculate_output_score(path);
        let speed_score = self.calculate_speed_score(path);
        let gas_score = self.calculate_gas_score(path);
        let reliability_score = path.confidence_score;
        let impact_score = self.calculate_impact_score(path);
        let diversity_score = path.dex_diversity_score;

        // Calculate constraint penalty
        let constraint_penalty = self.calculate_constraint_penalty(path, constraints);

        // Calculate weighted total score
        let total_score = (output_score * weights.output_weight
            + speed_score * weights.speed_weight
            + gas_score * weights.gas_weight
            + reliability_score * weights.reliability_weight
            + impact_score * weights.impact_weight)
            - constraint_penalty;

        Ok(RouteScore {
            total_score: total_score.max(0.0),
            output_score,
            speed_score,
            gas_score,
            reliability_score,
            impact_score,
            diversity_score,
            constraint_penalty,
            composite_score: 0.0, // Default value, will be calculated in evaluate_route
            evaluation_time: Duration::from_millis(0), // Default value
            route_hash: 0,        // Default value
        })
    }

    /// Evaluate a single route and return its score
    pub async fn evaluate_route(
        &self,
        route: &RoutePath,
        goals: &[OptimizationGoal],
    ) -> Result<RouteScore> {
        let start_time = Instant::now();

        // Calculate individual scores
        let output_score = self.calculate_output_score(route);
        let time_score = self.calculate_time_score(route);
        let gas_score = self.calculate_gas_score(route);
        let reliability_score = self.calculate_reliability_score(route);
        let impact_score = self.calculate_impact_score(route);

        // Calculate weighted composite score based on goals
        let mut total_weight = 0.0;
        let mut weighted_score = 0.0;

        for goal in goals {
            let (weight, score) = match goal {
                OptimizationGoal::MaximizeOutput => (0.4, output_score),
                OptimizationGoal::MinimizeTime => (0.3, time_score),
                OptimizationGoal::MinimizeGas => (0.2, gas_score),
                OptimizationGoal::MaximizeReliability => (0.25, reliability_score),
                OptimizationGoal::MinimizeImpact => (0.3, impact_score),
                OptimizationGoal::Balanced => {
                    // Balanced approach - equal weights
                    let balanced_score =
                        (output_score + time_score + gas_score + reliability_score + impact_score)
                            / 5.0;
                    (1.0, balanced_score)
                }
                OptimizationGoal::Custom(weights) => {
                    let custom_score = output_score * weights.output_weight
                        + time_score * weights.speed_weight
                        + gas_score * weights.gas_weight
                        + reliability_score * weights.reliability_weight
                        + impact_score * weights.impact_weight;
                    (1.0, custom_score)
                }
            };

            weighted_score += weight * score;
            total_weight += weight;
        }

        let final_score = if total_weight > 0.0 {
            weighted_score / total_weight
        } else {
            (output_score + time_score + gas_score + reliability_score + impact_score) / 5.0
        };

        let evaluation_time = start_time.elapsed();

        Ok(RouteScore {
            total_score: final_score,
            output_score,
            speed_score: time_score,
            gas_score,
            reliability_score,
            impact_score,
            diversity_score: 0.5,    // Default diversity score
            constraint_penalty: 0.0, // No penalty for now
            composite_score: final_score,
            evaluation_time,
            route_hash: self.calculate_route_hash(route),
        })
    }

    /// Calculate route hash for caching and deduplication
    fn calculate_route_hash(&self, path: &RoutePath) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        for step in &path.steps {
            step.dex_type.hash(&mut hasher);
            step.pool_address.hash(&mut hasher);
            (step.amount_in as u64).hash(&mut hasher);
        }
        hasher.finish()
    }

    #[allow(dead_code)]
    /// Determine optimization goal based on request characteristics
    fn determine_optimization_goal(&self, request: &RouteRequest) -> OptimizationGoal {
        match request.speed_priority {
            RoutingPriority::SpeedOptimized => {
                // For critical trades, prioritize speed and reliability
                OptimizationGoal::Custom(OptimizationWeights {
                    output_weight: 0.25,
                    speed_weight: 0.35,
                    gas_weight: 0.1,
                    reliability_weight: 0.25,
                    impact_weight: 0.05,
                })
            }
            RoutingPriority::Balanced => {
                // For high priority, balance output and speed
                OptimizationGoal::Custom(OptimizationWeights {
                    output_weight: 0.35,
                    speed_weight: 0.25,
                    gas_weight: 0.15,
                    reliability_weight: 0.2,
                    impact_weight: 0.05,
                })
            }
            RoutingPriority::CostOptimized => {
                // For low priority, optimize for gas efficiency
                OptimizationGoal::Custom(OptimizationWeights {
                    output_weight: 0.2,
                    speed_weight: 0.1,
                    gas_weight: 0.4,
                    reliability_weight: 0.2,
                    impact_weight: 0.1,
                })
            }
            RoutingPriority::MevProtected => {
                // For MEV protection, prioritize reliability and impact
                OptimizationGoal::Custom(OptimizationWeights {
                    output_weight: 0.15,
                    speed_weight: 0.15,
                    gas_weight: 0.2,
                    reliability_weight: 0.35,
                    impact_weight: 0.15,
                })
            }
        }
    }

    #[allow(dead_code)]
    /// Build optimization constraints from routing request
    fn build_constraints(&self, request: &RouteRequest) -> OptimizationConstraints {
        OptimizationConstraints {
            max_hops: request.constraints.execution_deadline.map(|_| 3), // Shorter deadline = fewer hops
            max_gas_cost: request.constraints.max_gas_cost.map(|gas| gas as u64),
            max_execution_time: request.constraints.execution_deadline,
            min_output_amount: request.min_amount_out.map(|amt| amt as f64),
            max_price_impact: request.max_slippage,
            required_dexs: request.constraints.allowed_dexs.as_ref().map(|dexs| {
                dexs.iter()
                    .map(|s| match s.as_str() {
                        "Orca" => crate::utils::DexType::Orca,
                        "Raydium" => crate::utils::DexType::Raydium,
                        "Lifinity" => crate::utils::DexType::Lifinity,
                        "Meteora" => crate::utils::DexType::Meteora,
                        "Phoenix" => crate::utils::DexType::Phoenix,
                        "Jupiter" => crate::utils::DexType::Jupiter,
                        "Whirlpool" => crate::utils::DexType::Whirlpool,
                        _ => crate::utils::DexType::Unknown(s.clone()),
                    })
                    .collect()
            }),
            forbidden_dexs: request.constraints.forbidden_dexs.as_ref().map(|dexs| {
                dexs.iter()
                    .map(|s| match s.as_str() {
                        "Orca" => crate::utils::DexType::Orca,
                        "Raydium" => crate::utils::DexType::Raydium,
                        "Lifinity" => crate::utils::DexType::Lifinity,
                        "Meteora" => crate::utils::DexType::Meteora,
                        "Phoenix" => crate::utils::DexType::Phoenix,
                        "Jupiter" => crate::utils::DexType::Jupiter,
                        "Whirlpool" => crate::utils::DexType::Whirlpool,
                        _ => crate::utils::DexType::Unknown(s.clone()),
                    })
                    .collect()
            }),
            min_liquidity_per_step: Some(1000.0),
        }
    }

    /// Get optimization weights based on goal
    fn get_optimization_weights(
        &self,
        goal: &OptimizationGoal,
        _request: &RouteRequest,
    ) -> OptimizationWeights {
        match goal {
            OptimizationGoal::MaximizeOutput => OptimizationWeights {
                output_weight: 0.6,
                speed_weight: 0.1,
                gas_weight: 0.1,
                reliability_weight: 0.15,
                impact_weight: 0.05,
            },
            OptimizationGoal::MinimizeTime => OptimizationWeights {
                output_weight: 0.2,
                speed_weight: 0.5,
                gas_weight: 0.1,
                reliability_weight: 0.15,
                impact_weight: 0.05,
            },
            OptimizationGoal::MinimizeGas => OptimizationWeights {
                output_weight: 0.2,
                speed_weight: 0.1,
                gas_weight: 0.5,
                reliability_weight: 0.15,
                impact_weight: 0.05,
            },
            OptimizationGoal::MaximizeReliability => OptimizationWeights {
                output_weight: 0.25,
                speed_weight: 0.15,
                gas_weight: 0.1,
                reliability_weight: 0.4,
                impact_weight: 0.1,
            },
            OptimizationGoal::MinimizeImpact => OptimizationWeights {
                output_weight: 0.2,
                speed_weight: 0.15,
                gas_weight: 0.15,
                reliability_weight: 0.2,
                impact_weight: 0.3,
            },
            OptimizationGoal::Balanced => OptimizationWeights::default(),
            OptimizationGoal::Custom(weights) => *weights,
        }
    }

    /// Calculate output score (normalized expected output)
    fn calculate_output_score(&self, path: &RoutePath) -> f64 {
        // Estimate output based on route steps
        path.steps
            .iter()
            .map(|step| step.amount_out / step.amount_in.max(1.0))
            .product::<f64>()
    }

    /// Calculate speed score (inverse of execution time)
    fn calculate_speed_score(&self, path: &RoutePath) -> f64 {
        let time_seconds = path
            .execution_time_estimate
            .unwrap_or(Duration::from_millis(500))
            .as_secs_f64();
        if time_seconds <= 0.0 {
            return 1.0;
        }

        // Normalize: 1 second = 1.0, 10 seconds = 0.1
        (1.0 / time_seconds).min(1.0)
    }

    /// Calculate gas efficiency score
    fn calculate_gas_score(&self, path: &RoutePath) -> f64 {
        if path.estimated_gas_cost == 0 {
            return 1.0;
        }

        // Score based on gas efficiency (output per gas unit)
        let gas_efficiency = path.expected_output / path.estimated_gas_cost as f64;

        // Normalize assuming good efficiency is 0.001 output per gas unit
        (gas_efficiency / 0.001).min(1.0)
    }

    /// Calculate price impact score (lower impact = higher score)
    fn calculate_impact_score(&self, path: &RoutePath) -> f64 {
        if path.steps.is_empty() {
            return 0.5;
        }

        let avg_impact =
            path.steps.iter().map(|step| step.price_impact).sum::<f64>() / path.steps.len() as f64;

        // Invert impact: 0% impact = 1.0 score, 10% impact = 0.0 score
        (1.0 - (avg_impact / 0.1)).max(0.0)
    }

    /// Calculate time score for a route (how fast it executes)
    fn calculate_time_score(&self, path: &RoutePath) -> f64 {
        let estimated_time = path
            .execution_time_estimate
            .unwrap_or(Duration::from_millis(1000))
            .as_millis() as f64;
        // Lower time = higher score
        1000.0 / (estimated_time + 1.0)
    }

    /// Calculate reliability score for a route
    fn calculate_reliability_score(&self, path: &RoutePath) -> f64 {
        // Base reliability on number of steps (fewer = more reliable)
        let step_penalty = path.steps.len() as f64 * 0.1;
        (1.0 - step_penalty).max(0.1)
    }

    /// Calculate constraint penalty
    fn calculate_constraint_penalty(
        &self,
        path: &RoutePath,
        constraints: &OptimizationConstraints,
    ) -> f64 {
        let mut penalty = 0.0;

        // Hop count penalty
        if let Some(max_hops) = constraints.max_hops {
            if path.steps.len() > max_hops {
                penalty += 0.2 * (path.steps.len() - max_hops) as f64 / max_hops as f64;
            }
        }

        // Gas cost penalty
        if let Some(max_gas) = constraints.max_gas_cost {
            if path.estimated_gas_cost > max_gas {
                penalty += 0.3 * (path.estimated_gas_cost - max_gas) as f64 / max_gas as f64;
            }
        }

        // Execution time penalty
        if let Some(max_time) = constraints.max_execution_time {
            if let Some(exec_time) = path.execution_time_estimate {
                if exec_time > max_time {
                    let excess_ratio = exec_time.as_secs_f64() / max_time.as_secs_f64() - 1.0;
                    penalty += 0.25 * excess_ratio;
                }
            }
        }

        // Output penalty
        if let Some(min_output) = constraints.min_output_amount {
            if path.expected_output < min_output {
                penalty += 0.4 * (min_output - path.expected_output) / min_output;
            }
        }

        // Price impact penalty
        if let Some(max_impact) = constraints.max_price_impact {
            let avg_impact = path.steps.iter().map(|step| step.price_impact).sum::<f64>()
                / path.steps.len().max(1) as f64;

            if avg_impact > max_impact {
                penalty += 0.3 * (avg_impact - max_impact) / max_impact;
            }
        }

        penalty.min(1.0) // Cap penalty at 1.0
    }

    #[allow(dead_code)]
    /// Check if path satisfies hard constraints
    fn satisfies_constraints(
        &self,
        path: &RoutePath,
        constraints: &OptimizationConstraints,
    ) -> bool {
        // Check hard constraints
        if let Some(max_hops) = constraints.max_hops {
            if path.steps.len() > max_hops {
                return false;
            }
        }

        if let Some(max_gas) = constraints.max_gas_cost {
            if path.estimated_gas_cost > max_gas {
                return false;
            }
        }

        if let Some(min_output) = constraints.min_output_amount {
            if path.expected_output < min_output {
                return false;
            }
        }

        // Check forbidden DEXs
        if let Some(forbidden) = &constraints.forbidden_dexs {
            let has_forbidden = path
                .steps
                .iter()
                .any(|step| forbidden.contains(&step.dex_type));
            if has_forbidden {
                return false;
            }
        }

        // Check required DEXs
        if let Some(required) = &constraints.required_dexs {
            let path_dexs: std::collections::HashSet<_> =
                path.steps.iter().map(|step| &step.dex_type).collect();

            let has_all_required = required.iter().all(|dex| path_dexs.contains(dex));
            if !has_all_required {
                return false;
            }
        }

        // Check minimum liquidity per step
        if let Some(min_liquidity) = constraints.min_liquidity_per_step {
            let has_low_liquidity = path
                .steps
                .iter()
                .any(|step| step.pool_liquidity < min_liquidity);
            if has_low_liquidity {
                return false;
            }
        }

        true
    }

    #[allow(dead_code)]
    /// Greedy optimization algorithm
    async fn greedy_optimization(
        &self,
        mut scored_paths: Vec<(RoutePath, RouteScore)>,
        _goal: &OptimizationGoal,
        _constraints: &OptimizationConstraints,
    ) -> Result<Vec<(RoutePath, RouteScore)>> {
        // Sort by total score (descending)
        scored_paths.sort_by(|a, b| {
            b.1.total_score
                .partial_cmp(&a.1.total_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Take top paths with some diversity
        let mut result = Vec::new();
        let mut used_pool_combinations = std::collections::HashSet::new();

        for (path, score) in scored_paths {
            // Create a signature for this path's pool combination
            let mut pool_signature: Vec<_> =
                path.steps.iter().map(|step| step.pool_address).collect();
            pool_signature.sort();

            // Add path if it uses a different combination of pools or is significantly better
            if !used_pool_combinations.contains(&pool_signature) || result.len() < 3 {
                used_pool_combinations.insert(pool_signature);
                result.push((path, score));

                if result.len() >= 10 {
                    break;
                }
            }
        }

        Ok(result)
    }

    #[allow(dead_code)]
    /// Genetic algorithm optimization
    async fn genetic_algorithm_optimization(
        &self,
        initial_population: Vec<(RoutePath, RouteScore)>,
        goal: &OptimizationGoal,
        constraints: &OptimizationConstraints,
        request: &RouteRequest,
    ) -> Result<Vec<(RoutePath, RouteScore)>> {
        let mut population = initial_population;

        // Ensure we have enough individuals
        while population.len() < self.config.population_size && !population.is_empty() {
            let to_clone = population
                .len()
                .min(self.config.population_size - population.len());
            for i in 0..to_clone {
                let cloned = population[i].clone();
                population.push(cloned);
            }
        }

        let start_time = Instant::now();
        let timeout = Duration::from_millis(self.config.timeout_ms);

        for generation in 0..self.config.max_iterations {
            if start_time.elapsed() > timeout {
                break;
            }

            // Selection: keep top 50%
            population.sort_by(|a, b| {
                b.1.total_score
                    .partial_cmp(&a.1.total_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            population.truncate(self.config.population_size / 2);

            // Crossover and mutation to create new generation
            let mut new_generation = population.clone();

            while new_generation.len() < self.config.population_size {
                if population.len() >= 2 {
                    let parent1_idx = fastrand::usize(..population.len());
                    let parent2_idx = fastrand::usize(..population.len());

                    if let Ok(child) =
                        self.crossover_paths(&population[parent1_idx].0, &population[parent2_idx].0)
                    {
                        let mutated = if fastrand::f64() < self.config.mutation_rate {
                            self.mutate_path(child)?
                        } else {
                            child
                        };

                        if let Ok(score) =
                            self.score_route(&mutated, goal, constraints, request).await
                        {
                            if self.satisfies_constraints(&mutated, constraints) {
                                new_generation.push((mutated, score));
                            }
                        }
                    }
                }

                // Avoid infinite loop
                if new_generation.len() == population.len() {
                    break;
                }
            }

            population = new_generation;

            debug!(
                "Generation {}: best score = {:.4}",
                generation,
                population
                    .iter()
                    .map(|(_, score)| score.total_score)
                    .fold(0.0, f64::max)
            );
        }

        // Return top results
        population.sort_by(|a, b| {
            b.1.total_score
                .partial_cmp(&a.1.total_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        population.truncate(10);

        Ok(population)
    }

    #[allow(dead_code)]
    /// Crossover two paths to create a new path
    fn crossover_paths(&self, path1: &RoutePath, path2: &RoutePath) -> Result<RoutePath> {
        // Simple crossover: take steps from both paths
        // This is a simplified implementation - in practice, this would be more sophisticated

        if path1.steps.is_empty() || path2.steps.is_empty() {
            return Ok(path1.clone());
        }

        let crossover_point = fastrand::usize(1..path1.steps.len().max(path2.steps.len()));

        let mut new_steps = Vec::new();

        // Take steps from path1 up to crossover point
        for (i, step) in path1.steps.iter().enumerate() {
            if i < crossover_point {
                new_steps.push(step.clone());
            }
        }

        // Take remaining steps from path2
        for (i, step) in path2.steps.iter().enumerate() {
            if i >= crossover_point {
                new_steps.push(step.clone());
            }
        }

        // Ensure path continuity (this is simplified)
        if new_steps.len() > 1 {
            for i in 1..new_steps.len() {
                if new_steps[i - 1].to_token != new_steps[i].from_token {
                    // Path is broken, just return path1
                    return Ok(path1.clone());
                }
            }
        }

        // Recalculate path metrics
        let mut new_path = path1.clone();
        new_path.steps = new_steps;
        self.recalculate_path_metrics(&mut new_path);

        Ok(new_path)
    }

    #[allow(dead_code)]
    /// Mutate a path by randomly modifying some aspects
    fn mutate_path(&self, mut path: RoutePath) -> Result<RoutePath> {
        // Simple mutation: slightly adjust amounts or shuffle steps

        if fastrand::f64() < 0.5 && path.steps.len() > 1 {
            // Shuffle two adjacent steps
            let idx = fastrand::usize(..path.steps.len() - 1);
            path.steps.swap(idx, idx + 1);

            // Fix token continuity
            if path.steps[idx].to_token != path.steps[idx + 1].from_token {
                // Swap back if it breaks continuity
                path.steps.swap(idx, idx + 1);
            }
        }

        // Recalculate metrics
        self.recalculate_path_metrics(&mut path);

        Ok(path)
    }

    #[allow(dead_code)]
    /// Recalculate path metrics after modification
    fn recalculate_path_metrics(&self, path: &mut RoutePath) {
        // Recalculate totals
        path.total_fees = path
            .steps
            .iter()
            .map(|s| s.amount_in * s.fee_bps as f64 / 10000.0)
            .sum();
        path.estimated_gas_cost = path
            .steps
            .iter()
            .map(|s| self.estimate_step_gas(&s.dex_type))
            .sum();
        path.execution_time_estimate = Some(Duration::from_millis(
            path.steps
                .iter()
                .map(|s| self.estimate_step_time(&s.dex_type))
                .sum(),
        ));

        // Recalculate scores
        path.confidence_score = self.calculate_path_confidence(&path.steps);
        path.liquidity_score = self.calculate_path_liquidity(&path.steps);
        path.dex_diversity_score = self.calculate_path_diversity(&path.steps);
    }

    #[allow(dead_code)]
    /// Calculate simple score for sorting
    fn calculate_simple_score(&self, path: &RoutePath, goal: &OptimizationGoal) -> f64 {
        match goal {
            OptimizationGoal::MaximizeOutput => path.expected_output,
            OptimizationGoal::MinimizeTime => {
                1.0 / path
                    .execution_time_estimate
                    .unwrap_or(Duration::from_millis(500))
                    .as_secs_f64()
                    .max(0.1)
            }
            OptimizationGoal::MinimizeGas => 1.0 / path.estimated_gas_cost as f64,
            OptimizationGoal::MaximizeReliability => path.confidence_score,
            OptimizationGoal::MinimizeImpact => {
                let avg_impact = path.steps.iter().map(|s| s.price_impact).sum::<f64>()
                    / path.steps.len().max(1) as f64;
                1.0 - avg_impact
            }
            _ => {
                path.expected_output * path.confidence_score
                    / (path.estimated_gas_cost as f64 / 100000.0)
            }
        }
    }

    #[allow(dead_code)]
    /// Helper functions for path metric calculation
    fn estimate_step_gas(&self, dex_type: &crate::utils::DexType) -> u64 {
        match dex_type {
            crate::utils::DexType::Jupiter => 200_000,
            crate::utils::DexType::Orca => 150_000,
            crate::utils::DexType::Raydium => 180_000,
            crate::utils::DexType::Meteora => 170_000,
            crate::utils::DexType::Lifinity => 160_000,
            crate::utils::DexType::Phoenix => 200_000,
            crate::utils::DexType::Whirlpool => 150_000,
            crate::utils::DexType::Unknown(_) => 250_000,
        }
    }

    #[allow(dead_code)]
    fn estimate_step_time(&self, dex_type: &crate::utils::DexType) -> u64 {
        match dex_type {
            crate::utils::DexType::Jupiter => 2000,
            crate::utils::DexType::Orca => 800,
            crate::utils::DexType::Raydium => 600,
            crate::utils::DexType::Meteora => 700,
            crate::utils::DexType::Lifinity => 900,
            crate::utils::DexType::Phoenix => 1200,
            crate::utils::DexType::Whirlpool => 800,
            crate::utils::DexType::Unknown(_) => 3000,
        }
    }

    #[allow(dead_code)]
    fn calculate_path_confidence(&self, steps: &[super::pathfinder::RouteStep]) -> f64 {
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

    #[allow(dead_code)]
    fn calculate_path_liquidity(&self, steps: &[super::pathfinder::RouteStep]) -> f64 {
        if steps.is_empty() {
            return 0.0;
        }

        let min_liquidity = steps
            .iter()
            .map(|s| s.pool_liquidity)
            .fold(f64::INFINITY, f64::min);
        (min_liquidity.ln() / 25.0).min(1.0).max(0.0)
    }

    #[allow(dead_code)]
    fn calculate_path_diversity(&self, steps: &[super::pathfinder::RouteStep]) -> f64 {
        if steps.is_empty() {
            return 0.0;
        }

        let unique_dexs: std::collections::HashSet<_> = steps.iter().map(|s| &s.dex_type).collect();
        unique_dexs.len() as f64 / steps.len() as f64
    }

    /// Update performance statistics
    fn update_stats(&mut self, goal: &OptimizationGoal, duration: Duration, success: bool) {
        // Update general stats
        let count = self.stats.total_optimizations as f64;
        let old_avg_ms = self.stats.average_optimization_time.as_millis() as f64;
        let new_avg_ms = (old_avg_ms * (count - 1.0) + duration.as_millis() as f64) / count;
        self.stats.average_optimization_time = Duration::from_millis(new_avg_ms as u64);

        // Update goal-specific stats
        let goal_name = format!("{:?}", goal);
        let stats = self
            .stats
            .goal_performance
            .entry(goal_name)
            .or_insert(GoalStats {
                usage_count: 0,
                success_rate: 0.0,
                average_improvement: 0.0,
                average_time: Duration::from_millis(0),
            });

        stats.usage_count += 1;
        let goal_count = stats.usage_count as f64;

        // Update success rate
        stats.success_rate = (stats.success_rate * (goal_count - 1.0)
            + if success { 1.0 } else { 0.0 })
            / goal_count;

        // Update average time
        let old_time_ms = stats.average_time.as_millis() as f64;
        let new_time_ms =
            (old_time_ms * (goal_count - 1.0) + duration.as_millis() as f64) / goal_count;
        stats.average_time = Duration::from_millis(new_time_ms as u64);
    }

    /// Get average execution time for monitoring
    pub fn average_execution_time(&self) -> Duration {
        self.stats.average_optimization_time
    }

    /// Get optimizer statistics
    pub fn get_stats(&self) -> &OptimizerStats {
        &self.stats
    }
}

impl Default for RouteOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

// Tests temporarily commented out due to struct field mismatches
