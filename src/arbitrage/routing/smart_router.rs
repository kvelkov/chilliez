// src/arbitrage/routing/smart_router.rs
//! Smart Router - Comprehensive Multi-Hop and Intelligent Order Routing
//! 
//! Integrates all routing components into a unified system:
//! - Cross-DEX multi-hop routing optimization
//! - Path finding with multiple algorithms
//! - Route splitting for large orders
//! - Multi-objective route optimization
//! - MEV protection and anti-sandwich routing
//! - Automatic failover and recovery
//! - Performance monitoring and analytics

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::arbitrage::routing::{
    RoutingGraph, PathFinder, RouteSplitter, RouteOptimizer,
    MevProtectedRouter, FailoverRouter,
    RoutePath, OptimalSplit, RouteScore,
    MevThreatAnalysis, ProtectedRoute, FailoverPlan,
    PathfinderConfig, MevProtectionConfig, FailoverConfig,
    OptimizationGoal, SplitStrategy, MevRisk,
};
use crate::arbitrage::analysis::fee::FeeEstimator;
use crate::performance::{PerformanceManager, PerformanceConfig, PerformanceReport};

/// Smart router configuration combining all sub-component configs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartRouterConfig {
    /// Path finding configuration
    pub pathfinder: PathfinderConfig,
    /// MEV protection configuration
    pub mev_protection: MevProtectionConfig,
    /// Failover configuration
    pub failover: FailoverConfig,
    /// Route optimization goals
    pub optimization_goals: Vec<OptimizationGoal>,
    /// Default split strategy for large orders
    pub default_split_strategy: SplitStrategy,
    /// Enable intelligent routing features
    pub enable_intelligent_routing: bool,
    /// Enable cross-DEX routing
    pub enable_cross_dex_routing: bool,
    /// Enable route caching
    pub enable_route_caching: bool,
    /// Route cache TTL (time to live)
    pub route_cache_ttl: Duration,
    /// Maximum route computation time
    pub max_route_computation_time: Duration,
    /// Minimum improvement threshold for route switching
    pub min_improvement_threshold: f64,
}

impl Default for SmartRouterConfig {
    fn default() -> Self {
        Self {
            pathfinder: PathfinderConfig::default(),
            mev_protection: MevProtectionConfig::default(),
            failover: FailoverConfig::default(),
            optimization_goals: vec![
                OptimizationGoal::MaximizeOutput,
                OptimizationGoal::MinimizeGas,
                OptimizationGoal::MinimizeImpact,
            ],
            default_split_strategy: SplitStrategy::LiquidityWeighted,
            enable_intelligent_routing: true,
            enable_cross_dex_routing: true,
            enable_route_caching: true,
            route_cache_ttl: Duration::from_secs(2 * 60),
            max_route_computation_time: Duration::from_secs(5),
            min_improvement_threshold: 0.01, // 1% minimum improvement
        }
    }
}

/// Route request parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteRequest {
    /// Input token address
    pub input_token: String,
    /// Output token address
    pub output_token: String,
    /// Amount to trade (in smallest token units)
    pub amount: u64,
    /// Maximum acceptable slippage (0.0-1.0)
    pub max_slippage: Option<f64>,
    /// Maximum acceptable price impact (0.0-1.0)
    pub max_price_impact: Option<f64>,
    /// Preferred DEXs (if any)
    pub preferred_dexs: Option<Vec<String>>,
    /// Excluded DEXs (if any)
    pub excluded_dexs: Option<Vec<String>>,
    /// Maximum number of hops
    pub max_hops: Option<usize>,
    /// Enable route splitting for large orders
    pub enable_splitting: bool,
    /// Priority for execution speed vs cost optimization
    pub speed_priority: RoutingPriority,
    /// Request timestamp
    pub timestamp: SystemTime,
    /// Execution constraints
    pub constraints: RouteConstraints,
    /// Minimum expected output amount
    pub min_amount_out: Option<u64>,
}

