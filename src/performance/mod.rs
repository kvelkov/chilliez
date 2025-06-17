// src/performance/mod.rs
//! Performance Optimization Module
//!
//! This module provides high-performance parallel processing, caching, and
//! optimization strategies for the Solana DEX arbitrage bot.

pub mod benchmark;
pub mod cache;
pub mod metrics;
pub mod parallel;

pub use benchmark::*;
pub use cache::*;
pub use metrics::*;
pub use parallel::{ParallelConfig, ParallelExecutor, ParallelStats};

use anyhow::Result;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Performance configuration for the entire system
#[derive(Debug, Clone)]
pub struct PerformanceConfig {
    /// Maximum number of concurrent workers
    pub max_concurrent_workers: usize,
    /// Timeout for individual operations
    pub operation_timeout: Duration,
    /// Cache TTL for pool states
    pub pool_cache_ttl: Duration,
    /// Cache TTL for routes
    pub route_cache_ttl: Duration,
    /// Cache TTL for quotes
    pub quote_cache_ttl: Duration,
    /// Maximum cache size (number of entries)
    pub max_cache_size: usize,
    /// Enable performance metrics collection
    pub metrics_enabled: bool,
    /// Benchmark interval
    pub benchmark_interval: Duration,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            max_concurrent_workers: num_cpus::get().max(4),
            operation_timeout: Duration::from_secs(30),
            pool_cache_ttl: Duration::from_secs(10), // 10s for pool states
            route_cache_ttl: Duration::from_secs(30), // 30s for routes
            quote_cache_ttl: Duration::from_secs(5), // 5s for quotes
            max_cache_size: 10000,
            metrics_enabled: true,
            benchmark_interval: Duration::from_secs(60),
        }
    }
}

/// Performance manager that coordinates all optimization systems
pub struct PerformanceManager {
    config: PerformanceConfig,
    parallel_executor: Arc<ParallelExecutor>,
    cache_manager: Arc<CacheManager>,
    metrics_collector: Arc<RwLock<MetricsCollector>>,
    benchmark_runner: Arc<BenchmarkRunner>,
}

impl PerformanceManager {
    /// Create a new performance manager
    pub async fn new(config: PerformanceConfig) -> Result<Self> {
        let parallel_executor =
            Arc::new(ParallelExecutor::new(config.max_concurrent_workers).await?);

        // Convert PerformanceConfig to CacheConfig
        let cache_config = CacheConfig {
            pool_ttl: config.pool_cache_ttl,
            route_ttl: config.route_cache_ttl,
            quote_ttl: config.quote_cache_ttl,
            metadata_ttl: Duration::from_secs(300), // Default for metadata
            max_entries_per_cache: config.max_cache_size,
            cleanup_interval: Duration::from_secs(60), // Default
            enable_auto_refresh: true,                 // Default
        };

        let cache_manager = Arc::new(CacheManager::new(cache_config).await?);
        let metrics_collector = Arc::new(RwLock::new(MetricsCollector::new()));
        let benchmark_runner = Arc::new(BenchmarkRunner::new(config.clone()));

        Ok(Self {
            config,
            parallel_executor,
            cache_manager,
            metrics_collector,
            benchmark_runner,
        })
    }

    /// Get the parallel executor
    pub fn parallel_executor(&self) -> Arc<ParallelExecutor> {
        self.parallel_executor.clone()
    }

    /// Get the cache manager
    pub fn cache_manager(&self) -> Arc<CacheManager> {
        self.cache_manager.clone()
    }

    /// Get the metrics collector
    pub fn metrics_collector(&self) -> Arc<RwLock<MetricsCollector>> {
        self.metrics_collector.clone()
    }

    /// Get the benchmark runner
    pub fn benchmark_runner(&self) -> Arc<BenchmarkRunner> {
        self.benchmark_runner.clone()
    }

    /// Start background performance monitoring
    pub async fn start_monitoring(&self) -> Result<()> {
        if self.config.metrics_enabled {
            let metrics = self.metrics_collector.clone();
            let benchmark = self.benchmark_runner.clone();
            let interval = self.config.benchmark_interval;

            tokio::spawn(async move {
                let mut interval_timer = tokio::time::interval(interval);
                loop {
                    interval_timer.tick().await;

                    if let Err(e) = benchmark.run_system_benchmark().await {
                        log::warn!("Benchmark failed: {}", e);
                    }

                    let mut collector = metrics.write().await;
                    collector.record_system_stats().await;
                }
            });
        }

        Ok(())
    }

    /// Get comprehensive performance report
    pub async fn get_performance_report(&self) -> PerformanceReport {
        let metrics = self.metrics_collector.read().await;
        let cache_stats = self.cache_manager.get_stats().await;
        let parallel_stats = self.parallel_executor.get_stats().await;

        PerformanceReport {
            timestamp: Instant::now(),
            metrics: metrics.get_summary(),
            cache_stats,
            parallel_stats,
            config: self.config.clone(),
        }
    }
}

/// Comprehensive performance report
#[derive(Debug, Clone)]
pub struct PerformanceReport {
    pub timestamp: Instant,
    pub metrics: MetricsSummary,
    pub cache_stats: CacheStats,
    pub parallel_stats: ParallelStats,
    pub config: PerformanceConfig,
}

impl PerformanceReport {
    /// Generate a human-readable performance summary
    pub fn summary(&self) -> String {
        format!(
            "Performance Report:\n\
             ==================\n\
             Timestamp: {:?}\n\
             \n\
             Parallel Processing:\n\
             - Active Workers: {}\n\
             - Completed Tasks: {}\n\
             - Average Task Time: {:.2}ms\n\
             \n\
             Cache Performance:\n\
             - Pool Cache Hit Rate: {:.1}%\n\
             - Route Cache Hit Rate: {:.1}%\n\
             - Quote Cache Hit Rate: {:.1}%\n\
             - Total Cache Size: {} entries\n\
             \n\
             System Metrics:\n\
             - CPU Usage: {:.1}%\n\
             - Memory Usage: {:.1}MB\n\
             - Network Latency: {:.1}ms\n\
             - Throughput: {:.1} ops/sec",
            self.timestamp,
            self.parallel_stats.active_workers,
            self.parallel_stats.completed_tasks,
            self.parallel_stats.avg_task_duration.as_millis() as f64,
            self.cache_stats.pool_hit_rate * 100.0,
            self.cache_stats.route_hit_rate * 100.0,
            self.cache_stats.quote_hit_rate * 100.0,
            self.cache_stats.total_entries,
            self.metrics.cpu_usage,
            self.metrics.memory_usage_mb,
            self.metrics.network_latency_ms,
            self.metrics.throughput_ops_per_sec
        )
    }
}
