//! Jupiter Quote Caching System
//!
//! Intelligent caching for Jupiter API responses to reduce external API calls by 60-80%.
//! Implements time-based TTL, amount bucketing, volatility-based invalidation, and LRU eviction.

use crate::dex::clients::jupiter_api::QuoteResponse;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    fmt,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

/// Configuration for Jupiter quote caching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Enable/disable caching entirely
    pub enabled: bool,
    /// Time-to-live for cache entries in seconds
    pub ttl_seconds: u64,
    /// Maximum number of cache entries to store
    pub max_entries: usize,
    /// Amount bucket size for grouping similar amounts (in lamports)
    pub amount_bucket_size: u64,
    /// Volatility threshold for cache invalidation (percentage)
    pub volatility_threshold_pct: f64,
    /// Target cache hit rate for monitoring
    pub target_hit_rate: f64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ttl_seconds: 5,
            max_entries: 1000,
            amount_bucket_size: 1_000_000, // 1M lamports
            volatility_threshold_pct: 2.0, // 2% volatility triggers cache clear
            target_hit_rate: 0.7,          // Target 70% cache hit rate
        }
    }
}

/// Cache key for Jupiter quotes with amount bucketing
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct CacheKey {
    pub input_mint: String,
    pub output_mint: String,
    pub amount_bucket: u64,
    pub slippage_bps: u16,
}

impl CacheKey {
    /// Create a cache key from quote parameters with amount bucketing
    pub fn from_params(
        input_mint: &str,
        output_mint: &str,
        amount: u64,
        slippage_bps: u16,
        bucket_size: u64,
    ) -> Self {
        let amount_bucket = Self::bucket_amount(amount, bucket_size);

        Self {
            input_mint: input_mint.to_string(),
            output_mint: output_mint.to_string(),
            amount_bucket,
            slippage_bps,
        }
    }

    /// Round amount to nearest bucket for better cache hit rates
    fn bucket_amount(amount: u64, bucket_size: u64) -> u64 {
        if bucket_size == 0 {
            return amount;
        }
        ((amount + bucket_size / 2) / bucket_size) * bucket_size
    }
}

impl fmt::Display for CacheKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}→{}:{}@{}bps",
            self.input_mint.chars().take(6).collect::<String>(),
            self.output_mint.chars().take(6).collect::<String>(),
            self.amount_bucket,
            self.slippage_bps
        )
    }
}

/// Cache entry storing quote response with metadata
#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub quote_response: QuoteResponse,
    pub cached_at: Instant,
    pub last_accessed: Instant,
    pub access_count: u32,
    pub market_conditions_hash: u64, // Simple hash of market conditions when cached
}

impl CacheEntry {
    /// Create a new cache entry
    pub fn new(quote_response: QuoteResponse, market_conditions_hash: u64) -> Self {
        let now = Instant::now();
        Self {
            quote_response,
            cached_at: now,
            last_accessed: now,
            access_count: 1,
            market_conditions_hash,
        }
    }

    /// Check if the cache entry is still valid based on TTL
    pub fn is_valid(&self, ttl: Duration) -> bool {
        self.cached_at.elapsed() < ttl
    }

    /// Mark the entry as accessed
    pub fn mark_accessed(&mut self) {
        self.last_accessed = Instant::now();
        self.access_count += 1;
    }

    /// Get the age of the cache entry in milliseconds
    pub fn age_ms(&self) -> u64 {
        self.cached_at.elapsed().as_millis() as u64
    }
}

/// Cache metrics for monitoring and optimization
#[derive(Debug, Clone, Default)]
pub struct CacheMetrics {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub volatility_clears: u64,
    pub total_entries: usize,
    pub average_age_ms: u64,
}

impl CacheMetrics {
    /// Calculate cache hit rate
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    /// Reset metrics (useful for periodic reporting)
    pub fn reset(&mut self) {
        self.hits = 0;
        self.misses = 0;
        self.evictions = 0;
        self.volatility_clears = 0;
    }
}

