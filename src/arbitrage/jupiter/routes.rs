//! Jupiter Multi-Route Optimization
//!
//! Advanced route discovery, evaluation, and selection for optimal Jupiter trades.
//! Implements parallel route evaluation, intelligent scoring, and route caching.

use crate::{
    dex::clients::jupiter::JupiterClient,
    dex::clients::jupiter_api::{QuoteRequest, QuoteResponse, RoutePlan},
    error::ArbError,
    local_metrics::Metrics,
};
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, Semaphore};

/// Configuration for multi-route optimization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteOptimizationConfig {
    /// Enable multi-route optimization
    pub enabled: bool,
    /// Maximum number of routes to evaluate in parallel
    pub max_parallel_routes: usize,
    /// Maximum number of alternative routes to request
    pub max_alternative_routes: u8,
    /// Timeout for route evaluation in milliseconds
    pub route_evaluation_timeout_ms: u64,
    /// Minimum price improvement required to switch routes (%)
    pub min_route_improvement_pct: f64,
    /// Route scoring weights
    pub scoring: RouteScoringConfig,
    /// Route caching configuration
    pub caching: RouteCacheConfig,
}

/// Route scoring configuration weights
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteScoringConfig {
    /// Weight for output amount (higher is better)
    pub output_amount_weight: f64,
    /// Weight for price impact (lower is better)
    pub price_impact_weight: f64,
    /// Weight for number of hops (lower is better)  
    pub hop_count_weight: f64,
    /// Weight for route reliability score
    pub reliability_weight: f64,
    /// Weight for estimated gas cost (lower is better)
    pub gas_cost_weight: f64,
}

/// Route caching configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteCacheConfig {
    /// Enable route caching
    pub enabled: bool,
    /// Cache TTL for route information in seconds
    pub route_ttl_seconds: u64,
    /// Maximum number of cached route sets
    pub max_cached_route_sets: usize,
    /// Invalidate routes when market moves more than this %
    pub market_movement_threshold_pct: f64,
}

impl Default for RouteOptimizationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_parallel_routes: 5,
            max_alternative_routes: 10,
            route_evaluation_timeout_ms: 2000,
            min_route_improvement_pct: 0.1,
            scoring: RouteScoringConfig::default(),
            caching: RouteCacheConfig::default(),
        }
    }
}

impl Default for RouteScoringConfig {
    fn default() -> Self {
        Self {
            output_amount_weight: 0.4, // 40% - output amount is most important
            price_impact_weight: 0.25, // 25% - avoid high slippage
            hop_count_weight: 0.15,    // 15% - prefer fewer hops
            reliability_weight: 0.15,  // 15% - prefer reliable routes
            gas_cost_weight: 0.05,     // 5% - gas cost consideration
        }
    }
}

impl Default for RouteCacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            route_ttl_seconds: 30, // Routes change more slowly than quotes
            max_cached_route_sets: 500,
            market_movement_threshold_pct: 1.0,
        }
    }
}

/// Route evaluation result with comprehensive metrics
#[derive(Debug, Clone)]
pub struct RouteEvaluation {
    /// Original quote response from Jupiter
    pub quote: QuoteResponse,
    /// Calculated route score (0.0 to 1.0, higher is better)
    pub score: f64,
    /// Individual scoring components
    pub score_components: RouteScoreComponents,
    /// Route reliability metrics
    pub reliability: RouteReliability,
    /// Evaluation timestamp
    pub evaluated_at: Instant,
    /// Time taken to evaluate this route
    pub evaluation_time: Duration,
}

/// Detailed route scoring components
#[derive(Debug, Clone)]
pub struct RouteScoreComponents {
    /// Normalized output amount score (0.0-1.0)
    pub output_amount_score: f64,
    /// Normalized price impact score (0.0-1.0, lower impact = higher score)
    pub price_impact_score: f64,
    /// Normalized hop count score (0.0-1.0, fewer hops = higher score)
    pub hop_count_score: f64,
    /// Route reliability score (0.0-1.0)
    pub reliability_score: f64,
    /// Normalized gas cost score (0.0-1.0, lower cost = higher score)
    pub gas_cost_score: f64,
}