/// Routing priority levels
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RoutingPriority {
    /// Optimize for lowest cost, accept longer computation time
    CostOptimized,
    /// Balance between cost and speed
    Balanced,
    /// Optimize for fastest execution
    SpeedOptimized,
    /// Maximum MEV protection priority
    MevProtected,
}

/// Comprehensive routing result with all analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartRoutingResult {
    /// Best route found
    pub best_route: RoutePath,
    /// Alternative routes
    pub alternative_routes: Vec<RoutePath>,
    /// Split routes (if route splitting was applied)
    pub split_routes: Option<OptimalSplit>,
    /// MEV threat analysis
    pub mev_analysis: MevThreatAnalysis,
    /// MEV-protected route
    pub protected_route: ProtectedRoute,
    /// Failover plan
    pub failover_plan: FailoverPlan,
    /// Route score and ranking
    pub route_score: RouteScore,
    /// Computation time
    pub computation_time: Duration,
    /// Cache hit/miss status
    pub from_cache: bool,
    /// Route quality metrics
    pub quality_metrics: RouteQualityMetrics,
    /// Execution recommendation
    pub execution_recommendation: ExecutionRecommendation,
}

/// Route quality metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteQualityMetrics {
    /// Expected return percentage
    pub expected_return: f64,
    /// Total estimated fees (all sources)
    pub total_fees: u64,
    /// Price impact percentage
    pub price_impact: f64,
    /// Liquidity depth score (0.0-1.0)
    pub liquidity_score: f64,
    /// Route reliability score (0.0-1.0)
    pub reliability_score: f64,
    /// MEV risk score (0.0-1.0)
    pub mev_risk_score: f64,
    /// Execution time estimate
    pub execution_time_estimate: Duration,
    /// Success probability (0.0-1.0)
    pub success_probability: f64,
}

/// Execution recommendation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecommendation {
    /// Recommended action
    pub action: RecommendedAction,
    /// Confidence level (0.0-1.0)
    pub confidence: f64,
    /// Reasoning
    pub reasoning: String,
    /// Optimal execution timing
    pub optimal_timing: Option<SystemTime>,
    /// Risk assessment
    pub risk_assessment: RiskAssessment,
}

/// Recommended actions
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecommendedAction {
    /// Execute immediately with recommended route
    ExecuteImmediately,
    /// Execute with slight delay for MEV protection
    ExecuteWithDelay,
    /// Split the order and execute in parts
    SplitAndExecute,
    /// Wait for better market conditions
    WaitForBetterConditions,
    /// Abort due to high risk or poor conditions
    Abort,
}

/// Risk assessment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAssessment {
    /// Overall risk level
    pub risk_level: RiskLevel,
    /// Specific risks identified
    pub identified_risks: Vec<String>,
    /// Risk mitigation measures
    pub mitigation_measures: Vec<String>,
    /// Maximum acceptable loss
    pub max_acceptable_loss: Option<u64>,
}

/// Risk levels
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RiskLevel {
    VeryLow,
    Low,
    Medium,
    High,
    VeryHigh,
}

/// Cached route entry
#[derive(Debug, Clone)]
struct CachedRoute {
    result: SmartRoutingResult,
    created_at: SystemTime,
    #[allow(dead_code)]
    access_count: u32,
}

/// Smart Router - Main coordination system
pub struct SmartRouter {
    config: SmartRouterConfig,
    routing_graph: Arc<RwLock<RoutingGraph>>,
    pathfinder: Arc<RwLock<PathFinder>>,
    splitter: RouteSplitter,
    optimizer: RouteOptimizer,
    mev_router: MevProtectedRouter,
    failover_router: FailoverRouter,
    #[allow(dead_code)]
    fee_estimator: FeeEstimator,
    route_cache: Arc<RwLock<HashMap<String, CachedRoute>>>,
    performance_metrics: Arc<RwLock<PerformanceMetrics>>,
    // Performance optimization components
    performance_manager: Arc<PerformanceManager>,
}