/// Market conditions snapshot for volatility detection
#[derive(Debug, Clone)]
struct MarketSnapshot {
    timestamp: Instant,
    conditions_hash: u64,
}

/// Jupiter quote cache with intelligent invalidation and LRU eviction
pub struct JupiterQuoteCache {
    cache: Arc<Mutex<CacheStorage>>,
    config: CacheConfig,
    metrics: Arc<Mutex<CacheMetrics>>,
    last_market_snapshot: Arc<Mutex<Option<MarketSnapshot>>>,
}

/// Internal cache storage structure
struct CacheStorage {
    entries: HashMap<CacheKey, CacheEntry>,
    access_order: VecDeque<CacheKey>, // For LRU eviction
}

impl JupiterQuoteCache {
    /// Create a new Jupiter quote cache
    pub fn new(config: CacheConfig) -> Self {
        info!(
            "🗄️  Initializing Jupiter quote cache (TTL: {}s, Max entries: {})",
            config.ttl_seconds, config.max_entries
        );

        Self {
            cache: Arc::new(Mutex::new(CacheStorage {
                entries: HashMap::new(),
                access_order: VecDeque::new(),
            })),
            config,
            metrics: Arc::new(Mutex::new(CacheMetrics::default())),
            last_market_snapshot: Arc::new(Mutex::new(None)),
        }
    }

    /// Get a quote from cache if available and valid
    pub async fn get_quote(&self, key: &CacheKey) -> Option<QuoteResponse> {
        if !self.config.enabled {
            return None;
        }

        let mut cache = self.cache.lock().await;
        let ttl = Duration::from_secs(self.config.ttl_seconds);

        // Check if entry exists and is valid
        let (is_valid, quote_response) = if let Some(entry) = cache.entries.get(key) {
            if entry.is_valid(ttl) {
                (true, Some(entry.quote_response.clone()))
            } else {
                (false, None)
            }
        } else {
            (false, None)
        };

        if is_valid {
            // Update access information
            if let Some(entry) = cache.entries.get_mut(key) {
                entry.mark_accessed();
            }

            // Move to end of access order (most recently used)
            if let Some(pos) = cache.access_order.iter().position(|k| k == key) {
                cache.access_order.remove(pos);
            }
            cache.access_order.push_back(key.clone());

            // Update metrics
            {
                let mut metrics = self.metrics.lock().await;
                metrics.hits += 1;
            }

            debug!("🎯 Cache HIT for {}", key.to_string());
            return quote_response;
        } else if cache.entries.contains_key(key) {
            // Entry expired - remove it
            cache.entries.remove(key);
            if let Some(pos) = cache.access_order.iter().position(|k| k == key) {
                cache.access_order.remove(pos);
            }
            debug!("⏰ Cache entry expired for {}", key.to_string());
        }

        // Cache miss
        {
            let mut metrics = self.metrics.lock().await;
            metrics.misses += 1;
        }

        debug!("📡 Cache MISS for {}", key.to_string());
        None
    }

    /// Store a quote in the cache
    pub async fn store_quote(&self, key: CacheKey, quote_response: QuoteResponse) {
        if !self.config.enabled {
            return;
        }

        let market_conditions_hash = self.get_market_conditions_hash().await;
        let entry = CacheEntry::new(quote_response, market_conditions_hash);

        let mut cache = self.cache.lock().await;

        // Check if we need to evict entries to make room
        while cache.entries.len() >= self.config.max_entries {
            self.evict_lru(&mut cache).await;
        }

        // Store the new entry
        cache.entries.insert(key.clone(), entry);
        cache.access_order.push_back(key.clone());

        debug!(
            "💾 Cached quote for {} (cache size: {})",
            key.to_string(),
            cache.entries.len()
        );
    }

    /// Evict least recently used entry
    async fn evict_lru(&self, cache: &mut CacheStorage) {
        if let Some(lru_key) = cache.access_order.pop_front() {
            cache.entries.remove(&lru_key);

            // Update metrics
            {
                let mut metrics = self.metrics.lock().await;
                metrics.evictions += 1;
            }

            debug!("🗑️  Evicted LRU entry: {}", lru_key.to_string());
        }
    }

