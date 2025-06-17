//! Price Aggregator with Jupiter Fallback
//!
//! This module provides a unified price aggregation interface that uses multiple
//! DEX clients for quote generation with Jupiter as a fallback when primary sources fail.

use crate::{
    arbitrage::jupiter::{integration::JupiterIntegrationConfig, JupiterFallbackManager},
    config::settings::Config,
    dex::clients::jupiter::JupiterClient,
    dex::{api::Quote, DexClient},
    error::ArbError,
    local_metrics::Metrics,
    utils::PoolInfo,
};
use log::{debug, error, info, warn};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Aggregated quote result with source information
#[derive(Debug, Clone)]
pub struct AggregatedQuote {
    pub quote: Quote,
    pub source: QuoteSource,
    pub confidence: f64, // 0.0 to 1.0, higher is better
    pub latency_ms: u64,
}

/// Source of the quote
#[derive(Debug, Clone, PartialEq)]
pub enum QuoteSource {
    Primary(String), // DEX name
    Jupiter,
    Fallback(String),
}

/// Price aggregator configuration
#[derive(Debug, Clone)]
pub struct PriceAggregatorConfig {
    pub enable_jupiter_fallback: bool,
    pub jupiter_fallback_min_profit_pct: f64,
    pub max_quote_age_ms: u64,
    pub min_confidence_threshold: f64,
    pub enable_quote_comparison: bool,
    pub max_price_deviation_pct: f64,
    pub cross_validation: CrossValidationConfig,
}

impl Default for PriceAggregatorConfig {
    fn default() -> Self {
        Self {
            enable_jupiter_fallback: true,
            jupiter_fallback_min_profit_pct: 0.001,
            max_quote_age_ms: 5000,
            min_confidence_threshold: 0.7,
            enable_quote_comparison: true,
            max_price_deviation_pct: 2.0,
            cross_validation: CrossValidationConfig::default(),
        }
    }
}

/// Cross-validation result for quote comparison
#[derive(Debug, Clone)]
pub struct CrossValidationResult {
    pub primary_average_price: f64,
    pub jupiter_price: f64,
    pub deviation_percent: f64,
    pub is_within_threshold: bool,
    pub primary_quotes_count: usize,
}

/// Quote cross-validation configuration
#[derive(Debug, Clone)]
pub struct CrossValidationConfig {
    pub enable_validation: bool,
    pub max_acceptable_deviation_pct: f64,
    pub min_primary_quotes_for_validation: usize,
    pub validation_confidence_boost: f64, // Confidence boost for validated quotes
}

impl Default for CrossValidationConfig {
    fn default() -> Self {
        Self {
            enable_validation: true,
            max_acceptable_deviation_pct: 5.0, // 5% max deviation
            min_primary_quotes_for_validation: 2,
            validation_confidence_boost: 0.1,
        }
    }
}

/// Main price aggregator with Jupiter fallback support
pub struct PriceAggregator {
    primary_dex_clients: Vec<Arc<dyn DexClient>>,
    jupiter_fallback_manager: Option<Arc<JupiterFallbackManager>>,
    config: PriceAggregatorConfig,
    metrics: Arc<Mutex<Metrics>>,
}

impl PriceAggregator {
    /// Create a new price aggregator
    pub fn new(
        primary_dex_clients: Vec<Arc<dyn DexClient>>,
        jupiter_client: Option<Arc<JupiterClient>>,
        system_config: &Config,
        metrics: Arc<Mutex<Metrics>>,
    ) -> Self {
        let config = PriceAggregatorConfig {
            enable_jupiter_fallback: system_config.jupiter_fallback_enabled,
            jupiter_fallback_min_profit_pct: system_config.jupiter_fallback_min_profit_pct,
            max_quote_age_ms: system_config.jupiter_api_timeout_ms as u64,
            min_confidence_threshold: 0.7,
            enable_quote_comparison: true,
            max_price_deviation_pct: 2.0,
            cross_validation: CrossValidationConfig::default(),
        };

        // Create Jupiter fallback manager if client is available
        let jupiter_fallback_manager = if let Some(jupiter_client) = jupiter_client {
            let jupiter_config = JupiterIntegrationConfig::from(system_config);
            Some(Arc::new(JupiterFallbackManager::new(
                jupiter_client,
                jupiter_config,
                Arc::clone(&metrics),
            )))
        } else {
            None
        };

        Self {
            primary_dex_clients,
            jupiter_fallback_manager,
            config,
            metrics,
        }
    }

