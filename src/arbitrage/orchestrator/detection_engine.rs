//! Opportunity Detection Engine
//! 
//! This module handles all arbitrage opportunity detection logic including
//! hot cache management, pool validation, and opportunity analysis.

use super::core::ArbitrageOrchestrator;
use crate::{
    arbitrage::opportunity::MultiHopArbOpportunity,
    error::ArbError,
    utils::PoolInfo,
};

use log::{info, warn, debug, error};
use std::{
    sync::Arc,
    time::Instant,
};

impl ArbitrageOrchestrator {
    /// Enhanced opportunity detection using hot cache
    pub async fn detect_arbitrage_opportunities(&self) -> Result<Vec<MultiHopArbOpportunity>, ArbError> {
        use crate::utils::timing::Timer;
        
        let mut timer = Timer::start("enhanced_arbitrage_detection");
        info!("🔍 Starting enhanced arbitrage detection with hot cache...");

        // Get pools snapshot from hot cache
        let pools_snapshot = self.get_hot_cache_snapshot().await;
        let pool_count = pools_snapshot.len();
        timer.checkpoint("hot_cache_snapshot");
        
        if pool_count == 0 {
            warn!("No pools available in hot cache for arbitrage detection");
            return Ok(Vec::new());
        }

        info!("📊 Analyzing {} pools from hot cache for arbitrage opportunities", pool_count);

        // Validate pools before detection
        let validated_pools = self.validate_hot_cache_pools(&pools_snapshot).await?;
        let validation_filtered = pool_count - validated_pools.len();
        timer.checkpoint("pool_validation");
        
        if validation_filtered > 0 {
            warn!("🔍 Pool validation filtered out {} of {} pools ({:.1}% rejection rate)", 
                  validation_filtered, pool_count, (validation_filtered as f64 / pool_count as f64) * 100.0);
        }

        // Run enhanced detection with validated pools
        let opportunities = {
            let detector = self.detector.lock().await;
            // Convert Vec to HashMap for compatibility
            let pools_map: std::collections::HashMap<solana_sdk::pubkey::Pubkey, Arc<PoolInfo>> = 
                validated_pools.iter().map(|pool| (pool.address, pool.clone())).collect();
            detector.detect_all_opportunities(&pools_map, &self.metrics).await?
        };
        timer.checkpoint("opportunity_detection");

        let detection_time = timer.finish_with_threshold(2000); // Warn if > 2 seconds
        info!("⚡ Enhanced detection completed in {:.2}ms, found {} opportunities", 
              detection_time.as_millis(), opportunities.len());

        // Update detection metrics
        self.update_detection_metrics(opportunities.len(), detection_time.as_millis() as f64).await;

        Ok(opportunities)
    }

    /// Get a snapshot of the hot cache for opportunity detection
    pub async fn get_hot_cache_snapshot(&self) -> Vec<Arc<PoolInfo>> {
        let start_time = Instant::now();
        
        // Convert DashMap to Vec efficiently
        let pools: Vec<Arc<PoolInfo>> = self.hot_cache
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        
        let snapshot_time = start_time.elapsed();
        debug!("📸 Hot cache snapshot taken: {} pools in {:.2}ms", 
               pools.len(), snapshot_time.as_millis());
        
        // Update cache hit metrics
        self.update_cache_metrics(pools.len(), 0).await;
        
        pools
    }

    /// Validate pools from hot cache before detection
    pub async fn validate_hot_cache_pools(&self, pools: &[Arc<PoolInfo>]) -> Result<Vec<Arc<PoolInfo>>, ArbError> {
        let start_time = Instant::now();
        let mut validated_pools = Vec::new();
        let mut validation_errors = 0;

        for pool in pools {
            // Basic validation checks
            if self.is_pool_valid(pool).await {
                validated_pools.push(pool.clone());
            } else {
                validation_errors += 1;
            }
        }

        let validation_time = start_time.elapsed();
        debug!("✅ Pool validation completed: {}/{} pools valid in {:.2}ms", 
               validated_pools.len(), pools.len(), validation_time.as_millis());

        if validation_errors > 0 {
            warn!("⚠️ {} pools failed validation checks", validation_errors);
        }

        Ok(validated_pools)
    }