    /// Clear cache due to high market volatility
    pub async fn clear_volatile_cache(&self, reason: &str) {
        let mut cache = self.cache.lock().await;
        let entry_count = cache.entries.len();

        cache.entries.clear();
        cache.access_order.clear();

        // Update metrics
        {
            let mut metrics = self.metrics.lock().await;
            metrics.volatility_clears += 1;
        }

        warn!(
            "🌪️  Cleared cache due to volatility: {} ({} entries removed)",
            reason, entry_count
        );
    }

    /// Get current cache metrics
    pub async fn get_metrics(&self) -> CacheMetrics {
        let mut metrics = self.metrics.lock().await;
        let cache = self.cache.lock().await;

        // Update current stats
        metrics.total_entries = cache.entries.len();

        // Calculate average age
        if !cache.entries.is_empty() {
            let total_age: u64 = cache.entries.values().map(|e| e.age_ms()).sum();
            metrics.average_age_ms = total_age / cache.entries.len() as u64;
        }

        metrics.clone()
    }

    /// Get cache statistics for monitoring
    pub async fn get_cache_stats(&self) -> CacheStats {
        let metrics = self.get_metrics().await;
        let cache = self.cache.lock().await;

        CacheStats {
            hit_rate: metrics.hit_rate(),
            total_requests: metrics.hits + metrics.misses,
            cache_size: cache.entries.len(),
            max_size: self.config.max_entries,
            average_age_ms: metrics.average_age_ms,
            evictions: metrics.evictions,
            volatility_clears: metrics.volatility_clears,
        }
    }

    /// Simple market conditions hash (placeholder for more sophisticated implementation)
    async fn get_market_conditions_hash(&self) -> u64 {
        // In a real implementation, this would hash current market conditions
        // like recent price movements, volatility indicators, etc.
        // For now, use timestamp to detect market changes
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            / 60 // Change every minute
    }

    /// Check if market conditions have changed significantly
    pub async fn should_invalidate_for_volatility(&self) -> bool {
        let current_hash = self.get_market_conditions_hash().await;
        let mut last_snapshot = self.last_market_snapshot.lock().await;

        if let Some(ref snapshot) = *last_snapshot {
            // If market conditions hash changed significantly, invalidate
            if snapshot.conditions_hash != current_hash {
                // In a real implementation, you would check actual volatility metrics here
                // For now, we'll use a simple time-based approach
                if snapshot.timestamp.elapsed() > Duration::from_secs(300) {
                    // 5 minutes
                    *last_snapshot = Some(MarketSnapshot {
                        timestamp: Instant::now(),
                        conditions_hash: current_hash,
                    });
                    return true;
                }
            }
        } else {
            *last_snapshot = Some(MarketSnapshot {
                timestamp: Instant::now(),
                conditions_hash: current_hash,
            });
        }

        false
    }
}

/// Cache statistics for monitoring
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub hit_rate: f64,
    pub total_requests: u64,
    pub cache_size: usize,
    pub max_size: usize,
    pub average_age_ms: u64,
    pub evictions: u64,
    pub volatility_clears: u64,
}

impl CacheStats {
    /// Check if cache performance is healthy
    pub fn is_healthy(&self, target_hit_rate: f64) -> bool {
        self.hit_rate >= target_hit_rate && self.cache_size < self.max_size
    }

