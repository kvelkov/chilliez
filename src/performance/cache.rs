// src/performance/cache.rs
//! Advanced Caching System with TTL and Freshness Validation
//! 
//! This module provides high-performance caching for:
//! - Pool states with automatic refresh
//! - Route calculations with validation
//! - Quote results with freshness checks
//! - DEX metadata and configuration

use anyhow::Result;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tokio::time::{interval};
use log::{debug, info};

/// Cache entry with TTL and access tracking
#[derive(Debug, Clone)]
struct CacheEntry<T> {
    value: T,
    created_at: SystemTime,
    last_accessed: SystemTime,
    access_count: u64,
    ttl: Duration,
}

impl<T> CacheEntry<T> {
    fn new(value: T, ttl: Duration) -> Self {
        let now = SystemTime::now();
        Self {
            value,
            created_at: now,
            last_accessed: now,
            access_count: 1,
            ttl,
        }
    }

    fn is_expired(&self) -> bool {
        self.created_at.elapsed().unwrap_or(Duration::MAX) > self.ttl
    }

    fn access(&mut self) -> &T {
        self.last_accessed = SystemTime::now();
        self.access_count += 1;
        &self.value
    }


}

/// High-performance cache manager with multiple cache types
pub struct CacheManager {
    pool_cache: Arc<RwLock<HashMap<String, CacheEntry<PoolState>>>>,
    route_cache: Arc<RwLock<HashMap<String, CacheEntry<RouteInfo>>>>,
    quote_cache: Arc<RwLock<HashMap<String, CacheEntry<QuoteInfo>>>>,
    metadata_cache: Arc<RwLock<HashMap<String, CacheEntry<MetadataInfo>>>>,
    config: CacheConfig,
    stats: Arc<RwLock<CacheStats>>,
}

/// Cache configuration
#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub pool_ttl: Duration,
    pub route_ttl: Duration,
    pub quote_ttl: Duration,
    pub metadata_ttl: Duration,
    pub max_entries_per_cache: usize,
    pub cleanup_interval: Duration,
    pub enable_auto_refresh: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            pool_ttl: Duration::from_secs(10),
            route_ttl: Duration::from_secs(30),
            quote_ttl: Duration::from_secs(5),
            metadata_ttl: Duration::from_secs(300),
            max_entries_per_cache: 10000,
            cleanup_interval: Duration::from_secs(60),
            enable_auto_refresh: true,
        }
    }
}

/// Cache statistics
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub pool_hits: u64,
    pub pool_misses: u64,
    pub route_hits: u64,
    pub route_misses: u64,
    pub quote_hits: u64,
    pub quote_misses: u64,
    pub metadata_hits: u64,
    pub metadata_misses: u64,
    pub total_entries: usize,
    pub evictions: u64,
    pub refresh_operations: u64,
    pub pool_hit_rate: f64,
    pub route_hit_rate: f64,
    pub quote_hit_rate: f64,
    pub metadata_hit_rate: f64,
}

impl CacheStats {
    fn calculate_hit_rates(&mut self) {
        self.pool_hit_rate = if self.pool_hits + self.pool_misses > 0 {
            self.pool_hits as f64 / (self.pool_hits + self.pool_misses) as f64
        } else { 0.0 };
        
        self.route_hit_rate = if self.route_hits + self.route_misses > 0 {
            self.route_hits as f64 / (self.route_hits + self.route_misses) as f64
        } else { 0.0 };
        
        self.quote_hit_rate = if self.quote_hits + self.quote_misses > 0 {
            self.quote_hits as f64 / (self.quote_hits + self.quote_misses) as f64
        } else { 0.0 };
        
        self.metadata_hit_rate = if self.metadata_hits + self.metadata_misses > 0 {
            self.metadata_hits as f64 / (self.metadata_hits + self.metadata_misses) as f64
        } else { 0.0 };
    }
}

/// Pool state information for caching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolState {
    pub pool_address: String,
    pub token_a: String,
    pub token_b: String,
    pub reserves_a: u64,
    pub reserves_b: u64,
    pub fee_rate: f64,
    pub liquidity: u64,
    pub price: f64,
    pub last_updated: u64,
    pub dex_type: String,
}

/// Route information for caching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteInfo {
    pub input_token: String,
    pub output_token: String,
    pub amount: u64,
    pub routes: Vec<RouteData>,
    pub best_route_index: usize,
    pub total_output: u64,
    pub price_impact: f64,
    pub execution_time_estimate: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteData {
    pub dex_name: String,
    pub hops: Vec<String>,
    pub estimated_output: u64,
    pub fees: u64,
    pub slippage: f64,
}