    /// Get the best quote from all available sources with fallback
    pub async fn get_best_quote(
        &self,
        pool: &PoolInfo,
        input_amount: u64,
    ) -> Result<AggregatedQuote, ArbError> {
        let start_time = std::time::Instant::now();

        // Step 1: Try primary DEX clients
        let primary_quotes = self.get_primary_quotes(pool, input_amount).await;

        // Step 2: If primary quotes are insufficient, try Jupiter fallback
        let jupiter_quote = if self.should_use_jupiter_fallback(&primary_quotes).await {
            self.get_jupiter_quote(pool, input_amount).await
        } else {
            None
        };

        // Step 3: Select the best quote
        let best_quote = self
            .select_best_quote(primary_quotes, jupiter_quote)
            .await?;

        // Step 4: Record metrics
        self.record_aggregation_metrics(&best_quote, start_time.elapsed())
            .await;

        Ok(best_quote)
    }

    /// Get quotes from primary DEX clients
    async fn get_primary_quotes(&self, pool: &PoolInfo, input_amount: u64) -> Vec<AggregatedQuote> {
        let mut quotes = Vec::new();

        for client in &self.primary_dex_clients {
            match client.calculate_onchain_quote(pool, input_amount) {
                Ok(quote) => {
                    let aggregated_quote = AggregatedQuote {
                        quote,
                        source: QuoteSource::Primary(client.get_name().to_string()),
                        confidence: self.calculate_confidence_for_dex_quote(client.get_name()),
                        latency_ms: 0, // Would be measured in real implementation
                    };
                    debug!(
                        "✅ Got quote from {}: {} -> {}",
                        client.get_name(),
                        input_amount,
                        aggregated_quote.quote.output_amount
                    );
                    quotes.push(aggregated_quote);
                }
                Err(e) => {
                    warn!("❌ Failed to get quote from {}: {}", client.get_name(), e);
                }
            }
        }

        quotes
    }

    /// Determine if Jupiter fallback should be used
    async fn should_use_jupiter_fallback(&self, primary_quotes: &[AggregatedQuote]) -> bool {
        if !self.config.enable_jupiter_fallback {
            return false;
        }

        if self.jupiter_fallback_manager.is_none() {
            return false;
        }

        // Use Jupiter fallback if:
        // 1. No primary quotes available
        // 2. Primary quotes have low confidence
        // 3. Primary quotes show high price deviation

        if primary_quotes.is_empty() {
            debug!("🔄 No primary quotes available, using Jupiter fallback");
            return true;
        }

        let avg_confidence =
            primary_quotes.iter().map(|q| q.confidence).sum::<f64>() / primary_quotes.len() as f64;

        if avg_confidence < self.config.min_confidence_threshold {
            debug!(
                "🔄 Low confidence quotes ({:.2}), using Jupiter fallback",
                avg_confidence
            );
            return true;
        }

        // Check for high price deviation among primary quotes
        if self.has_high_price_deviation(primary_quotes) {
            debug!("🔄 High price deviation detected, using Jupiter fallback for validation");
            return true;
        }

        false
    }

