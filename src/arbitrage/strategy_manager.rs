// src/arbitrage/strategy_manager.rs
use crate::{
    arbitrage::{
        analysis::{AdvancedArbitrageMath, DynamicThresholdUpdater},
        opportunity::MultiHopArbOpportunity,
        strategy::ArbitrageStrategy,
        types::DetectionMetrics,
    },
    config::settings::Config,
    dex::{BannedPairsManager, PoolValidationConfig, validate_pools},
    error::ArbError,
    metrics::Metrics,
    utils::PoolInfo,
};
use dashmap::DashMap;
use log::{debug, info, warn};
use solana_sdk::pubkey::Pubkey;
use std::{
    sync::Arc,
    time::Instant,
};
use tokio::sync::Mutex;

/// Strategy manager responsible for arbitrage detection and analysis
pub struct StrategyManager {
    pub detector: Arc<Mutex<ArbitrageStrategy>>,
    pub dynamic_threshold_updater: Option<Arc<DynamicThresholdUpdater>>,
    pub pool_validation_config: PoolValidationConfig,
    pub banned_pairs_manager: Arc<BannedPairsManager>,
    pub advanced_math: Arc<Mutex<AdvancedArbitrageMath>>,
    pub detection_metrics: Arc<Mutex<DetectionMetrics>>,
}

impl StrategyManager {
    pub fn new(
        config: &Config,
        metrics: Arc<Mutex<Metrics>>,
        banned_pairs_manager: Arc<BannedPairsManager>,
    ) -> Self {
        let detector = Arc::new(Mutex::new(ArbitrageStrategy::new_from_config(config)));
        
        // Initialize dynamic threshold updater if configured
        let dynamic_threshold_updater = if config.volatility_tracker_window.is_some() {
            Some(Arc::new(DynamicThresholdUpdater::new(config, Arc::clone(&metrics))))
        } else {
            None
        };

        // Configure pool validation with sensible defaults
        let pool_validation_config = PoolValidationConfig {
            min_liquidity_usd: 1000.0,
            max_price_impact_bps: 500, // 5%
            require_balanced_reserves: false,
        };

        let detection_metrics = Arc::new(Mutex::new(DetectionMetrics::default()));
        let advanced_math = Arc::new(Mutex::new(AdvancedArbitrageMath::new(12))); // 12-digit precision

        Self {
            detector,
            dynamic_threshold_updater,
            pool_validation_config,
            banned_pairs_manager,
            advanced_math,
            detection_metrics,
        }
    }

    /// Enhanced opportunity detection using hot cache
    pub async fn detect_arbitrage_opportunities(
        &self,
        hot_cache: &Arc<DashMap<Pubkey, Arc<PoolInfo>>>,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        let start_time = Instant::now();
        info!("🔍 Starting enhanced arbitrage detection with hot cache...");

        // Update detection metrics
        {
            let mut metrics = self.detection_metrics.lock().await;
            metrics.total_detection_cycles += 1;
            metrics.last_detection_timestamp = chrono::Utc::now().timestamp_millis() as u64;
        }

        // Sprint 2: Use hot cache for sub-millisecond pool access
        let cache_pools: Vec<Arc<PoolInfo>> = hot_cache.iter()
            .map(|entry| Arc::clone(entry.value()))
            .collect();

        if cache_pools.is_empty() {
            warn!("🚫 No pools in hot cache for arbitrage detection");
            return Ok(vec![]);
        }

        info!("🔥 Hot cache contains {} pools for detection", cache_pools.len());

        // Record cache hit
        {
            let mut metrics = self.detection_metrics.lock().await;
            metrics.hot_cache_hits += 1;
        }

        // Pre-validate pools using enhanced validation
        let validation_start = Instant::now();
        let validated_pools = validate_pools(&cache_pools, &self.pool_validation_config)?;
        let validation_time = validation_start.elapsed();
        
        info!("✅ Pool validation completed in {:?}: {}/{} pools passed", 
              validation_time, validated_pools.len(), cache_pools.len());

        if validated_pools.is_empty() {
            warn!("🚫 No valid pools after validation");
            return Ok(vec![]);
        }

        // Filter out banned pairs
        let filtered_pools = self.filter_banned_pairs(validated_pools).await?;
        info!("🛡️ Banned pairs filter: {} pools remaining", filtered_pools.len());

        // Detect opportunities using the strategy engine
        let mut detector = self.detector.lock().await;
        let opportunities = detector.detect_opportunities(&filtered_pools).await?;
        drop(detector);

        let detection_time = start_time.elapsed();
        
        // Update metrics
        {
            let mut metrics = self.detection_metrics.lock().await;
            metrics.total_opportunities_found += opportunities.len() as u64;
            metrics.average_detection_time_ms = detection_time.as_millis() as f64;
        }

        info!("🎯 Detection complete in {:?}: {} opportunities found", 
              detection_time, opportunities.len());

        // Apply dynamic thresholds if configured
        let filtered_opportunities = if let Some(threshold_updater) = &self.dynamic_threshold_updater {
            self.apply_dynamic_thresholds(opportunities, threshold_updater).await?
        } else {
            opportunities
        };

        info!("📊 Final opportunities after filtering: {}", filtered_opportunities.len());

        Ok(filtered_opportunities)
    }