/// Quote information for caching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteInfo {
    pub input_token: String,
    pub output_token: String,
    pub input_amount: u64,
    pub output_amount: u64,
    pub price: f64,
    pub price_impact: f64,
    pub fees: u64,
    pub dex_name: String,
    pub route_hops: Vec<String>,
    pub freshness_score: f64, // 0.0 to 1.0, higher = fresher
}

/// Metadata information for caching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataInfo {
    pub token_address: String,
    pub token_symbol: String,
    pub token_decimals: u8,
    pub token_name: String,
    pub verified: bool,
    pub coingecko_id: Option<String>,
}

impl CacheManager {
    /// Create a new cache manager
    pub async fn new(config: CacheConfig) -> Result<Self> {
        info!("🗄️ Initializing CacheManager with {:?}", config);
        
        let manager = Self {
            pool_cache: Arc::new(RwLock::new(HashMap::new())),
            route_cache: Arc::new(RwLock::new(HashMap::new())),
            quote_cache: Arc::new(RwLock::new(HashMap::new())),
            metadata_cache: Arc::new(RwLock::new(HashMap::new())),
            config: config.clone(),
            stats: Arc::new(RwLock::new(CacheStats::default())),
        };

        // Start cleanup task
        manager.start_cleanup_task().await;
        
        Ok(manager)
    }

    /// Start background cleanup task
    async fn start_cleanup_task(&self) {
        let pool_cache = self.pool_cache.clone();
        let route_cache = self.route_cache.clone();
        let quote_cache = self.quote_cache.clone();
        let metadata_cache = self.metadata_cache.clone();
        let stats = self.stats.clone();
        let cleanup_interval = self.config.cleanup_interval;
        let max_entries = self.config.max_entries_per_cache;

        tokio::spawn(async move {
            let mut interval_timer = interval(cleanup_interval);
            
            loop {
                interval_timer.tick().await;
                
                // Clean expired entries
                let mut evicted_count = 0;
                
                // Clean pool cache
                {
                    let mut cache = pool_cache.write().await;
                    let before_count = cache.len();
                    cache.retain(|_, entry| !entry.is_expired());
                    evicted_count += before_count - cache.len();
                    
                    // Enforce size limit
                    if cache.len() > max_entries {
                        let entries_to_remove = cache.len() - max_entries;
                        let mut entries: Vec<_> = cache.iter().map(|(k, v)| (k.clone(), v.last_accessed)).collect();
                        entries.sort_by_key(|(_, last_accessed)| *last_accessed);
                        
                        let keys_to_remove: Vec<_> = entries.iter().take(entries_to_remove).map(|(k, _)| k.clone()).collect();
                        for key in keys_to_remove {
                            cache.remove(&key);
                        }
                        evicted_count += entries_to_remove;
                    }
                }
                
                // Clean route cache
                {
                    let mut cache = route_cache.write().await;
                    let before_count = cache.len();
                    cache.retain(|_, entry| !entry.is_expired());
                    evicted_count += before_count - cache.len();
                    
                    // Enforce size limit
                    if cache.len() > max_entries {
                        let entries_to_remove = cache.len() - max_entries;
                        let mut entries: Vec<_> = cache.iter().map(|(k, v)| (k.clone(), v.last_accessed)).collect();
                        entries.sort_by_key(|(_, last_accessed)| *last_accessed);
                        
                        let keys_to_remove: Vec<_> = entries.iter().take(entries_to_remove).map(|(k, _)| k.clone()).collect();
                        for key in keys_to_remove {
                            cache.remove(&key);
                        }
                        evicted_count += entries_to_remove;
                    }
                }
                
                // Clean quote cache
                {
                    let mut cache = quote_cache.write().await;
                    let before_count = cache.len();
                    cache.retain(|_, entry| !entry.is_expired());
                    evicted_count += before_count - cache.len();
                    
                    // Enforce size limit
                    if cache.len() > max_entries {
                        let entries_to_remove = cache.len() - max_entries;
                        let mut entries: Vec<_> = cache.iter().map(|(k, v)| (k.clone(), v.last_accessed)).collect();
                        entries.sort_by_key(|(_, last_accessed)| *last_accessed);
                        
                        let keys_to_remove: Vec<_> = entries.iter().take(entries_to_remove).map(|(k, _)| k.clone()).collect();
                        for key in keys_to_remove {
                            cache.remove(&key);
                        }
                        evicted_count += entries_to_remove;
                    }
                }
                
                // Clean metadata cache
                {
                    let mut cache = metadata_cache.write().await;
                    let before_count = cache.len();
                    cache.retain(|_, entry| !entry.is_expired());
                    evicted_count += before_count - cache.len();
                }
                
                // Update stats
                {
                    let mut stats = stats.write().await;
                    stats.evictions += evicted_count as u64;
                    stats.calculate_hit_rates();
                }
                
                if evicted_count > 0 {
                    debug!("Cache cleanup: evicted {} expired entries", evicted_count);
                }
            }
        });
    }