    /// Check if a pool meets validation criteria
    async fn is_pool_valid(&self, pool: &PoolInfo) -> bool {
        // Check banned pairs
        if self.banned_pairs_manager.is_pair_banned(&pool.token_a.mint.to_string(), &pool.token_b.mint.to_string()) {
            debug!("🚫 Pool rejected: banned pair ({}, {})", 
                   pool.token_a.mint, pool.token_b.mint);
            return false;
        }

        // Check minimum liquidity
        if let Some(liquidity) = pool.liquidity {
            if (liquidity as f64) < self.pool_validation_config.min_liquidity_usd {
                debug!("💧 Pool rejected: insufficient liquidity {:.2} < {:.2}", 
                       liquidity, self.pool_validation_config.min_liquidity_usd);
                return false;
            }
        }

        // Check balanced reserves if required
        if self.pool_validation_config.require_balanced_reserves {
            let reserve_ratio = if pool.token_b.reserve > 0 {
                pool.token_a.reserve as f64 / pool.token_b.reserve as f64
            } else {
                return false;
            };
            
            // Reject if reserves are too imbalanced (more than 10:1 ratio)
            if reserve_ratio > 10.0 || reserve_ratio < 0.1 {
                debug!("⚖️ Pool rejected: imbalanced reserves ratio {:.2}", reserve_ratio);
                return false;
            }
        }

        true
    }

    /// Update detection metrics
    async fn update_detection_metrics(&self, opportunities_found: usize, detection_time_ms: f64) {
        // This would be implemented with a proper metrics system
        // For now, we'll just log the metrics
        debug!("📈 Detection metrics: {} opportunities, {:.2}ms", 
               opportunities_found, detection_time_ms);
    }

    /// Update cache hit/miss metrics
    async fn update_cache_metrics(&self, hits: usize, misses: usize) {
        // This would be implemented with a proper metrics system
        debug!("🎯 Cache metrics: {} hits, {} misses", hits, misses);
    }

    /// Refresh hot cache from multiple sources
    pub async fn refresh_hot_cache(&self) -> Result<usize, ArbError> {
        let start_time = Instant::now();
        info!("🔄 Refreshing hot cache from all DEX providers...");

        let mut total_pools_added = 0;
        let mut total_errors = 0;

        // Refresh from each DEX provider
        for dex_provider in &self.dex_providers {
            match self.refresh_cache_from_provider(dex_provider.as_ref()).await {
                Ok(pools_added) => {
                    total_pools_added += pools_added;
                    debug!("✅ Added {} pools from {}", pools_added, dex_provider.get_name());
                }
                Err(e) => {
                    total_errors += 1;
                    error!("❌ Failed to refresh cache from {}: {}", dex_provider.get_name(), e);
                }
            }
        }

        let refresh_time = start_time.elapsed();
        
        if total_errors == 0 {
            info!("🎉 Hot cache refresh completed: {} pools added in {:.2}ms", 
                  total_pools_added, refresh_time.as_millis());
        } else {
            warn!("⚠️ Hot cache refresh completed with {} errors: {} pools added in {:.2}ms", 
                  total_errors, total_pools_added, refresh_time.as_millis());
        }

        Ok(total_pools_added)
    }

    /// Refresh cache from a specific DEX provider
    async fn refresh_cache_from_provider(&self, provider: &dyn crate::dex::DexClient) -> Result<usize, ArbError> {
        let pools = provider.discover_pools().await?;
        let mut pools_added = 0;

        for pool in pools {
            self.hot_cache.insert(pool.address, Arc::new(pool));
            pools_added += 1;
        }

        Ok(pools_added)
    }

    /// Remove stale pools from hot cache
    pub async fn cleanup_stale_pools(&self) -> Result<usize, ArbError> {
        let start_time = Instant::now();
        let removed_count = 0;
        
        // This is a simplified cleanup - in practice, you'd check for stale data
        // For now, we'll just log that cleanup was attempted
        let cleanup_time = start_time.elapsed();
        
        debug!("🧹 Hot cache cleanup completed: {} stale pools removed in {:.2}ms", 
               removed_count, cleanup_time.as_millis());

        Ok(removed_count)
    }

    /// Get hot cache statistics
    pub async fn get_cache_stats(&self) -> CacheStats {
        CacheStats {
            total_pools: self.hot_cache.len(),
            memory_usage_bytes: self.estimate_cache_memory_usage(),
            last_refresh_time: Instant::now(), // Simplified
        }
    }

    /// Estimate memory usage of hot cache
    fn estimate_cache_memory_usage(&self) -> usize {
        // Rough estimation: each PoolInfo is approximately 1KB
        self.hot_cache.len() * 1024
    }
}

#[derive(Debug)]
pub struct CacheStats {
    pub total_pools: usize,
    pub memory_usage_bytes: usize,
    pub last_refresh_time: Instant,
}