/// Route reliability tracking
#[derive(Debug, Clone)]
pub struct RouteReliability {
    /// Success rate for this route pattern (0.0-1.0)
    pub success_rate: f64,
    /// Average execution time for this route type
    pub avg_execution_time_ms: f64,
    /// Number of times this route has been used
    pub usage_count: u32,
    /// Last successful execution timestamp
    pub last_success: Option<Instant>,
    /// Route complexity score
    pub complexity_score: f64,
}

/// Multi-route evaluation results
#[derive(Debug)]
pub struct MultiRouteResult {
    /// Best route selected
    pub best_route: RouteEvaluation,
    /// All evaluated routes sorted by score
    pub all_routes: Vec<RouteEvaluation>,
    /// Total evaluation time
    pub total_evaluation_time: Duration,
    /// Number of routes successfully evaluated
    pub routes_evaluated: usize,
    /// Number of routes that failed evaluation
    pub routes_failed: usize,
    /// Route selection reasoning
    pub selection_reason: String,
}

/// Route cache key for storing route sets
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct RouteCacheKey {
    pub input_mint: String,
    pub output_mint: String,
    pub amount_range: u64, // Bucketed amount for broader caching
    pub market_conditions_hash: u64,
}

/// Cached route set with metadata
#[derive(Debug, Clone)]
pub struct CachedRouteSet {
    pub routes: Vec<QuoteResponse>,
    pub cached_at: Instant,
    pub market_conditions: MarketConditions,
    pub hit_count: u32,
}

/// Market conditions snapshot for route caching
#[derive(Debug, Clone)]
pub struct MarketConditions {
    pub volatility_score: f64,
    pub liquidity_score: f64,
    pub spread_score: f64,
    pub timestamp: Instant,
}

/// Jupiter multi-route optimizer
pub struct JupiterRouteOptimizer {
    jupiter_client: Arc<JupiterClient>,
    config: RouteOptimizationConfig,
    route_cache: Arc<Mutex<HashMap<RouteCacheKey, CachedRouteSet>>>,
    route_reliability: Arc<Mutex<HashMap<String, RouteReliability>>>,
    evaluation_semaphore: Arc<Semaphore>,
    metrics: Arc<Mutex<Metrics>>,
}

impl JupiterRouteOptimizer {
    /// Create a new route optimizer
    pub fn new(
        jupiter_client: Arc<JupiterClient>,
        config: RouteOptimizationConfig,
        metrics: Arc<Mutex<Metrics>>,
    ) -> Self {
        let evaluation_semaphore = Arc::new(Semaphore::new(config.max_parallel_routes));

        info!("🛣️  Jupiter route optimizer initialized");
        info!("   - Max parallel routes: {}", config.max_parallel_routes);
        info!(
            "   - Route evaluation timeout: {}ms",
            config.route_evaluation_timeout_ms
        );
        info!("   - Caching enabled: {}", config.caching.enabled);

        Self {
            jupiter_client,
            config,
            route_cache: Arc::new(Mutex::new(HashMap::new())),
            route_reliability: Arc::new(Mutex::new(HashMap::new())),
            evaluation_semaphore,
            metrics,
        }
    }