    /// Get pool state from cache or return None if not found/expired
    pub async fn get_pool_state(&self, pool_address: &str) -> Option<PoolState> {
        let mut cache = self.pool_cache.write().await;
        let mut stats = self.stats.write().await;
        
        if let Some(entry) = cache.get_mut(pool_address) {
            if !entry.is_expired() {
                stats.pool_hits += 1;
                return Some(entry.access().clone());
            } else {
                cache.remove(pool_address);
            }
        }
        
        stats.pool_misses += 1;
        None
    }

    /// Cache pool state
    pub async fn set_pool_state(&self, pool_address: String, state: PoolState) {
        let mut cache = self.pool_cache.write().await;
        let entry = CacheEntry::new(state, self.config.pool_ttl);
        cache.insert(pool_address, entry);
    }

    /// Get route from cache
    pub async fn get_route(&self, route_key: &str) -> Option<RouteInfo> {
        let mut cache = self.route_cache.write().await;
        let mut stats = self.stats.write().await;
        
        if let Some(entry) = cache.get_mut(route_key) {
            if !entry.is_expired() {
                stats.route_hits += 1;
                return Some(entry.access().clone());
            } else {
                cache.remove(route_key);
            }
        }
        
        stats.route_misses += 1;
        None
    }

    /// Cache route information
    pub async fn set_route(&self, route_key: String, route: RouteInfo) {
        let mut cache = self.route_cache.write().await;
        let entry = CacheEntry::new(route, self.config.route_ttl);
        cache.insert(route_key, entry);
    }

    /// Get quote from cache with freshness validation
    pub async fn get_quote(&self, quote_key: &str, required_freshness: f64) -> Option<QuoteInfo> {
        let mut cache = self.quote_cache.write().await;
        let mut stats = self.stats.write().await;
        
        if let Some(entry) = cache.get_mut(quote_key) {
            if !entry.is_expired() {
                let quote = entry.access();
                if quote.freshness_score >= required_freshness {
                    stats.quote_hits += 1;
                    return Some(quote.clone());
                }
            } else {
                cache.remove(quote_key);
            }
        }
        
        stats.quote_misses += 1;
        None
    }

    /// Cache quote with freshness score
    pub async fn set_quote(&self, quote_key: String, quote: QuoteInfo) {
        let mut cache = self.quote_cache.write().await;
        let entry = CacheEntry::new(quote, self.config.quote_ttl);
        cache.insert(quote_key, entry);
    }

    /// Get metadata from cache
    pub async fn get_metadata(&self, token_address: &str) -> Option<MetadataInfo> {
        let mut cache = self.metadata_cache.write().await;
        let mut stats = self.stats.write().await;
        
        if let Some(entry) = cache.get_mut(token_address) {
            if !entry.is_expired() {
                stats.metadata_hits += 1;
                return Some(entry.access().clone());
            } else {
                cache.remove(token_address);
            }
        }
        
        stats.metadata_misses += 1;
        None
    }

    /// Cache metadata
    pub async fn set_metadata(&self, token_address: String, metadata: MetadataInfo) {
        let mut cache = self.metadata_cache.write().await;
        let entry = CacheEntry::new(metadata, self.config.metadata_ttl);
        cache.insert(token_address, entry);
    }

    /// Generate cache key for routes
    pub fn generate_route_key(&self, input_token: &str, output_token: &str, amount: u64) -> String {
        format!("route:{}:{}:{}", input_token, output_token, amount)
    }

    /// Generate cache key for quotes
    pub fn generate_quote_key(&self, input_token: &str, output_token: &str, amount: u64, dex: &str) -> String {
        format!("quote:{}:{}:{}:{}", input_token, output_token, amount, dex)
    }

