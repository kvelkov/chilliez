//! Jupiter Integration Manager
//!
//! Manages Jupiter fallback functionality with intelligent caching and monitoring.
//! Provides a high-level interface for Jupiter operations with the arbitrage system.

use crate::{
    arbitrage::jupiter::cache::{CacheConfig, CacheKey, JupiterQuoteCache},
    arbitrage::jupiter::routes::{
        JupiterRouteOptimizer, MultiRouteResult, RouteOptimizationConfig,
    },
    config::settings::Config,
    dex::clients::jupiter::JupiterClient,
    dex::clients::jupiter_api::QuoteResponse,
    error::ArbError,
    local_metrics::Metrics,
};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Configuration for Jupiter integration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JupiterIntegrationConfig {
    /// Enable Jupiter fallback functionality
    pub fallback_enabled: bool,
    /// Minimum profit percentage to consider Jupiter opportunities
    pub min_profit_pct: f64,
    /// Maximum slippage tolerance in basis points
    pub max_slippage_bps: u16,
    /// Cache configuration
    pub cache: CacheConfig,
    /// Route optimization configuration
    pub route_optimization: RouteOptimizationConfig,
    /// Performance monitoring settings
    pub monitoring: MonitoringConfig,
}

/// Performance monitoring configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringConfig {
    /// Log cache statistics every N requests
    pub log_stats_every_n_requests: u64,
    /// Warn if cache hit rate falls below this threshold
    pub min_acceptable_hit_rate: f64,
    /// Alert if API response time exceeds this (ms)
    pub max_acceptable_latency_ms: u64,
}

impl Default for MonitoringConfig {
    fn default() -> Self {
        Self {
            log_stats_every_n_requests: 100,
            min_acceptable_hit_rate: 0.6,
            max_acceptable_latency_ms: 1000,
        }
    }
}

impl Default for JupiterIntegrationConfig {
    fn default() -> Self {
        Self {
            fallback_enabled: true,
            min_profit_pct: 0.1,
            max_slippage_bps: 100,
            cache: CacheConfig::default(),
            route_optimization: RouteOptimizationConfig::default(),
            monitoring: MonitoringConfig {
                log_stats_every_n_requests: 100,
                min_acceptable_hit_rate: 0.6,
                max_acceptable_latency_ms: 1000,
            },
        }
    }
}

impl From<&Config> for JupiterIntegrationConfig {
    fn from(config: &Config) -> Self {
        Self {
            fallback_enabled: config.jupiter_fallback_enabled,
            min_profit_pct: config.jupiter_fallback_min_profit_pct,
            max_slippage_bps: config.jupiter_slippage_tolerance_bps,
            cache: CacheConfig {
                enabled: config.jupiter_cache_enabled,
                ttl_seconds: config.jupiter_cache_ttl_seconds,
                max_entries: config.jupiter_cache_max_entries,
                amount_bucket_size: config.jupiter_cache_amount_bucket_size,
                volatility_threshold_pct: config.jupiter_cache_volatility_threshold_pct,
                target_hit_rate: 0.7, // Fixed target for now
            },
            route_optimization: RouteOptimizationConfig {
                enabled: config.jupiter_route_optimization_enabled,
                max_parallel_routes: config.jupiter_max_parallel_routes,
                max_alternative_routes: config.jupiter_max_alternative_routes,
                route_evaluation_timeout_ms: config.jupiter_route_evaluation_timeout_ms,
                min_route_improvement_pct: config.jupiter_min_route_improvement_pct,
                ..RouteOptimizationConfig::default()
            },
            monitoring: MonitoringConfig::default(),
        }
    }
}

/// Jupiter fallback manager with caching, route optimization, and monitoring
pub struct JupiterFallbackManager {
    jupiter_client: Arc<JupiterClient>,
    quote_cache: JupiterQuoteCache,
    route_optimizer: Option<JupiterRouteOptimizer>,
    config: JupiterIntegrationConfig,
    metrics: Arc<Mutex<Metrics>>,
    request_counter: Arc<Mutex<u64>>,
}