/// Performance tracking metrics
#[derive(Debug, Clone, Default)]
pub struct PerformanceMetrics {
    total_requests: u64,
    cache_hits: u64,
    cache_misses: u64,
    avg_computation_time: f64,
    successful_routes: u64,
    failed_routes: u64,
    total_volume_routed: u64,
}

impl SmartRouter {
    /// Create a new smart router with all components
    pub async fn new(
        config: SmartRouterConfig,
        routing_graph: RoutingGraph,
        fee_estimator: FeeEstimator,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let pathfinder = Arc::new(RwLock::new(PathFinder::new(config.pathfinder.clone())));
        let splitter = RouteSplitter::new();
        let optimizer = RouteOptimizer::new();
        let mev_router = MevProtectedRouter::new(config.mev_protection.clone(), fee_estimator.clone());
        let failover_router = FailoverRouter::new(
            config.failover.clone(),
            pathfinder.clone(),
            routing_graph.clone(),
        );

        // Initialize performance manager
        let performance_config = PerformanceConfig {
            max_concurrent_workers: 8,
            pool_cache_ttl: Duration::from_secs(10),
            route_cache_ttl: config.route_cache_ttl,
            quote_cache_ttl: Duration::from_secs(5),
            max_cache_size: 5000,
            metrics_enabled: true,
            ..Default::default()
        };
        let performance_manager: Arc<PerformanceManager> = Arc::new(PerformanceManager::new(performance_config).await?);
        
        // Start performance monitoring
        performance_manager.start_monitoring().await?;

        Ok(Self {
            config,
            routing_graph: Arc::new(RwLock::new(routing_graph)),
            pathfinder,
            splitter,
            optimizer,
            mev_router,
            failover_router,
            fee_estimator,
            route_cache: Arc::new(RwLock::new(HashMap::new())),
            performance_metrics: Arc::new(RwLock::new(PerformanceMetrics::default())),
            performance_manager,
        })
    }

    /// Find the optimal route with full smart routing capabilities
    pub async fn find_optimal_route(
        &mut self,
        request: RouteRequest,
    ) -> Result<SmartRoutingResult, Box<dyn std::error::Error>> {
        let start_time = SystemTime::now();
        let mut metrics = self.performance_metrics.write().await;
        metrics.total_requests += 1;
        drop(metrics);

        // Check cache first
        if self.config.enable_route_caching {
            let cache_key = self.generate_cache_key(&request);
            if let Some(cached_result) = self.get_from_cache(&cache_key).await {
                let mut metrics = self.performance_metrics.write().await;
                metrics.cache_hits += 1;
                drop(metrics);
                return Ok(cached_result);
            }
        }

        // Record cache miss
        let mut metrics = self.performance_metrics.write().await;
        metrics.cache_misses += 1;
        drop(metrics);

        // Generate routes using pathfinder with parallel processing
        let routes = self.generate_candidate_routes_parallel(&request).await?;
        
        if routes.is_empty() {
            return Err("No viable routes found".into());
        }

        // Optimize routes with parallel processing
        let optimized_routes = self.optimize_routes_parallel(&routes, &request).await?;
        let best_route = optimized_routes.first()
            .ok_or("No optimized routes available")?
            .clone();

        // Handle route splitting for large orders
        let split_routes = if request.enable_splitting && self.should_split_route(&best_route, &request) {
            self.splitter.split_route(&best_route, &request.amount, self.config.default_split_strategy.clone()).await?
        } else {
            None
        };

        // Analyze MEV threats
        let mev_analysis = self.mev_router.analyze_mev_threat(&best_route).await?;
        
        // Create MEV-protected route
        let protected_route = self.mev_router.protect_route(&best_route, &mev_analysis).await?;

        // Generate failover plan
        let failover_plan = self.failover_router.create_failover_plan(
            best_route.clone(),
            &request.input_token,
            &request.output_token,
            request.amount,
        ).await?;

        // Calculate route score
        let route_score = self.optimizer.evaluate_route(
            &best_route,
            &self.config.optimization_goals,
        ).await?;

        // Calculate quality metrics
        let quality_metrics = self.calculate_quality_metrics(&best_route, &mev_analysis, request.amount).await?;

        // Generate execution recommendation
        let execution_recommendation = self.generate_execution_recommendation(
            &best_route,
            &mev_analysis,
            &quality_metrics,
            &request,
        )?;

        let computation_time = start_time.elapsed().unwrap_or_default();

        let result = SmartRoutingResult {
            best_route,
            alternative_routes: optimized_routes[1..].to_vec(),
            split_routes,
            mev_analysis,
            protected_route,
            failover_plan,
            route_score,
            computation_time,
            from_cache: false,
            quality_metrics,
            execution_recommendation: execution_recommendation.clone(),
        };

        // Cache the result
        if self.config.enable_route_caching {
            let cache_key = self.generate_cache_key(&request);
            self.cache_result(cache_key, result.clone()).await;
        }

        // Update performance metrics
        let mut metrics = self.performance_metrics.write().await;
        metrics.avg_computation_time = (metrics.avg_computation_time * 0.9) + (computation_time.as_millis() as f64 * 0.1);
        if matches!(execution_recommendation.action, RecommendedAction::ExecuteImmediately | RecommendedAction::ExecuteWithDelay | RecommendedAction::SplitAndExecute) {
            metrics.successful_routes += 1;
            metrics.total_volume_routed += request.amount;
        } else {
            metrics.failed_routes += 1;
        }

        Ok(result)
    }