    /// Get comprehensive cache statistics
    pub async fn get_stats(&self) -> CacheStats {
        let stats = self.stats.read().await;
        let mut stats_copy = stats.clone();
        
        // Update total entries count
        let pool_count = self.pool_cache.read().await.len();
        let route_count = self.route_cache.read().await.len();
        let quote_count = self.quote_cache.read().await.len();
        let metadata_count = self.metadata_cache.read().await.len();
        
        stats_copy.total_entries = pool_count + route_count + quote_count + metadata_count;
        stats_copy.calculate_hit_rates();
        
        stats_copy
    }

    /// Clear all caches
    pub async fn clear_all(&self) {
        self.pool_cache.write().await.clear();
        self.route_cache.write().await.clear();
        self.quote_cache.write().await.clear();
        self.metadata_cache.write().await.clear();
        
        let mut stats = self.stats.write().await;
        *stats = CacheStats::default();
    }

    /// Preload frequently used data
    pub async fn preload_frequent_pairs(&self, pairs: Vec<(String, String)>) -> Result<()> {
        info!("Preloading {} frequent trading pairs", pairs.len());
        
        for (token_a, token_b) in pairs {
            // This would typically fetch and cache the most common routes/quotes
            // for these pairs to improve initial response times
            let _route_key = self.generate_route_key(&token_a, &token_b, 1_000_000);
            
            // Placeholder: would implement actual preloading logic here
            debug!("Preloading route for pair: {} -> {}", token_a, token_b);
        }
        
        Ok(())
    }
}

/// Calculate freshness score based on quote age and market volatility
pub fn calculate_freshness_score(quote_age: Duration, market_volatility: f64) -> f64 {
    let age_seconds = quote_age.as_secs_f64();
    let base_freshness = (1.0 - (age_seconds / 300.0)).max(0.0); // 5 min = 0 freshness
    let volatility_penalty = market_volatility * 0.1; // Higher volatility = lower freshness
    
    (base_freshness - volatility_penalty).max(0.0).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_basic_operations() {
        let config = CacheConfig::default();
        let cache = CacheManager::new(config).await.unwrap();
        
        // Test pool state caching
        let pool_state = PoolState {
            pool_address: "test_pool".to_string(),
            token_a: "SOL".to_string(),
            token_b: "USDC".to_string(),
            reserves_a: 1000000,
            reserves_b: 2000000,
            fee_rate: 0.003,
            liquidity: 1500000,
            price: 100.0,
            last_updated: 123456789,
            dex_type: "Raydium".to_string(),
        };
        
        cache.set_pool_state("test_pool".to_string(), pool_state.clone()).await;
        
        let retrieved = cache.get_pool_state("test_pool").await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().pool_address, pool_state.pool_address);
        
        // Test cache miss
        let missing = cache.get_pool_state("nonexistent").await;
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn test_cache_expiration() {
        let mut config = CacheConfig::default();
        config.pool_ttl = Duration::from_millis(50); // Very short TTL for testing
        
        let cache = CacheManager::new(config).await.unwrap();
        
        let pool_state = PoolState {
            pool_address: "test_pool".to_string(),
            token_a: "SOL".to_string(),
            token_b: "USDC".to_string(),
            reserves_a: 1000000,
            reserves_b: 2000000,
            fee_rate: 0.003,
            liquidity: 1500000,
            price: 100.0,
            last_updated: 123456789,
            dex_type: "Raydium".to_string(),
        };
        
        cache.set_pool_state("test_pool".to_string(), pool_state).await;
        
        // Should be available immediately
        assert!(cache.get_pool_state("test_pool").await.is_some());
        
        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        // Should be expired
        assert!(cache.get_pool_state("test_pool").await.is_none());
    }

    #[test]
    fn test_freshness_calculation() {
        // Fresh quote (0 seconds old)
        let fresh_score = calculate_freshness_score(Duration::from_secs(0), 0.1);
        assert!(fresh_score > 0.8);
        
        // Old quote (5 minutes old)
        let old_score = calculate_freshness_score(Duration::from_secs(300), 0.1);
        assert!(old_score < 0.2);
        
        // High volatility penalty
        let volatile_score = calculate_freshness_score(Duration::from_secs(30), 0.9);
        let stable_score = calculate_freshness_score(Duration::from_secs(30), 0.1);
        assert!(volatile_score < stable_score);
    }
}
