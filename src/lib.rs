pub mod jito_bundle;
pub mod api; // NEW: Production API management (rate limiting, connection pooling, failover)
pub mod arbitrage;
pub mod cache;
pub mod config;
pub mod dex;
pub mod error;
pub mod ffi; // FFI exports for JavaScript bridge
pub mod local_metrics;
pub mod monitoring; // Enhanced monitoring and alerting
pub mod paper_trading; // Paper trading simulation system
pub mod performance; // NEW: Performance optimization and monitoring
pub mod solana;
pub mod streams;
pub mod testing; // Testing infrastructure
pub mod utils;
pub mod wallet; // NEW: Wallet management features
pub mod webhooks;
pub mod websocket; // NEW: Real-time data streams and processing

// Re-export key testing components for easy access
pub use testing::{MarketCondition, MockDexEnvironment, TestSuiteRunner};

// Re-export key API management components
pub use api::{
    AdvancedRateLimiter, ApiError, ApiErrorType, ApiManager, ApiRequest, ApiResponse,
    BackoffStrategy, BanDetectionConfig, BanStatusReport, EndpointHealth, EndpointStatus,
    EnhancedApiErrorHandler, EnhancedRetryExecutor, RateLimitStats, RequestPriority,
    RpcConnectionPool,
};

// Re-export key performance components
pub use performance::{
    BenchmarkResults, BenchmarkRunner, CacheManager, CacheStats, MetricsCollector, MetricsSummary,
    ParallelExecutor, ParallelStats, PerformanceConfig, PerformanceManager, PerformanceReport,
};