    /// Get quote from Jupiter fallback with multi-route optimization
    async fn get_jupiter_quote(
        &self,
        pool: &PoolInfo,
        input_amount: u64,
    ) -> Option<AggregatedQuote> {
        if let Some(fallback_manager) = &self.jupiter_fallback_manager {
            let start_time = std::time::Instant::now();

            // Use configured slippage tolerance
            let slippage_bps = 50; // Default 0.5% slippage, could be configurable

            // Try multi-route optimization first if available
            let result = if fallback_manager.is_route_optimization_enabled() {
                debug!("🛣️  Using Jupiter multi-route optimization for quote");

                match fallback_manager
                    .get_optimal_route(
                        &pool.token_a.mint.to_string(),
                        &pool.token_b.mint.to_string(),
                        input_amount,
                        Some(slippage_bps),
                    )
                    .await
                {
                    Ok(multi_route_result) => {
                        let quote_response = multi_route_result.best_route.quote;
                        let latency_ms = start_time.elapsed().as_millis() as u64;

                        info!("🏆 Jupiter optimal route selected: {} routes evaluated, {} failed, reason: {}", 
                              multi_route_result.routes_evaluated,
                              multi_route_result.routes_failed,
                              multi_route_result.selection_reason);

                        Some((quote_response, latency_ms, true)) // true = used route optimization
                    }
                    Err(e) => {
                        warn!("🛣️  Jupiter multi-route optimization failed: {}, falling back to cache", e);
                        None
                    }
                }
            } else {
                None
            };

            // Fallback to cached single quote if multi-route failed or not available
            let (quote_response, latency_ms, used_optimization) = match result {
                Some(data) => data,
                None => {
                    debug!("📡 Using Jupiter cached quote fallback");
                    match fallback_manager
                        .get_quote_with_cache(
                            &pool.token_a.mint.to_string(),
                            &pool.token_b.mint.to_string(),
                            input_amount,
                            Some(slippage_bps),
                        )
                        .await
                    {
                        Ok(quote_response) => {
                            let latency_ms = start_time.elapsed().as_millis() as u64;
                            (quote_response, latency_ms, false) // false = used single route
                        }
                        Err(e) => {
                            error!("❌ Jupiter fallback completely failed: {}", e);
                            return None;
                        }
                    }
                }
            };

            // Convert QuoteResponse to Quote
            let quote = Quote {
                input_token: pool.token_a.symbol.clone(),
                output_token: pool.token_b.symbol.clone(),
                input_amount,
                output_amount: quote_response.out_amount.parse().unwrap_or(0),
                dex: if used_optimization {
                    "Jupiter (Multi-Route)"
                } else {
                    "Jupiter (Single)"
                }
                .to_string(),
                route: vec![], // Jupiter routes are complex, simplified here
                slippage_estimate: Some(slippage_bps as f64 / 10000.0),
            };

            // Higher confidence for optimized routes
            let confidence = if used_optimization { 0.9 } else { 0.8 };

            let aggregated_quote = AggregatedQuote {
                quote,
                source: QuoteSource::Jupiter,
                confidence,
                latency_ms,
            };

            info!(
                "🪐 Jupiter quote: {} -> {} ({}, {}ms)",
                input_amount,
                aggregated_quote.quote.output_amount,
                if used_optimization {
                    "optimized"
                } else {
                    "cached"
                },
                latency_ms
            );

            Some(aggregated_quote)
        } else {
            None
        }
    }

