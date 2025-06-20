pub mod api; // NEW: Production API management (rate limiting, connection pooling, failover)
pub mod arbitrage;
pub mod cache;
pub mod config;
pub mod dex;
pub mod error;
pub mod ffi; // FFI exports for JavaScript bridge
pub mod jito_bundle;
pub mod monitoring; // Enhanced monitoring and alerting
pub mod simulation; // Simulation system (was paper_trading)
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

// Re-export key monitoring and performance components
pub use monitoring::{
    BenchmarkResults, BenchmarkRunner, PerformanceMetricsCollector, PerformanceConfig, 
    PerformanceManager, PerformanceReport, PerformanceSummary,
    HealthMonitor, HealthStatus,
};