impl JupiterFallbackManager {
    /// Create a new Jupiter fallback manager
    pub fn new(
        jupiter_client: Arc<JupiterClient>,
        config: JupiterIntegrationConfig,
        metrics: Arc<Mutex<Metrics>>,
    ) -> Self {
        info!("🪐 Initializing Jupiter fallback manager with caching");

        let quote_cache = JupiterQuoteCache::new(config.cache.clone());
        let route_optimizer = if config.route_optimization.enabled {
            Some(JupiterRouteOptimizer::new(
                Arc::clone(&jupiter_client),
                config.route_optimization.clone(),
                Arc::clone(&metrics),
            ))
        } else {
            None
        };

        Self {
            jupiter_client,
            quote_cache,
            route_optimizer,
            config,
            metrics,
            request_counter: Arc::new(Mutex::new(0)),
        }
    }

    /// Get a quote with intelligent caching
    pub async fn get_quote_with_cache(
        &self,
        input_mint: &str,
        output_mint: &str,
        amount: u64,
        slippage_bps: Option<u16>,
    ) -> Result<QuoteResponse, ArbError> {
        if !self.config.fallback_enabled {
            return Err(ArbError::DexError(
                "Jupiter fallback is disabled".to_string(),
            ));
        }

        let start_time = std::time::Instant::now();
        let slippage = slippage_bps.unwrap_or(self.config.max_slippage_bps);

        // Create cache key
        let cache_key = CacheKey::from_params(
            input_mint,
            output_mint,
            amount,
            slippage,
            self.config.cache.amount_bucket_size,
        );

        // Increment request counter and check for stats logging
        {
            let mut counter = self.request_counter.lock().await;
            *counter += 1;
            if *counter % self.config.monitoring.log_stats_every_n_requests == 0 {
                self.log_performance_stats().await;
            }
        }

        // Try cache first
        if let Some(cached_quote) = self.quote_cache.get_quote(&cache_key).await {
            let latency = start_time.elapsed().as_millis() as u64;
            debug!(
                "🎯 Jupiter cache hit: {}→{} ({}ms)",
                input_mint.chars().take(6).collect::<String>(),
                output_mint.chars().take(6).collect::<String>(),
                latency
            );

            self.record_metrics(true, latency).await;
            return Ok(cached_quote);
        }

        // Cache miss - check for volatility before making API call
        if self.quote_cache.should_invalidate_for_volatility().await {
            self.quote_cache
                .clear_volatile_cache("High market volatility detected")
                .await;
        }

        // Make Jupiter API call
        debug!(
            "📡 Jupiter cache miss - making API call for {}→{}",
            input_mint.chars().take(6).collect::<String>(),
            output_mint.chars().take(6).collect::<String>()
        );

        let api_start = std::time::Instant::now();
        let quote_result = self
            .jupiter_client
            .get_quote_with_fallback(input_mint, output_mint, amount, slippage)
            .await;

        let api_latency = api_start.elapsed().as_millis() as u64;
        let total_latency = start_time.elapsed().as_millis() as u64;

        match quote_result {
            Ok(quote) => {
                // Store in cache for future use
                self.quote_cache.store_quote(cache_key, quote.clone()).await;

                debug!(
                    "✅ Jupiter API success: {}→{} (API: {}ms, Total: {}ms)",
                    input_mint.chars().take(6).collect::<String>(),
                    output_mint.chars().take(6).collect::<String>(),
                    api_latency,
                    total_latency
                );

                // Check for performance warnings
                if api_latency > self.config.monitoring.max_acceptable_latency_ms {
                    warn!(
                        "⚠️ Jupiter API slow response: {}ms (threshold: {}ms)",
                        api_latency, self.config.monitoring.max_acceptable_latency_ms
                    );
                }

                self.record_metrics(false, total_latency).await;
                Ok(quote)
            }
            Err(e) => {
                error!("❌ Jupiter API failed: {}", e);
                self.record_metrics(false, total_latency).await;
                Err(ArbError::NetworkError(format!("Jupiter API error: {}", e)))
            }
        }
    }