    /// Filter out banned trading pairs
    async fn filter_banned_pairs(&self, pools: Vec<Arc<PoolInfo>>) -> Result<Vec<Arc<PoolInfo>>, ArbError> {
        let mut filtered_pools = Vec::new();
        let mut banned_count = 0;

        for pool in pools {
            let pair = (pool.token_a.mint, pool.token_b.mint);
            
            if self.banned_pairs_manager.is_pair_banned(&pair.0, &pair.1).await? {
                banned_count += 1;
                debug!("🚫 Skipping banned pair: {:?}", pair);
                continue;
            }
            
            filtered_pools.push(pool);
        }

        if banned_count > 0 {
            info!("🛡️ Filtered out {} banned pairs", banned_count);
        }

        Ok(filtered_pools)
    }

    /// Apply dynamic thresholds to filter opportunities
    async fn apply_dynamic_thresholds(
        &self,
        opportunities: Vec<MultiHopArbOpportunity>,
        threshold_updater: &Arc<DynamicThresholdUpdater>,
    ) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        let mut filtered_opportunities = Vec::new();

        for opportunity in opportunities {
            // Get current dynamic threshold
            let current_threshold = threshold_updater.get_current_threshold().await?;
            
            // Apply threshold filter
            if opportunity.expected_profit_usd >= current_threshold {
                filtered_opportunities.push(opportunity);
            } else {
                debug!("🔻 Opportunity filtered by dynamic threshold: ${:.2} < ${:.2}", 
                       opportunity.expected_profit_usd, current_threshold);
            }
        }

        info!("📊 Dynamic threshold filtering: {}/{} opportunities passed", 
              filtered_opportunities.len(), opportunities.len());

        Ok(filtered_opportunities)
    }

    /// Get current detection metrics
    pub async fn get_detection_metrics(&self) -> DetectionMetrics {
        let metrics = self.detection_metrics.lock().await;
        DetectionMetrics {
            total_detection_cycles: metrics.total_detection_cycles,
            total_opportunities_found: metrics.total_opportunities_found,
            average_detection_time_ms: metrics.average_detection_time_ms,
            hot_cache_hits: metrics.hot_cache_hits,
            hot_cache_misses: metrics.hot_cache_misses,
            last_detection_timestamp: metrics.last_detection_timestamp,
        }
    }

    /// Reset detection metrics
    pub async fn reset_detection_metrics(&self) {
        let mut metrics = self.detection_metrics.lock().await;
        *metrics = DetectionMetrics::default();
        info!("📊 Detection metrics reset");
    }

    /// Update dynamic thresholds based on market conditions
    pub async fn update_dynamic_thresholds(&self) -> Result<(), ArbError> {
        if let Some(threshold_updater) = &self.dynamic_threshold_updater {
            threshold_updater.update_thresholds().await?;
            info!("📊 Dynamic thresholds updated");
        }
        Ok(())
    }
}