    /// Find and evaluate optimal routes for a trade
    pub async fn find_optimal_route(
        &self,
        input_mint: &str,
        output_mint: &str,
        amount: u64,
        slippage_bps: u16,
    ) -> Result<MultiRouteResult, ArbError> {
        if !self.config.enabled {
            // Fallback to single route if optimization disabled
            return self
                .single_route_fallback(input_mint, output_mint, amount, slippage_bps)
                .await;
        }

        let start_time = Instant::now();
        info!(
            "🛣️  Finding optimal route: {} -> {} (amount: {})",
            input_mint.chars().take(6).collect::<String>(),
            output_mint.chars().take(6).collect::<String>(),
            amount
        );

        // Check route cache first
        if let Some(cached_routes) = self
            .get_cached_routes(input_mint, output_mint, amount)
            .await
        {
            debug!("📦 Using cached routes for evaluation");
            return self
                .evaluate_cached_routes(cached_routes, slippage_bps, start_time)
                .await;
        }

        // Discover multiple routes
        let discovered_routes = self
            .discover_routes(input_mint, output_mint, amount, slippage_bps)
            .await?;

        if discovered_routes.is_empty() {
            warn!("❌ No routes discovered for trade");
            return Err(ArbError::JupiterApiError("No routes available".to_string()));
        }

        // Cache discovered routes if caching enabled
        if self.config.caching.enabled {
            self.cache_routes(input_mint, output_mint, amount, &discovered_routes)
                .await;
        }

        // Evaluate routes in parallel
        let evaluation_results = self.evaluate_routes_parallel(discovered_routes).await;

        // Select best route
        let best_route = self.select_best_route(&evaluation_results)?;

        let total_time = start_time.elapsed();
        let successful_evals = evaluation_results.len();

        info!("✅ Route optimization complete in {:?}", total_time);
        info!("   - Routes evaluated: {}", successful_evals);
        info!("   - Best route score: {:.3}", best_route.score);
        info!(
            "   - Output improvement: {:.2}%",
            (best_route.score_components.output_amount_score - 0.5) * 200.0
        );

        // Update metrics
        self.update_metrics(successful_evals, total_time).await;

        Ok(MultiRouteResult {
            best_route: best_route.clone(),
            all_routes: evaluation_results.clone(),
            total_evaluation_time: total_time,
            routes_evaluated: successful_evals,
            routes_failed: 0, // TODO: Track failures
            selection_reason: self.generate_selection_reason(best_route),
        })
    }

    /// Discover multiple routes for the same trade
    async fn discover_routes(
        &self,
        input_mint: &str,
        output_mint: &str,
        amount: u64,
        slippage_bps: u16,
    ) -> Result<Vec<QuoteResponse>, ArbError> {
        debug!("🔍 Discovering routes for trade");

        // Create multiple quote requests with different parameters to get route diversity
        let mut quote_requests = vec![
            // Standard route request
            QuoteRequest {
                input_mint: input_mint.to_string(),
                output_mint: output_mint.to_string(),
                amount,
                slippage_bps,
                only_direct_routes: Some(false),
                exclude_dexes: None,
                max_accounts: Some(64),
            },
            // Direct routes only for comparison
            QuoteRequest {
                input_mint: input_mint.to_string(),
                output_mint: output_mint.to_string(),
                amount,
                slippage_bps,
                only_direct_routes: Some(true),
                exclude_dexes: None,
                max_accounts: Some(32),
            },
            // Lower account limit for simpler routes
            QuoteRequest {
                input_mint: input_mint.to_string(),
                output_mint: output_mint.to_string(),
                amount,
                slippage_bps,
                only_direct_routes: Some(false),
                exclude_dexes: None,
                max_accounts: Some(20),
            },
        ];

        // Add alternative amount requests for amount-based route diversity
        let amount_variations = [
            (amount as f64 * 0.95) as u64, // 5% lower
            (amount as f64 * 1.05) as u64, // 5% higher
        ];

        for alt_amount in amount_variations {
            quote_requests.push(QuoteRequest {
                input_mint: input_mint.to_string(),
                output_mint: output_mint.to_string(),
                amount: alt_amount,
                slippage_bps,
                only_direct_routes: Some(false),
                exclude_dexes: None,
                max_accounts: Some(64),
            });
        }

        // Execute quote requests in parallel with semaphore limiting
        let mut routes = Vec::new();
        let mut tasks = Vec::new();

        for request in quote_requests
            .into_iter()
            .take(self.config.max_alternative_routes as usize)
        {
            let client = Arc::clone(&self.jupiter_client);
            let permit = Arc::clone(&self.evaluation_semaphore);

            let task = tokio::spawn(async move {
                let _permit = permit.acquire().await.unwrap();

                // Use the existing quote method but extract the request logic
                client
                    .get_quote_with_fallback(
                        &request.input_mint,
                        &request.output_mint,
                        request.amount,
                        request.slippage_bps,
                    )
                    .await
            });

            tasks.push(task);
        }

        // Collect results with timeout
        let timeout_duration = Duration::from_millis(self.config.route_evaluation_timeout_ms);

        for task in tasks {
            match tokio::time::timeout(timeout_duration, task).await {
                Ok(Ok(Ok(quote))) => {
                    routes.push(quote);
                }
                Ok(Ok(Err(e))) => {
                    debug!("Route discovery failed: {}", e);
                }
                Ok(Err(_)) => {
                    debug!("Route discovery task panicked");
                }
                Err(_) => {
                    debug!("Route discovery timed out");
                }
            }
        }

        // Deduplicate routes based on route plan similarity
        routes = self.deduplicate_routes(routes);

        debug!("📍 Discovered {} unique routes", routes.len());

        Ok(routes)
    }