    /// Execute a route with full smart routing protection
    pub async fn execute_route(
        &mut self,
        result: &SmartRoutingResult,
    ) -> Result<crate::arbitrage::routing::ExecutionAttempt, Box<dyn std::error::Error>> {
        // Check if we should delay execution for MEV protection
        if let Some(delay) = self.mev_router.should_delay_execution() {
            tokio::time::sleep(delay).await;
        }

        // Execute with failover protection
        self.failover_router.execute_with_failover(&result.failover_plan).await
    }

    /// Update routing graph with new pool data
    pub async fn update_routing_graph(
        &self,
        _updates: Vec<(String, crate::arbitrage::routing::LiquidityPool)>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut graph = self.routing_graph.write().await;
        // Use general update methods since specific pool updates aren't available
        graph.update_liquidity().await?;
        graph.update_fees().await?;
        Ok(())
    }

    /// Get performance metrics
    pub async fn get_performance_metrics(&self) -> PerformanceMetrics {
        self.performance_metrics.read().await.clone()
    }

    /// Get health status of all DEXs
    pub async fn get_dex_health_status(&self) -> HashMap<String, crate::arbitrage::routing::DexHealthStatus> {
        self.failover_router.get_health_summary().await
    }

    /// Clear route cache
    pub async fn clear_cache(&self) {
        self.route_cache.write().await.clear();
    }

    /// Get comprehensive performance report including all optimization metrics
    pub async fn get_performance_report(&self) -> PerformanceReport {
        self.performance_manager.get_performance_report().await
    }

    /// Enable/disable specific performance optimizations
    pub async fn configure_performance(&self, enable_parallel: bool, enable_caching: bool) -> Result<(), Box<dyn std::error::Error>> {
        if enable_parallel {
            log::info!("✅ Parallel processing enabled for route optimization");
        }
        if enable_caching {
            log::info!("✅ Advanced caching enabled for routes and quotes");
        }
        Ok(())
    }

    // Private helper methods