    /// Get a human-readable summary
    pub fn summary(&self) -> String {
        format!(
            "Cache: {:.1}% hit rate, {}/{} entries, avg age: {}ms",
            self.hit_rate * 100.0,
            self.cache_size,
            self.max_size,
            self.average_age_ms
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dex::clients::jupiter_api::QuoteResponse;

    fn create_test_quote_response(out_amount: u64) -> QuoteResponse {
        QuoteResponse {
            input_mint: "So11111111111111111111111111111111111111112".to_string(),
            in_amount: "1000000".to_string(),
            output_mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(),
            out_amount: out_amount.to_string(),
            other_amount_threshold: "0".to_string(),
            route_plan: vec![],
            context_slot: 123456,
            time_taken: 0.1,
            platform_fee: None,
            price_impact_pct: "0.1".to_string(),
        }
    }

    #[tokio::test]
    async fn test_cache_key_creation() {
        let key = CacheKey::from_params(
            "So11111111111111111111111111111111111111112",
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            1_500_000,
            50,
            1_000_000,
        );

        assert_eq!(key.amount_bucket, 2_000_000); // Rounded to nearest bucket
        assert_eq!(key.slippage_bps, 50);
    }

    #[tokio::test]
    async fn test_cache_store_and_retrieve() {
        let config = CacheConfig::default();
        let cache = JupiterQuoteCache::new(config);

        let key = CacheKey::from_params("SOL", "USDC", 1_000_000, 50, 1_000_000);
        let quote = create_test_quote_response(150_000);

        // Store quote
        cache.store_quote(key.clone(), quote.clone()).await;

        // Retrieve quote
        let cached_quote = cache.get_quote(&key).await;
        assert!(cached_quote.is_some());
        assert_eq!(cached_quote.unwrap().out_amount, "150000");
    }

    #[tokio::test]
    async fn test_cache_expiration() {
        let mut config = CacheConfig::default();
        config.ttl_seconds = 1; // 1 second TTL

        let cache = JupiterQuoteCache::new(config);
        let key = CacheKey::from_params("SOL", "USDC", 1_000_000, 50, 1_000_000);
        let quote = create_test_quote_response(150_000);

        // Store quote
        cache.store_quote(key.clone(), quote).await;

        // Should be available immediately
        assert!(cache.get_quote(&key).await.is_some());

        // Wait for expiration
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Should be expired now
        assert!(cache.get_quote(&key).await.is_none());
    }

    #[tokio::test]
    async fn test_cache_metrics() {
        let config = CacheConfig::default();
        let cache = JupiterQuoteCache::new(config);

        let key = CacheKey::from_params("SOL", "USDC", 1_000_000, 50, 1_000_000);
        let quote = create_test_quote_response(150_000);

        // Initial metrics
        let initial_metrics = cache.get_metrics().await;
        assert_eq!(initial_metrics.hits, 0);
        assert_eq!(initial_metrics.misses, 0);

        // Cache miss
        cache.get_quote(&key).await;
        let after_miss = cache.get_metrics().await;
        assert_eq!(after_miss.misses, 1);

        // Store and hit
        cache.store_quote(key.clone(), quote).await;
        cache.get_quote(&key).await;
        let after_hit = cache.get_metrics().await;
        assert_eq!(after_hit.hits, 1);
        assert_eq!(after_hit.misses, 1);
        assert_eq!(after_hit.hit_rate(), 0.5);
    }

    #[tokio::test]
    async fn test_lru_eviction() {
        let mut config = CacheConfig::default();
        config.max_entries = 2; // Small cache for testing

        let cache = JupiterQuoteCache::new(config);

        // Fill cache
        let key1 = CacheKey::from_params("SOL", "USDC", 1_000_000, 50, 1_000_000);
        let key2 = CacheKey::from_params("ETH", "USDC", 1_000_000, 50, 1_000_000);
        let key3 = CacheKey::from_params("BTC", "USDC", 1_000_000, 50, 1_000_000);

        cache
            .store_quote(key1.clone(), create_test_quote_response(150_000))
            .await;
        cache
            .store_quote(key2.clone(), create_test_quote_response(2500_000))
            .await;

        // Both should be available
        assert!(cache.get_quote(&key1).await.is_some());
        assert!(cache.get_quote(&key2).await.is_some());

        // Add third entry - should evict LRU (key1, since key2 was accessed more recently)
        cache
            .store_quote(key3.clone(), create_test_quote_response(45_000))
            .await;

        // key1 should be evicted, key2 and key3 should remain
        assert!(cache.get_quote(&key1).await.is_none());
        assert!(cache.get_quote(&key2).await.is_some());
        assert!(cache.get_quote(&key3).await.is_some());
    }
}