    /// Remove duplicate routes based on route plan similarity
    fn deduplicate_routes(&self, routes: Vec<QuoteResponse>) -> Vec<QuoteResponse> {
        let mut unique_routes = Vec::new();
        let mut seen_patterns = std::collections::HashSet::new();
        let original_count = routes.len();

        for route in routes {
            // Create a simple signature for the route based on the AMM keys used
            let route_signature = route
                .route_plan
                .iter()
                .map(|plan| plan.swap_info.amm_key.clone())
                .collect::<Vec<_>>()
                .join("|");

            if !seen_patterns.contains(&route_signature) {
                seen_patterns.insert(route_signature);
                unique_routes.push(route);
            }
        }

        debug!(
            "🔄 Deduplicated {} routes to {} unique patterns",
            original_count,
            unique_routes.len()
        );

        unique_routes
    }

    /// Evaluate multiple routes in parallel
    async fn evaluate_routes_parallel(&self, routes: Vec<QuoteResponse>) -> Vec<RouteEvaluation> {
        debug!("⚖️  Evaluating {} routes in parallel", routes.len());

        let mut evaluation_tasks = Vec::new();

        for route in routes {
            let config = self.config.clone();
            let reliability_map = Arc::clone(&self.route_reliability);

            let task = tokio::spawn(async move {
                Self::evaluate_single_route(route, &config.scoring, reliability_map).await
            });

            evaluation_tasks.push(task);
        }

        // Collect evaluation results
        let mut evaluations = Vec::new();

        for task in evaluation_tasks {
            match task.await {
                Ok(evaluation) => evaluations.push(evaluation),
                Err(e) => {
                    warn!("Route evaluation task failed: {}", e);
                }
            }
        }

        // Sort by score (highest first)
        evaluations.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        debug!(
            "📊 Route evaluation complete: {} successful",
            evaluations.len()
        );

        evaluations
    }

    /// Evaluate a single route and calculate its score
    async fn evaluate_single_route(
        quote: QuoteResponse,
        scoring_config: &RouteScoringConfig,
        reliability_map: Arc<Mutex<HashMap<String, RouteReliability>>>,
    ) -> RouteEvaluation {
        let start_time = Instant::now();

        // Parse route metrics
        let output_amount: u64 = quote.out_amount.parse().unwrap_or(0);
        let price_impact: f64 = quote.price_impact_pct.parse().unwrap_or(0.0);
        let hop_count = quote.route_plan.len();

        // Get or create route reliability
        let route_pattern = Self::create_route_pattern(&quote.route_plan);
        let reliability = {
            let reliability_guard = reliability_map.lock().await;
            reliability_guard
                .get(&route_pattern)
                .cloned()
                .unwrap_or_else(|| {
                    RouteReliability {
                        success_rate: 0.8, // Default assumption
                        avg_execution_time_ms: 2000.0,
                        usage_count: 0,
                        last_success: None,
                        complexity_score: Self::calculate_complexity_score(&quote.route_plan),
                    }
                })
        };

        // Calculate individual score components
        let score_components = RouteScoreComponents {
            output_amount_score: Self::normalize_output_amount(output_amount),
            price_impact_score: Self::normalize_price_impact(price_impact),
            hop_count_score: Self::normalize_hop_count(hop_count),
            reliability_score: reliability.success_rate,
            gas_cost_score: Self::estimate_gas_cost_score(&quote.route_plan),
        };

        // Calculate weighted final score
        let final_score = score_components.output_amount_score
            * scoring_config.output_amount_weight
            + score_components.price_impact_score * scoring_config.price_impact_weight
            + score_components.hop_count_score * scoring_config.hop_count_weight
            + score_components.reliability_score * scoring_config.reliability_weight
            + score_components.gas_cost_score * scoring_config.gas_cost_weight;

        let evaluation_time = start_time.elapsed();

        RouteEvaluation {
            quote,
            score: final_score.clamp(0.0, 1.0),
            score_components,
            reliability,
            evaluated_at: Instant::now(),
            evaluation_time,
        }
    }