    /// Get the best route using multi-route optimization
    pub async fn get_optimal_route(
        &self,
        input_mint: &str,
        output_mint: &str,
        amount: u64,
        slippage_bps: Option<u16>,
    ) -> Result<MultiRouteResult, ArbError> {
        if !self.config.fallback_enabled {
            return Err(ArbError::JupiterApiError(
                "Jupiter fallback disabled".to_string(),
            ));
        }

        let slippage = slippage_bps.unwrap_or(self.config.max_slippage_bps);

        // Increment request counter for monitoring
        {
            let mut counter = self.request_counter.lock().await;
            *counter += 1;

            // Log statistics periodically
            if *counter % self.config.monitoring.log_stats_every_n_requests == 0 {
                self.log_performance_stats().await;
            }
        }

        // Use route optimizer if available
        if let Some(ref optimizer) = self.route_optimizer {
            debug!("🛣️  Using route optimization for quote request");
            optimizer
                .find_optimal_route(input_mint, output_mint, amount, slippage)
                .await
        } else {
            // Fallback to single route via existing cache method
            debug!("📡 Using single route fallback (optimization disabled)");
            let quote = self
                .get_quote_with_cache(input_mint, output_mint, amount, Some(slippage))
                .await?;

            // Convert single quote to MultiRouteResult format
            Ok(MultiRouteResult {
                best_route: self.create_route_evaluation_from_quote(quote).await,
                all_routes: vec![],
                total_evaluation_time: std::time::Duration::from_millis(0),
                routes_evaluated: 1,
                routes_failed: 0,
                selection_reason: "single route mode".to_string(),
            })
        }
    }

    /// Check if route optimization is enabled and available
    pub fn is_route_optimization_enabled(&self) -> bool {
        self.route_optimizer.is_some() && self.config.route_optimization.enabled
    }

    /// Get route optimization statistics
    pub async fn get_route_optimization_stats(&self) -> Option<String> {
        if let Some(ref _optimizer) = self.route_optimizer {
            // In a real implementation, this would return comprehensive route stats
            Some("Route optimization active".to_string())
        } else {
            None
        }
    }

    /// Create a route evaluation from a single quote (for compatibility)
    async fn create_route_evaluation_from_quote(
        &self,
        quote: QuoteResponse,
    ) -> crate::arbitrage::jupiter::routes::RouteEvaluation {
        use crate::arbitrage::jupiter::routes::{
            RouteEvaluation, RouteReliability, RouteScoreComponents,
        };

        RouteEvaluation {
            score: 0.8, // Default score for single route
            score_components: RouteScoreComponents {
                output_amount_score: 0.8,
                price_impact_score: 0.8,
                hop_count_score: 0.8,
                reliability_score: 0.8,
                gas_cost_score: 0.8,
            },
            reliability: RouteReliability {
                success_rate: 0.8,
                avg_execution_time_ms: 2000.0,
                usage_count: 1,
                last_success: None,
                complexity_score: 0.5,
            },
            evaluated_at: std::time::Instant::now(),
            evaluation_time: std::time::Duration::from_millis(100),
            quote,
        }
    }

    /// Check if Jupiter quote meets minimum profit requirements
    pub fn meets_profit_threshold(&self, quote: &QuoteResponse, input_amount: u64) -> bool {
        if let Ok(output_amount) = quote.out_amount.parse::<u64>() {
            let profit_ratio = output_amount as f64 / input_amount as f64;
            let profit_pct = (profit_ratio - 1.0) * 100.0;

            if profit_pct >= self.config.min_profit_pct {
                debug!(
                    "💰 Jupiter quote meets profit threshold: {:.2}% (min: {:.2}%)",
                    profit_pct, self.config.min_profit_pct
                );
                return true;
            } else {
                debug!(
                    "📉 Jupiter quote below profit threshold: {:.2}% (min: {:.2}%)",
                    profit_pct, self.config.min_profit_pct
                );
            }
        }
        false
    }

    /// Get comprehensive cache statistics
    pub async fn get_cache_statistics(&self) -> CacheStatistics {
        let cache_stats = self.quote_cache.get_cache_stats().await;
        let cache_metrics = self.quote_cache.get_metrics().await;

        CacheStatistics {
            hit_rate: cache_stats.hit_rate,
            total_requests: cache_stats.total_requests,
            cache_hits: cache_metrics.hits,
            cache_misses: cache_metrics.misses,
            cache_size: cache_stats.cache_size,
            max_cache_size: cache_stats.max_size,
            average_age_ms: cache_stats.average_age_ms,
            evictions: cache_stats.evictions,
            volatility_clears: cache_stats.volatility_clears,
            is_healthy: cache_stats.is_healthy(self.config.monitoring.min_acceptable_hit_rate),
        }
    }

    /// Force clear cache (useful for testing or emergency situations)
    pub async fn clear_cache(&self, reason: &str) {
        self.quote_cache.clear_volatile_cache(reason).await;
        info!("🗑️  Jupiter cache manually cleared: {}", reason);
    }