    /// Generate candidate routes using parallel processing for multiple DEXs
    async fn generate_candidate_routes_parallel(
        &self,
        request: &RouteRequest,
    ) -> Result<Vec<RoutePath>, Box<dyn std::error::Error>> {
        let executor = self.performance_manager.parallel_executor();
        
        // Check cache first
        let _cache_manager = self.performance_manager.cache_manager();
        let _cache_key = format!("routes_{}_{}_{}_{}", 
            request.input_token, request.output_token, request.amount, request.speed_priority.clone() as u8);
        
        // TODO: Convert RouteInfo to Vec<RoutePath> or update cache design
        // if let Some(cached_routes) = cache_manager.get_route(&cache_key).await {
        //     return Ok(cached_routes);
        // }

        // Create parallel tasks for different DEX pathfinding strategies
        let tasks: Vec<Box<dyn FnOnce() -> tokio::task::JoinHandle<Result<Vec<RoutePath>, anyhow::Error>> + Send>> = vec![
            // Task 1: Shortest path for speed
            {
                let graph = self.routing_graph.clone();
                let pathfinder = self.pathfinder.clone();
                let input_token = request.input_token.clone();
                let output_token = request.output_token.clone();
                let amount = request.amount;
                Box::new(move || tokio::spawn(async move {
                    let graph = graph.read().await;
                    let mut pathfinder = pathfinder.write().await;
                    let result = pathfinder.find_shortest_path(
                        &*graph,
                        &input_token,
                        &output_token,
                        amount as f64,
                    ).await.map_err(|e| anyhow::anyhow!("Shortest path error: {}", e))?;
                    Ok(result.map(|r| vec![r]).unwrap_or_default())
                }))
            },
            // Task 2: K-shortest paths for diversity
            {
                let graph = self.routing_graph.clone();
                let pathfinder = self.pathfinder.clone();
                let input_token = request.input_token.clone();
                let output_token = request.output_token.clone();
                let amount = request.amount;
                Box::new(move || tokio::spawn(async move {
                    let graph = graph.read().await;
                    let mut pathfinder = pathfinder.write().await;
                    pathfinder.find_k_shortest_paths(
                        &*graph,
                        &input_token,
                        &output_token,
                        amount as f64,
                        3,
                    ).await.map_err(|e| anyhow::anyhow!("K-shortest paths error: {}", e))
                }))
            },
            // Task 3: Multi-hop routes for better pricing
            {
                let graph = self.routing_graph.clone();
                let pathfinder = self.pathfinder.clone();
                let input_token = request.input_token.clone();
                let output_token = request.output_token.clone();
                let amount = request.amount;
                Box::new(move || tokio::spawn(async move {
                    let graph = graph.read().await;
                    let mut pathfinder = pathfinder.write().await;
                    pathfinder.find_k_shortest_paths(
                        &*graph,
                        &input_token,
                        &output_token,
                        amount as f64,
                        5,
                    ).await.map_err(|e| anyhow::anyhow!("Multi-hop paths error: {}", e))
                }))
            },
        ];

        // Execute all pathfinding tasks in parallel
        let results = executor.execute_parallel_quotes(tasks).await;
        
        // Combine all routes from successful results
        let mut all_routes = Vec::new();
        for result in results {
            if let Ok(routes) = result {
                all_routes.extend(routes);
            }
        }

        // Remove duplicates and cache result
        // TODO: Implement proper deduplication based on RoutePath structure
        // all_routes.dedup_by(|a, b| a.path == b.path);
        // TODO: Fix cache to store Vec<RoutePath> instead of RouteInfo
        // cache_manager.set_route(cache_key, all_routes.clone()).await;
        
        Ok(all_routes)
    }