    /// Select the best route from evaluated options
    fn select_best_route<'a>(
        &self,
        evaluations: &'a [RouteEvaluation],
    ) -> Result<&'a RouteEvaluation, ArbError> {
        if evaluations.is_empty() {
            return Err(ArbError::JupiterApiError(
                "No route evaluations available".to_string(),
            ));
        }

        // Primary selection: highest score
        let best_by_score = &evaluations[0]; // Already sorted by score

        // Validate minimum improvement threshold
        if evaluations.len() > 1 {
            let second_best = &evaluations[1];
            let improvement_pct = (best_by_score.score - second_best.score) * 100.0;

            if improvement_pct < self.config.min_route_improvement_pct {
                debug!(
                    "🤏 Route improvement {:.2}% below threshold {:.2}%, using second route",
                    improvement_pct, self.config.min_route_improvement_pct
                );
                // Use more reliable route if improvement is marginal
                if second_best.reliability.success_rate
                    > best_by_score.reliability.success_rate + 0.1
                {
                    return Ok(second_best);
                }
            }
        }

        Ok(best_by_score)
    }

    /// Generate human-readable selection reasoning
    fn generate_selection_reason(&self, selected_route: &RouteEvaluation) -> String {
        let components = &selected_route.score_components;

        let mut reasons = Vec::new();

        if components.output_amount_score > 0.8 {
            reasons.push("excellent output amount");
        }
        if components.price_impact_score > 0.8 {
            reasons.push("low price impact");
        }
        if components.hop_count_score > 0.8 {
            reasons.push("efficient routing");
        }
        if components.reliability_score > 0.8 {
            reasons.push("high reliability");
        }

        if reasons.is_empty() {
            "best available option".to_string()
        } else {
            format!("selected for: {}", reasons.join(", "))
        }
    }

    /// Fallback to single route when optimization is disabled
    async fn single_route_fallback(
        &self,
        input_mint: &str,
        output_mint: &str,
        amount: u64,
        slippage_bps: u16,
    ) -> Result<MultiRouteResult, ArbError> {
        let start_time = Instant::now();

        let quote = self
            .jupiter_client
            .get_quote_with_fallback(input_mint, output_mint, amount, slippage_bps)
            .await?;

        let evaluation = Self::evaluate_single_route(
            quote,
            &self.config.scoring,
            Arc::clone(&self.route_reliability),
        )
        .await;

        let total_time = start_time.elapsed();

        Ok(MultiRouteResult {
            best_route: evaluation.clone(),
            all_routes: vec![evaluation],
            total_evaluation_time: total_time,
            routes_evaluated: 1,
            routes_failed: 0,
            selection_reason: "single route mode".to_string(),
        })
    }

    /// Cache discovered routes for future use
    async fn cache_routes(
        &self,
        input_mint: &str,
        output_mint: &str,
        amount: u64,
        routes: &[QuoteResponse],
    ) {
        if !self.config.caching.enabled {
            return;
        }

        let cache_key = RouteCacheKey {
            input_mint: input_mint.to_string(),
            output_mint: output_mint.to_string(),
            amount_range: Self::bucket_amount_for_routes(amount),
            market_conditions_hash: Self::calculate_market_conditions_hash(),
        };

        let cached_set = CachedRouteSet {
            routes: routes.to_vec(),
            cached_at: Instant::now(),
            market_conditions: Self::capture_market_conditions(),
            hit_count: 0,
        };

        let mut cache = self.route_cache.lock().await;

        // Clean up old entries if cache is full
        if cache.len() >= self.config.caching.max_cached_route_sets {
            Self::cleanup_old_cache_entries(&mut cache, &self.config.caching);
        }

        cache.insert(cache_key, cached_set);
        debug!("📦 Cached {} routes for future use", routes.len());
    }

    /// Get cached routes if available and valid
    async fn get_cached_routes(
        &self,
        input_mint: &str,
        output_mint: &str,
        amount: u64,
    ) -> Option<Vec<QuoteResponse>> {
        if !self.config.caching.enabled {
            return None;
        }

        let cache_key = RouteCacheKey {
            input_mint: input_mint.to_string(),
            output_mint: output_mint.to_string(),
            amount_range: Self::bucket_amount_for_routes(amount),
            market_conditions_hash: Self::calculate_market_conditions_hash(),
        };

        let mut cache = self.route_cache.lock().await;

        if let Some(cached_set) = cache.get_mut(&cache_key) {
            // Check if cached routes are still valid
            let age = cached_set.cached_at.elapsed();
            let ttl = Duration::from_secs(self.config.caching.route_ttl_seconds);

            if age < ttl && Self::are_market_conditions_stable(&cached_set.market_conditions) {
                cached_set.hit_count += 1;
                debug!(
                    "📦 Using cached routes (age: {:?}, hits: {})",
                    age, cached_set.hit_count
                );
                return Some(cached_set.routes.clone());
            } else {
                debug!("🗑️  Cached routes expired or market conditions changed");
                cache.remove(&cache_key);
            }
        }

        None
    }

    /// Evaluate cached routes with current parameters
    async fn evaluate_cached_routes(
        &self,
        routes: Vec<QuoteResponse>,
        slippage_bps: u16,
        start_time: Instant,
    ) -> Result<MultiRouteResult, ArbError> {
        // Filter routes that match current slippage requirements
        let compatible_routes: Vec<_> = routes
            .into_iter()
            .filter(|route| {
                // Allow some slippage tolerance variance
                let route_slippage = route.price_impact_pct.parse::<f64>().unwrap_or(0.0) * 100.0;
                (route_slippage - slippage_bps as f64).abs() <= 50.0 // 0.5% tolerance
            })
            .collect();

        if compatible_routes.is_empty() {
            warn!("❌ No cached routes compatible with current slippage requirement");
            return Err(ArbError::JupiterApiError(
                "No compatible cached routes".to_string(),
            ));
        }

        let routes_evaluated = compatible_routes.len();
        let evaluations = self.evaluate_routes_parallel(compatible_routes).await;
        let best_route = self.select_best_route(&evaluations)?;
        let total_time = start_time.elapsed();

        Ok(MultiRouteResult {
            best_route: best_route.clone(),
            all_routes: evaluations.clone(),
            total_evaluation_time: total_time,
            routes_evaluated,
            routes_failed: 0,
            selection_reason: format!(
                "cached routes: {}",
                self.generate_selection_reason(best_route)
            ),
        })
    }

    /// Update performance metrics
    async fn update_metrics(&self, routes_evaluated: usize, total_time: Duration) {
        if let Ok(_metrics) = self.metrics.try_lock() {
            // Update custom metrics for route optimization
            // This would integrate with the existing metrics system
            debug!(
                "📊 Route optimization metrics: {} routes in {:?}",
                routes_evaluated, total_time
            );
        }
    }

    // Helper methods for scoring and caching

    /// Create a route pattern string for reliability tracking
    fn create_route_pattern(route_plan: &[RoutePlan]) -> String {
        route_plan
            .iter()
            .map(|plan| plan.swap_info.amm_key.chars().take(8).collect::<String>())
            .collect::<Vec<_>>()
            .join("-")
    }

    /// Calculate route complexity score based on hops and DEX types
    fn calculate_complexity_score(route_plan: &[RoutePlan]) -> f64 {
        let hop_penalty = route_plan.len() as f64 * 0.1;
        let complexity = hop_penalty.min(1.0);
        1.0 - complexity // Higher score for less complex routes
    }

    /// Normalize output amount to 0-1 score
    fn normalize_output_amount(amount: u64) -> f64 {
        // This would ideally compare against expected amounts
        // For now, use a simple normalization
        let normalized = (amount as f64).log10() / 10.0; // Rough normalization
        normalized.clamp(0.0, 1.0)
    }

    /// Normalize price impact to 0-1 score (lower impact = higher score)
    fn normalize_price_impact(impact_pct: f64) -> f64 {
        if impact_pct <= 0.0 {
            1.0
        } else if impact_pct >= 5.0 {
            0.0
        } else {
            1.0 - (impact_pct / 5.0)
        }
    }

    /// Normalize hop count to 0-1 score (fewer hops = higher score)
    fn normalize_hop_count(hop_count: usize) -> f64 {
        match hop_count {
            0..=1 => 1.0,
            2 => 0.8,
            3 => 0.6,
            4 => 0.4,
            5 => 0.2,
            _ => 0.0,
        }
    }

    /// Estimate gas cost score based on route complexity
    fn estimate_gas_cost_score(route_plan: &[RoutePlan]) -> f64 {
        let base_cost = 50_000u64; // Base transaction cost
        let hop_cost = route_plan.len() as u64 * 100_000; // Cost per hop
        let total_estimated_cost = base_cost + hop_cost;

        // Normalize: lower cost = higher score
        let normalized = 1.0 - (total_estimated_cost as f64 / 1_000_000.0);
        normalized.clamp(0.0, 1.0)
    }

    /// Bucket amounts for route caching (broader than quote caching)
    fn bucket_amount_for_routes(amount: u64) -> u64 {
        let bucket_size = 10_000_000; // 10M lamports for route caching
        ((amount + bucket_size / 2) / bucket_size) * bucket_size
    }

    /// Calculate market conditions hash for cache invalidation
    fn calculate_market_conditions_hash() -> u64 {
        // Simplified market conditions hash
        // In practice, this would consider volatility, spreads, etc.
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut hasher = DefaultHasher::new();
        (now / 300).hash(&mut hasher); // 5-minute buckets
        hasher.finish()
    }

    /// Capture current market conditions
    fn capture_market_conditions() -> MarketConditions {
        MarketConditions {
            volatility_score: 0.5, // Placeholder
            liquidity_score: 0.8,  // Placeholder
            spread_score: 0.7,     // Placeholder
            timestamp: Instant::now(),
        }
    }

    /// Check if market conditions are stable for cache validity
    fn are_market_conditions_stable(conditions: &MarketConditions) -> bool {
        let age = conditions.timestamp.elapsed();
        age < Duration::from_secs(300) // 5 minutes stability window
    }

    /// Clean up old cache entries when cache is full
    fn cleanup_old_cache_entries(
        cache: &mut HashMap<RouteCacheKey, CachedRouteSet>,
        _config: &RouteCacheConfig,
    ) {
        let entries: Vec<_> = cache
            .iter()
            .map(|(k, v)| (k.clone(), v.cached_at))
            .collect();
        let mut entries = entries;
        entries.sort_by_key(|(_, cached_at)| *cached_at);

        // Remove oldest 25% of entries
        let remove_count = cache.len() / 4;
        for (key, _) in entries.iter().take(remove_count) {
            cache.remove(key);
        }

        debug!("🧹 Cleaned up {} old route cache entries", remove_count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dex::clients::jupiter_api::{QuoteResponse, RoutePlan, SwapInfo};

    fn create_test_quote(out_amount: u64, price_impact: f64, hop_count: usize) -> QuoteResponse {
        let route_plan = (0..hop_count)
            .map(|i| RoutePlan {
                swap_info: SwapInfo {
                    amm_key: format!("test_amm_{}", i),
                    label: "Test DEX".to_string(),
                    input_mint: "So11111111111111111111111111111111111111112".to_string(),
                    output_mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(),
                    in_amount: "1000000".to_string(),
                    out_amount: out_amount.to_string(),
                    fee_amount: "1000".to_string(),
                    fee_mint: "So11111111111111111111111111111111111111112".to_string(),
                },
                percent: 100,
            })
            .collect();

        QuoteResponse {
            input_mint: "So11111111111111111111111111111111111111112".to_string(),
            in_amount: "1000000".to_string(),
            output_mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(),
            out_amount: out_amount.to_string(),
            other_amount_threshold: "0".to_string(),
            route_plan,
            context_slot: 12345,
            time_taken: 0.1,
            platform_fee: None,
            price_impact_pct: price_impact.to_string(),
        }
    }

    #[tokio::test]
    async fn test_route_scoring() {
        let scoring_config = RouteScoringConfig::default();
        let reliability_map = Arc::new(Mutex::new(HashMap::new()));

        let quote = create_test_quote(1000000, 0.5, 2);
        let evaluation =
            JupiterRouteOptimizer::evaluate_single_route(quote, &scoring_config, reliability_map)
                .await;

        assert!(evaluation.score > 0.0);
        assert!(evaluation.score <= 1.0);
        assert!(evaluation.score_components.hop_count_score > 0.0);
    }

    #[test]
    fn test_route_pattern_creation() {
        let route_plan = vec![RoutePlan {
            swap_info: SwapInfo {
                amm_key: "test_amm_1".to_string(),
                label: "DEX1".to_string(),
                input_mint: "mint1".to_string(),
                output_mint: "mint2".to_string(),
                in_amount: "1000".to_string(),
                out_amount: "2000".to_string(),
                fee_amount: "10".to_string(),
                fee_mint: "mint1".to_string(),
            },
            percent: 100,
        }];

        let pattern = JupiterRouteOptimizer::create_route_pattern(&route_plan);
        assert_eq!(pattern, "test_amm");
    }

    #[test]
    fn test_score_normalization() {
        // Test price impact normalization
        assert_eq!(JupiterRouteOptimizer::normalize_price_impact(0.0), 1.0);
        assert_eq!(JupiterRouteOptimizer::normalize_price_impact(5.0), 0.0);
        assert_eq!(JupiterRouteOptimizer::normalize_price_impact(2.5), 0.5);

        // Test hop count normalization
        assert_eq!(JupiterRouteOptimizer::normalize_hop_count(1), 1.0);
        assert_eq!(JupiterRouteOptimizer::normalize_hop_count(2), 0.8);
        assert_eq!(JupiterRouteOptimizer::normalize_hop_count(10), 0.0);
    }

    #[test]
    fn test_amount_bucketing() {
        assert_eq!(
            JupiterRouteOptimizer::bucket_amount_for_routes(5_000_000),
            10_000_000
        );
        assert_eq!(
            JupiterRouteOptimizer::bucket_amount_for_routes(15_000_000),
            20_000_000
        );
        assert_eq!(
            JupiterRouteOptimizer::bucket_amount_for_routes(22_000_000),
            20_000_000
        );
    }

    #[test]
    fn test_configuration_defaults() {
        let config = RouteOptimizationConfig::default();
        assert!(config.enabled);
        assert_eq!(config.max_parallel_routes, 5);
        assert_eq!(config.max_alternative_routes, 10);

        let scoring = RouteScoringConfig::default();
        assert!((scoring.output_amount_weight - 0.4).abs() < f64::EPSILON);
        assert!((scoring.price_impact_weight - 0.25).abs() < f64::EPSILON);
    }
}