    /// Record performance metrics
    async fn record_metrics(&self, was_cache_hit: bool, latency_ms: u64) {
        if let Ok(metrics) = self.metrics.try_lock() {
            if was_cache_hit {
                // Could add specific cache hit metrics here
                metrics.increment_opportunities_detected();
            } else {
                // Record API call metrics
                metrics.add_to_total_execution_ms(latency_ms);
            }
        }
    }

    /// Log performance statistics periodically
    async fn log_performance_stats(&self) {
        let stats = self.get_cache_statistics().await;

        if stats.is_healthy {
            info!("📊 Jupiter performance: {}", stats.summary());
        } else {
            warn!("⚠️ Jupiter performance issues: {}", stats.summary());

            if stats.hit_rate < self.config.monitoring.min_acceptable_hit_rate {
                warn!(
                    "🎯 Cache hit rate below threshold: {:.1}% < {:.1}%",
                    stats.hit_rate * 100.0,
                    self.config.monitoring.min_acceptable_hit_rate * 100.0
                );
            }
        }
    }

    /// Get configuration for monitoring/debugging
    pub fn get_config(&self) -> &JupiterIntegrationConfig {
        &self.config
    }

    /// Update cache configuration dynamically
    pub async fn update_cache_config(&mut self, new_config: CacheConfig) {
        self.config.cache = new_config;
        // Note: Would need to recreate cache with new config in a full implementation
        info!("🔧 Jupiter cache configuration updated");
    }
}

/// Comprehensive cache statistics for monitoring
#[derive(Debug, Clone)]
pub struct CacheStatistics {
    pub hit_rate: f64,
    pub total_requests: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub cache_size: usize,
    pub max_cache_size: usize,
    pub average_age_ms: u64,
    pub evictions: u64,
    pub volatility_clears: u64,
    pub is_healthy: bool,
}

impl CacheStatistics {
    /// Get a human-readable summary of cache performance
    pub fn summary(&self) -> String {
        format!(
            "Hit rate: {:.1}%, Requests: {}, Size: {}/{}, Avg age: {}ms, Evictions: {}, Healthy: {}",
            self.hit_rate * 100.0,
            self.total_requests,
            self.cache_size,
            self.max_cache_size,
            self.average_age_ms,
            self.evictions,
            self.is_healthy
        )
    }

    /// Get cache efficiency rating
    pub fn efficiency_rating(&self) -> CacheEfficiency {
        if self.hit_rate >= 0.8 {
            CacheEfficiency::Excellent
        } else if self.hit_rate >= 0.6 {
            CacheEfficiency::Good
        } else if self.hit_rate >= 0.4 {
            CacheEfficiency::Fair
        } else {
            CacheEfficiency::Poor
        }
    }
}

/// Cache efficiency rating
#[derive(Debug, Clone, PartialEq)]
pub enum CacheEfficiency {
    Excellent, // 80%+ hit rate
    Good,      // 60-80% hit rate
    Fair,      // 40-60% hit rate
    Poor,      // <40% hit rate
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::settings::Config;

    #[test]
    fn test_jupiter_integration_config_from_system_config() {
        let system_config = Config::test_default();
        let jupiter_config = JupiterIntegrationConfig::from(&system_config);

        assert_eq!(
            jupiter_config.fallback_enabled,
            system_config.jupiter_fallback_enabled
        );
        assert_eq!(
            jupiter_config.min_profit_pct,
            system_config.jupiter_fallback_min_profit_pct
        );
        assert_eq!(
            jupiter_config.max_slippage_bps,
            system_config.jupiter_slippage_tolerance_bps
        );
        assert!(jupiter_config.cache.enabled);
    }

    #[test]
    fn test_cache_efficiency_rating() {
        let excellent_stats = CacheStatistics {
            hit_rate: 0.85,
            total_requests: 100,
            cache_hits: 85,
            cache_misses: 15,
            cache_size: 50,
            max_cache_size: 100,
            average_age_ms: 2000,
            evictions: 0,
            volatility_clears: 0,
            is_healthy: true,
        };

        assert_eq!(
            excellent_stats.efficiency_rating(),
            CacheEfficiency::Excellent
        );

        let poor_stats = CacheStatistics {
            hit_rate: 0.3,
            ..excellent_stats
        };

        assert_eq!(poor_stats.efficiency_rating(), CacheEfficiency::Poor);
    }
}