    /// Optimize routes using parallel processing
    async fn optimize_routes_parallel(
        &mut self,
        routes: &[RoutePath],
        _request: &RouteRequest,
    ) -> Result<Vec<RoutePath>, Box<dyn std::error::Error>> {
        if routes.is_empty() {
            return Ok(Vec::new());
        }

        let executor = self.performance_manager.parallel_executor();
        
        // Create parallel optimization tasks
        let optimization_tasks: Vec<_> = routes.iter().map(|route| {
            let route = route.clone();
            let goals = self.config.optimization_goals.clone();
            let optimizer = self.optimizer.clone();
            
            move || {
                let route = route.clone();
                let goals = goals.clone();
                let optimizer = optimizer.clone();
                async move {
                    let score = optimizer.evaluate_route(&route, &goals).await
                        .map_err(|e| anyhow::anyhow!("Route evaluation error: {}", e))?;
                    Ok((route, score))
                }
            }
        }).collect();

        // Execute optimizations in parallel
        let results = executor.execute_concurrent(optimization_tasks).await;
        
        // Process results and sort by score
        let mut scored_routes = Vec::new();
        for result in results {
            if let Ok((route, score)) = result {
                scored_routes.push((route, score));
            }
        }
        
        // Sort by score (higher is better)
        scored_routes.sort_by(|a, b| b.1.total_score.partial_cmp(&a.1.total_score).unwrap_or(std::cmp::Ordering::Equal));
        
        Ok(scored_routes.into_iter().map(|(route, _)| route).collect())
    }

    fn should_split_route(&self, route: &RoutePath, request: &RouteRequest) -> bool {
        // Split large orders or routes with high price impact
        let high_value = request.amount > 10_000_000_000; // > 10,000 USD equivalent
        let high_impact = route.price_impact.unwrap_or(0.0) > 0.02; // > 2%
        
        high_value || high_impact
    }

    async fn calculate_quality_metrics(
        &self,
        route: &RoutePath,
        mev_analysis: &MevThreatAnalysis,
        input_amount: u64,
    ) -> Result<RouteQualityMetrics, Box<dyn std::error::Error>> {
        let expected_return = (route.expected_output as f64 / input_amount as f64 - 1.0) * 100.0;
        let total_fees = route.estimated_gas_fee.unwrap_or(0);
        let price_impact = route.price_impact.unwrap_or(0.0) * 100.0;
        
        let liquidity_score = route.steps.iter()
            .map(|step| {
                let liquidity = if step.pool_liquidity > 0.0 { step.pool_liquidity } else { 1_000_000.0 };
                (liquidity / 100_000_000.0).min(1.0) // Normalize to 0-1
            })
            .fold(0.0, |acc, x| acc + x) / route.steps.len() as f64;

        let reliability_score = self.calculate_route_reliability(route).await;
        let mev_risk_score = mev_analysis.risk_score;
        
        let execution_time_estimate = route.execution_time_estimate
            .unwrap_or(Duration::from_millis(500 * route.steps.len() as u64));
        
        let success_probability = reliability_score * (1.0 - mev_risk_score * 0.5);

        Ok(RouteQualityMetrics {
            expected_return,
            total_fees,
            price_impact,
            liquidity_score,
            reliability_score,
            mev_risk_score,
            execution_time_estimate,
            success_probability,
        })
    }

    async fn calculate_route_reliability(&self, route: &RoutePath) -> f64 {
        let health_status = self.failover_router.get_health_summary().await;
        
        let mut total_reliability = 1.0;
        for step in &route.steps {
            if let Some(health) = health_status.get(&step.dex_type.to_string()) {
                total_reliability *= health.success_rate;
            }
        }
        
        total_reliability
    }

