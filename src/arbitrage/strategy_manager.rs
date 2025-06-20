// src/arbitrage/strategy_manager.rs
use crate::{
    arbitrage::{
        opportunity::MultiHopArbOpportunity,
        strategy::ArbitrageStrategy,
        types::DetectionMetrics,
        analysis::EnhancedSlippageModel,
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
    pub pool_validation_config: PoolValidationConfig,
    pub banned_pairs_manager: Arc<BannedPairsManager>,
    pub detection_metrics: Arc<Mutex<DetectionMetrics>>,
    pub slippage_model: Arc<Mutex<EnhancedSlippageModel>>,
    // Removed dynamic_threshold_updater and advanced_math (missing types)
}

impl StrategyManager {
    pub fn new(
        config: &Config,
        metrics: Arc<Mutex<Metrics>>,
        banned_pairs_manager: Arc<BannedPairsManager>,
    ) -> Self {
        let detector = Arc::new(Mutex::new(ArbitrageStrategy::new_from_config(config)));
        // Configure pool validation with sensible defaults
        let pool_validation_config = PoolValidationConfig {
            min_liquidity_usd: 1000.0,
            max_price_impact_bps: 500, // 5%
            require_balanced_reserves: false,
        };
        let detection_metrics = Arc::new(Mutex::new(DetectionMetrics::default()));
        let slippage_model = Arc::new(Mutex::new(EnhancedSlippageModel::new()));
        Self {
            detector,
            pool_validation_config,
            banned_pairs_manager,
            detection_metrics,
            slippage_model,
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

        Ok(opportunities)
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
}