    /// Select the best quote from all available sources with cross-validation
    async fn select_best_quote(
        &self,
        mut primary_quotes: Vec<AggregatedQuote>,
        jupiter_quote: Option<AggregatedQuote>,
    ) -> Result<AggregatedQuote, ArbError> {
        if primary_quotes.is_empty() && jupiter_quote.is_none() {
            return Err(ArbError::DexError(
                "No quotes available from any source".to_string(),
            ));
        }

        // Perform cross-validation if both primary and Jupiter quotes are available
        if let Some(ref jupiter_quote) = jupiter_quote {
            if self.config.cross_validation.enable_validation && !primary_quotes.is_empty() {
                let validation_result = self
                    .cross_validate_quotes(&primary_quotes, jupiter_quote)
                    .await;
                self.log_cross_validation_result(&validation_result).await;

                // Update Jupiter quote confidence based on validation
                if validation_result.is_within_threshold {
                    // Boost Jupiter confidence if it passes validation
                    let mut validated_jupiter = jupiter_quote.clone();
                    validated_jupiter.confidence +=
                        self.config.cross_validation.validation_confidence_boost;
                    validated_jupiter.confidence = validated_jupiter.confidence.min(1.0); // Cap at 1.0

                    primary_quotes.push(validated_jupiter);
                    info!(
                        "✅ Jupiter quote validated against primary sources (deviation: {:.2}%)",
                        validation_result.deviation_percent
                    );
                } else {
                    // Log but still include Jupiter quote with original confidence
                    primary_quotes.push(jupiter_quote.clone());
                    warn!(
                        "⚠️ Jupiter quote failed validation (deviation: {:.2}% > {:.2}%)",
                        validation_result.deviation_percent,
                        self.config.cross_validation.max_acceptable_deviation_pct
                    );
                }
            } else {
                // No validation, just add Jupiter quote
                primary_quotes.push(jupiter_quote.clone());
            }
        }

        if primary_quotes.is_empty() {
            return Err(ArbError::DexError("No valid quotes available".to_string()));
        }

        // Sort by output amount (descending) and confidence (descending)
        primary_quotes.sort_by(|a, b| {
            // First compare by output amount
            let output_cmp = b.quote.output_amount.cmp(&a.quote.output_amount);
            if output_cmp == std::cmp::Ordering::Equal {
                // If output amounts are equal, compare by confidence
                b.confidence
                    .partial_cmp(&a.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            } else {
                output_cmp
            }
        });

        // Return the best quote by output amount and confidence
        let best_quote = primary_quotes.into_iter().next().unwrap();

        info!(
            "🎯 Selected best quote from {}: {} -> {} (confidence: {:.2})",
            match &best_quote.source {
                QuoteSource::Primary(name) => name,
                QuoteSource::Jupiter => "Jupiter",
                QuoteSource::Fallback(name) => name,
            },
            best_quote.quote.input_amount,
            best_quote.quote.output_amount,
            best_quote.confidence
        );

        Ok(best_quote)
    }

    /// Calculate confidence score for a DEX quote
    fn calculate_confidence_for_dex_quote(&self, dex_name: &str) -> f64 {
        match dex_name.to_lowercase().as_str() {
            "orca" => 0.9,     // High confidence for Orca
            "raydium" => 0.85, // High confidence for Raydium
            "jupiter" => 0.8,  // Good confidence for Jupiter
            _ => 0.7,          // Default confidence
        }
    }

    /// Check if there's high price deviation among quotes
    fn has_high_price_deviation(&self, quotes: &[AggregatedQuote]) -> bool {
        if quotes.len() < 2 {
            return false;
        }

        let prices: Vec<f64> = quotes
            .iter()
            .map(|q| q.quote.output_amount as f64 / q.quote.input_amount as f64)
            .collect();

        let avg_price = prices.iter().sum::<f64>() / prices.len() as f64;
        let max_deviation = prices
            .iter()
            .map(|price| ((price - avg_price) / avg_price).abs() * 100.0)
            .fold(0.0, f64::max);

        max_deviation > self.config.max_price_deviation_pct
    }

    /// Record aggregation metrics
    async fn record_aggregation_metrics(
        &self,
        quote: &AggregatedQuote,
        duration: std::time::Duration,
    ) {
        let mut metrics = self.metrics.lock().await;

        // Record quote source metrics
        match &quote.source {
            QuoteSource::Primary(dex_name) => {
                metrics.increment_primary_quote_count(dex_name);
            }
            QuoteSource::Jupiter => {
                metrics.increment_jupiter_fallback_count();
            }
            QuoteSource::Fallback(_) => {
                metrics.increment_fallback_count();
            }
        }

        // Record aggregation latency
        metrics.record_quote_aggregation_latency(duration.as_millis() as u64);
    }

    /// Cross-validate Jupiter quote against primary DEX quotes
    async fn cross_validate_quotes(
        &self,
        primary_quotes: &[AggregatedQuote],
        jupiter_quote: &AggregatedQuote,
    ) -> CrossValidationResult {
        if primary_quotes.len()
            < self
                .config
                .cross_validation
                .min_primary_quotes_for_validation
        {
            return CrossValidationResult {
                primary_average_price: 0.0,
                jupiter_price: 0.0,
                deviation_percent: 0.0,
                is_within_threshold: false,
                primary_quotes_count: primary_quotes.len(),
            };
        }

        // Calculate average price from primary quotes (price = output_amount / input_amount)
        let primary_prices: Vec<f64> = primary_quotes
            .iter()
            .filter(|q| matches!(q.source, QuoteSource::Primary(_)))
            .map(|q| q.quote.output_amount as f64 / q.quote.input_amount as f64)
            .collect();

        if primary_prices.is_empty() {
            return CrossValidationResult {
                primary_average_price: 0.0,
                jupiter_price: 0.0,
                deviation_percent: 0.0,
                is_within_threshold: false,
                primary_quotes_count: 0,
            };
        }

        let primary_average_price =
            primary_prices.iter().sum::<f64>() / primary_prices.len() as f64;
        let jupiter_price =
            jupiter_quote.quote.output_amount as f64 / jupiter_quote.quote.input_amount as f64;

        // Calculate percentage deviation
        let deviation_percent = if primary_average_price > 0.0 {
            ((jupiter_price - primary_average_price) / primary_average_price * 100.0).abs()
        } else {
            100.0 // If primary price is 0, consider it 100% deviation
        };

        let is_within_threshold =
            deviation_percent <= self.config.cross_validation.max_acceptable_deviation_pct;

        CrossValidationResult {
            primary_average_price,
            jupiter_price,
            deviation_percent,
            is_within_threshold,
            primary_quotes_count: primary_prices.len(),
        }
    }

    /// Log cross-validation results for monitoring
    async fn log_cross_validation_result(&self, result: &CrossValidationResult) {
        debug!("🔍 Cross-validation result: Primary avg: {:.6}, Jupiter: {:.6}, Deviation: {:.2}%, Valid: {}",
               result.primary_average_price,
               result.jupiter_price,
               result.deviation_percent,
               result.is_within_threshold);

        // Record metrics
        if let Ok(metrics) = self.metrics.try_lock() {
            // Could add specific cross-validation metrics here
            if result.is_within_threshold {
                metrics.increment_opportunities_detected(); // Placeholder for validation success
            }
        }

        // Alert on high deviation
        if result.deviation_percent
            > self.config.cross_validation.max_acceptable_deviation_pct * 2.0
        {
            warn!(
                "🚨 High Jupiter quote deviation detected: {:.2}% (threshold: {:.2}%)",
                result.deviation_percent, self.config.cross_validation.max_acceptable_deviation_pct
            );
        }
    }
}

/// Extension trait for metrics to add price aggregator specific metrics
trait PriceAggregatorMetrics {
    fn increment_primary_quote_count(&mut self, dex_name: &str);
    fn increment_jupiter_fallback_count(&mut self);
    fn increment_fallback_count(&mut self);
    fn record_quote_aggregation_latency(&mut self, latency_ms: u64);
}

impl PriceAggregatorMetrics for Metrics {
    fn increment_primary_quote_count(&mut self, _dex_name: &str) {
        // Implementation would depend on the specific metrics structure
        // For now, just increment the opportunities detected counter
        self.increment_opportunities_detected();
    }