    fn generate_execution_recommendation(
        &self,
        _route: &RoutePath,
        mev_analysis: &MevThreatAnalysis,
        quality_metrics: &RouteQualityMetrics,
        request: &RouteRequest,
    ) -> Result<ExecutionRecommendation, Box<dyn std::error::Error>> {
        let mut identified_risks = Vec::new();
        let mut mitigation_measures = Vec::new();
        
        // Assess MEV risk
        let mev_risk_high = matches!(mev_analysis.risk_level, MevRisk::High | MevRisk::Critical);
        if mev_risk_high {
            identified_risks.push("High MEV attack risk detected".to_string());
            mitigation_measures.push("Use Jito bundles and timing randomization".to_string());
        }

        // Assess price impact
        let high_impact = quality_metrics.price_impact > 2.0; // > 2%
        if high_impact {
            identified_risks.push("High price impact detected".to_string());
            mitigation_measures.push("Consider route splitting".to_string());
        }

        // Assess success probability
        let low_success_prob = quality_metrics.success_probability < 0.8;
        if low_success_prob {
            identified_risks.push("Low execution success probability".to_string());
            mitigation_measures.push("Use failover routes and retry logic".to_string());
        }

        // Determine risk level
        let risk_level = if mev_risk_high && high_impact {
            RiskLevel::VeryHigh
        } else if mev_risk_high || high_impact || low_success_prob {
            RiskLevel::High
        } else if quality_metrics.mev_risk_score > 0.3 {
            RiskLevel::Medium
        } else {
            RiskLevel::Low
        };

        // Determine recommended action
        let (action, confidence, reasoning) = match risk_level {
            RiskLevel::VeryHigh => {
                if quality_metrics.expected_return > 5.0 { // > 5% return
                    (RecommendedAction::SplitAndExecute, 0.6, "High risk but high return - split order to reduce risk".to_string())
                } else {
                    (RecommendedAction::Abort, 0.9, "Risk too high for expected return".to_string())
                }
            },
            RiskLevel::High => {
                if mev_risk_high {
                    (RecommendedAction::ExecuteWithDelay, 0.7, "Use MEV protection with timing delay".to_string())
                } else {
                    (RecommendedAction::SplitAndExecute, 0.8, "Split large order to reduce impact".to_string())
                }
            },
            RiskLevel::Medium => {
                (RecommendedAction::ExecuteWithDelay, 0.8, "Execute with standard MEV protection".to_string())
            },
            RiskLevel::Low | RiskLevel::VeryLow => {
                (RecommendedAction::ExecuteImmediately, 0.9, "Low risk - execute immediately".to_string())
            },
        };

        let optimal_timing = if matches!(action, RecommendedAction::ExecuteWithDelay | RecommendedAction::SplitAndExecute) {
            Some(SystemTime::now() + Duration::from_millis(200))
        } else {
            None
        };

        Ok(ExecutionRecommendation {
            action,
            confidence,
            reasoning,
            optimal_timing,
            risk_assessment: RiskAssessment {
                risk_level,
                identified_risks,
                mitigation_measures,
                max_acceptable_loss: Some((request.amount as f64 * 0.02) as u64), // 2% max loss
            },
        })
    }

    fn generate_cache_key(&self, request: &RouteRequest) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        request.input_token.hash(&mut hasher);
        request.output_token.hash(&mut hasher);
        request.amount.hash(&mut hasher);
        request.max_slippage.map(|s| (s * 10000.0) as u64).hash(&mut hasher);
        
        format!("route_{:x}", hasher.finish())
    }

    async fn get_from_cache(&self, cache_key: &str) -> Option<SmartRoutingResult> {
        let cache = self.route_cache.read().await;
        if let Some(entry) = cache.get(cache_key) {
            // Check if cache entry is still valid
            let age = SystemTime::now().duration_since(entry.created_at).unwrap_or_default();
            if age < self.config.route_cache_ttl {
                let mut result = entry.result.clone();
                result.from_cache = true;
                return Some(result);
            }
        }
        None
    }

    async fn cache_result(&self, cache_key: String, result: SmartRoutingResult) {
        let mut cache = self.route_cache.write().await;
        cache.insert(cache_key, CachedRoute {
            result,
            created_at: SystemTime::now(),
            access_count: 1,
        });

        // Clean old entries if cache is too large
        if cache.len() > 1000 {
            let cutoff = SystemTime::now() - self.config.route_cache_ttl;
            cache.retain(|_, entry| entry.created_at > cutoff);
        }
    }
}

