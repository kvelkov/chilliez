pub mod arbitrage;
pub mod local_metrics;
pub mod config;
pub mod dex;
pub mod solana;
pub mod utils;
pub mod error;
pub mod websocket;
pub mod webhooks;
pub mod cache;
pub mod helius_client;  // Enhanced Helius SDK client with rate limiting
pub mod testing; // Testing infrastructure
pub mod paper_trading; // Paper trading simulation system
pub mod monitoring; // Enhanced monitoring and alerting
pub mod api; // NEW: Production API management (rate limiting, connection pooling, failover)

// Re-export key testing components for easy access
pub use testing::{
    MockDexEnvironment, MarketCondition, TestSuiteRunner,
};

// Re-export key API management components
pub use api::{
    ApiManager, ApiRequest, ApiResponse, ApiError,
    AdvancedRateLimiter, RequestPriority, RateLimitStats,
    RpcConnectionPool, EndpointHealth, EndpointStatus,
};