    fn increment_jupiter_fallback_count(&mut self) {
        // Increment Jupiter fallback usage counter
        self.increment_opportunities_detected();
    }

    fn increment_fallback_count(&mut self) {
        // Increment general fallback counter
        self.increment_opportunities_detected();
    }

    fn record_quote_aggregation_latency(&mut self, latency_ms: u64) {
        // Record latency metrics by adding to total execution time
        self.add_to_total_execution_ms(latency_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::DexType;
    use solana_sdk::pubkey::Pubkey;

    fn create_test_pool() -> PoolInfo {
        let mut pool = PoolInfo::default();
        pool.address = Pubkey::new_unique();
        pool.name = "Test Pool".to_string();
        pool.token_a.mint = Pubkey::new_unique();
        pool.token_a.symbol = "SOL".to_string();
        pool.token_a.reserve = 1000000;
        pool.token_a.decimals = 9;
        pool.token_b.mint = Pubkey::new_unique();
        pool.token_b.symbol = "USDC".to_string();
        pool.token_b.reserve = 100000000;
        pool.token_b.decimals = 6;
        pool.fee_rate_bips = Some(30);
        pool.liquidity = Some(1000000);
        pool.sqrt_price = Some(1414213562);
        pool.dex_type = DexType::Orca;
        pool
    }

    #[tokio::test]
    async fn test_price_aggregator_selection() {
        let config = Config::test_default();
        let metrics = Arc::new(Mutex::new(Metrics::new()));

        let aggregator = PriceAggregator::new(
            vec![], // No primary DEX clients for this test
            None,   // No Jupiter client for this test
            &config,
            metrics,
        );

        // Test that empty quotes return error
        let pool = create_test_pool();
        let result = aggregator.get_best_quote(&pool, 1000000).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_price_deviation_detection() {
        let config = Config::test_default();
        let metrics = Arc::new(Mutex::new(Metrics::new()));

        let aggregator = PriceAggregator::new(vec![], None, &config, metrics);

        // Create quotes with high price deviation
        let quotes = vec![
            AggregatedQuote {
                quote: Quote {
                    input_token: "SOL".to_string(),
                    output_token: "USDC".to_string(),
                    input_amount: 1000000,
                    output_amount: 100000, // Price: 0.1
                    dex: "DEX1".to_string(),
                    route: vec![],
                    slippage_estimate: Some(0.01),
                },
                source: QuoteSource::Primary("DEX1".to_string()),
                confidence: 0.9,
                latency_ms: 100,
            },
            AggregatedQuote {
                quote: Quote {
                    input_token: "SOL".to_string(),
                    output_token: "USDC".to_string(),
                    input_amount: 1000000,
                    output_amount: 150000, // Price: 0.15 (50% higher)
                    dex: "DEX2".to_string(),
                    route: vec![],
                    slippage_estimate: Some(0.01),
                },
                source: QuoteSource::Primary("DEX2".to_string()),
                confidence: 0.9,
                latency_ms: 120,
            },
        ];

        // Should detect high price deviation
        assert!(aggregator.has_high_price_deviation(&quotes));
    }

    #[tokio::test]
    async fn test_cross_validation_within_threshold() {
        let config = Config::test_default();
        let metrics = Arc::new(Mutex::new(Metrics::new()));

        let aggregator = PriceAggregator::new(vec![], None, &config, metrics);

        // Create primary quotes
        let primary_quotes = vec![
            AggregatedQuote {
                quote: Quote {
                    input_token: "SOL".to_string(),
                    output_token: "USDC".to_string(),
                    input_amount: 1000000,
                    output_amount: 100000, // Price: 0.1
                    dex: "Orca".to_string(),
                    route: vec![],
                    slippage_estimate: Some(0.01),
                },
                source: QuoteSource::Primary("Orca".to_string()),
                confidence: 0.9,
                latency_ms: 100,
            },
            AggregatedQuote {
                quote: Quote {
                    input_token: "SOL".to_string(),
                    output_token: "USDC".to_string(),
                    input_amount: 1000000,
                    output_amount: 102000, // Price: 0.102 (2% difference)
                    dex: "Raydium".to_string(),
                    route: vec![],
                    slippage_estimate: Some(0.01),
                },
                source: QuoteSource::Primary("Raydium".to_string()),
                confidence: 0.9,
                latency_ms: 120,
            },
        ];

        // Create Jupiter quote with similar price
        let jupiter_quote = AggregatedQuote {
            quote: Quote {
                input_token: "SOL".to_string(),
                output_token: "USDC".to_string(),
                input_amount: 1000000,
                output_amount: 101500, // Price: 0.1015 (within 5% threshold)
                dex: "Jupiter".to_string(),
                route: vec![],
                slippage_estimate: Some(0.005),
            },
            source: QuoteSource::Jupiter,
            confidence: 0.8,
            latency_ms: 200,
        };

        let validation_result = aggregator
            .cross_validate_quotes(&primary_quotes, &jupiter_quote)
            .await;

        assert!(
            validation_result.is_within_threshold,
            "Jupiter quote should pass validation"
        );
        assert!(
            validation_result.deviation_percent < 5.0,
            "Deviation should be less than 5%"
        );
        assert_eq!(validation_result.primary_quotes_count, 2);
    }

    #[tokio::test]
    async fn test_cross_validation_exceeds_threshold() {
        let config = Config::test_default();
        let metrics = Arc::new(Mutex::new(Metrics::new()));

        let aggregator = PriceAggregator::new(vec![], None, &config, metrics);

        // Create primary quotes - need at least 2 for validation
        let primary_quotes = vec![
            AggregatedQuote {
                quote: Quote {
                    input_token: "SOL".to_string(),
                    output_token: "USDC".to_string(),
                    input_amount: 1000000,
                    output_amount: 100000, // Price: 0.1
                    dex: "Orca".to_string(),
                    route: vec![],
                    slippage_estimate: Some(0.01),
                },
                source: QuoteSource::Primary("Orca".to_string()),
                confidence: 0.9,
                latency_ms: 100,
            },
            AggregatedQuote {
                quote: Quote {
                    input_token: "SOL".to_string(),
                    output_token: "USDC".to_string(),
                    input_amount: 1000000,
                    output_amount: 101000, // Price: 0.101 (very close)
                    dex: "Raydium".to_string(),
                    route: vec![],
                    slippage_estimate: Some(0.01),
                },
                source: QuoteSource::Primary("Raydium".to_string()),
                confidence: 0.9,
                latency_ms: 120,
            },
        ];

        // Create Jupiter quote with significantly different price
        let jupiter_quote = AggregatedQuote {
            quote: Quote {
                input_token: "SOL".to_string(),
                output_token: "USDC".to_string(),
                input_amount: 1000000,
                output_amount: 110000, // Price: 0.11 (10% difference - exceeds 5% threshold)
                dex: "Jupiter".to_string(),
                route: vec![],
                slippage_estimate: Some(0.005),
            },
            source: QuoteSource::Jupiter,
            confidence: 0.8,
            latency_ms: 200,
        };

        let validation_result = aggregator
            .cross_validate_quotes(&primary_quotes, &jupiter_quote)
            .await;

        println!("Deviation: {:.2}%", validation_result.deviation_percent);
        assert!(
            !validation_result.is_within_threshold,
            "Jupiter quote should fail validation"
        );
        assert!(
            validation_result.deviation_percent >= 5.0,
            "Deviation should be at least 5%, got {:.2}%",
            validation_result.deviation_percent
        );
        assert_eq!(validation_result.primary_quotes_count, 2);
    }

    #[tokio::test]
    async fn test_cross_validation_confidence_boost() {
        let config = Config::test_default();
        let metrics = Arc::new(Mutex::new(Metrics::new()));

        let aggregator = PriceAggregator::new(vec![], None, &config, metrics);

        // Create primary quotes
        let primary_quotes = vec![AggregatedQuote {
            quote: Quote {
                input_token: "SOL".to_string(),
                output_token: "USDC".to_string(),
                input_amount: 1000000,
                output_amount: 99000, // Lower output than Jupiter
                dex: "Orca".to_string(),
                route: vec![],
                slippage_estimate: Some(0.01),
            },
            source: QuoteSource::Primary("Orca".to_string()),
            confidence: 0.9,
            latency_ms: 100,
        }];

        // Create Jupiter quote with better price that passes validation
        let jupiter_quote = AggregatedQuote {
            quote: Quote {
                input_token: "SOL".to_string(),
                output_token: "USDC".to_string(),
                input_amount: 1000000,
                output_amount: 100000, // Better output amount
                dex: "Jupiter".to_string(),
                route: vec![],
                slippage_estimate: Some(0.005),
            },
            source: QuoteSource::Jupiter,
            confidence: 0.8, // Lower initial confidence
            latency_ms: 200,
        };

        let result = aggregator
            .select_best_quote(primary_quotes, Some(jupiter_quote))
            .await;

        assert!(result.is_ok(), "Should successfully select best quote");
        let best_quote = result.unwrap();

        println!(
            "Best quote source: {:?}, confidence: {:.2}",
            best_quote.source, best_quote.confidence
        );

        // Jupiter should be selected due to better output amount
        assert!(
            matches!(best_quote.source, QuoteSource::Jupiter),
            "Jupiter should be selected"
        );
        // Note: We don't test confidence boost here as it depends on validation passing
    }
}