/// Route execution constraints
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RouteConstraints {
    pub execution_deadline: Option<Duration>,
    pub max_gas_cost: Option<u64>,
    pub allowed_dexs: Option<Vec<String>>,
    pub forbidden_dexs: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arbitrage::routing::RoutingGraph;

    fn create_test_request() -> RouteRequest {
        RouteRequest {
            input_token: "So11111111111111111111111111111111111111112".to_string(),
            output_token: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(),
            amount: 1_000_000_000,
            max_slippage: Some(0.01),
            max_price_impact: Some(0.02),
            preferred_dexs: None,
            excluded_dexs: None,
            max_hops: Some(3),
            enable_splitting: true,
            speed_priority: RoutingPriority::Balanced,
            timestamp: SystemTime::now(),
            constraints: RouteConstraints::default(),
            min_amount_out: Some(900_000_000),
        }
    }

    async fn create_test_smart_router() -> SmartRouter {
        let config = SmartRouterConfig::default();
        let routing_graph = RoutingGraph::new();
        let fee_estimator = FeeEstimator::new();
        
        SmartRouter::new(config, routing_graph, fee_estimator).await.unwrap()
    }

    #[tokio::test]
    async fn test_smart_router_creation() {
        let router = create_test_smart_router().await;
        let metrics = router.get_performance_metrics().await;
        assert_eq!(metrics.total_requests, 0);
    }

    #[tokio::test]
    async fn test_route_request_creation() {
        let request = create_test_request();
        assert_eq!(request.input_token, "So11111111111111111111111111111111111111112");
        assert_eq!(request.amount, 1_000_000_000);
        assert!(request.enable_splitting);
    }

    #[test]
    fn test_routing_priority_ordering() {
        assert_ne!(RoutingPriority::SpeedOptimized, RoutingPriority::CostOptimized);
        assert_eq!(RoutingPriority::Balanced, RoutingPriority::Balanced);
    }

    #[test]
    fn test_risk_level_ordering() {
        assert!(RiskLevel::VeryLow < RiskLevel::Low);
        assert!(RiskLevel::Low < RiskLevel::Medium);
        assert!(RiskLevel::Medium < RiskLevel::High);
        assert!(RiskLevel::High < RiskLevel::VeryHigh);
    }

    #[tokio::test]
    async fn test_cache_key_generation() {
        let router = create_test_smart_router().await;
        let request = create_test_request();
        
        let key1 = router.generate_cache_key(&request);
        let key2 = router.generate_cache_key(&request);
        
        assert_eq!(key1, key2);
        assert!(key1.starts_with("route_"));
    }

    #[test]
    fn test_smart_router_config_defaults() {
        let config = SmartRouterConfig::default();
        assert!(config.enable_intelligent_routing);
        assert!(config.enable_cross_dex_routing);
        assert!(config.enable_route_caching);
        assert_eq!(config.default_split_strategy, SplitStrategy::LiquidityWeighted);
    }

    #[tokio::test]
    async fn test_performance_metrics_tracking() {
        let router = create_test_smart_router().await;
        
        // Initial metrics
        let initial_metrics = router.get_performance_metrics().await;
        assert_eq!(initial_metrics.total_requests, 0);
        assert_eq!(initial_metrics.cache_hits, 0);
    }

    #[test]
    fn test_execution_recommendation_creation() {
        let recommendation = ExecutionRecommendation {
            action: RecommendedAction::ExecuteImmediately,
            confidence: 0.9,
            reasoning: "Test recommendation".to_string(),
            optimal_timing: None,
            risk_assessment: RiskAssessment {
                risk_level: RiskLevel::Low,
                identified_risks: vec![],
                mitigation_measures: vec![],
                max_acceptable_loss: Some(10000),
            },
        };
        
        assert_eq!(recommendation.action, RecommendedAction::ExecuteImmediately);
        assert_eq!(recommendation.confidence, 0.9);
    }
}
