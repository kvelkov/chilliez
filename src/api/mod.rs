// src/api/mod.rs
//! API Management Module
//!
//! Provides comprehensive API management for production-scale operations:
//! - Advanced rate limiting with priority queuing
//! - RPC connection pooling with automatic failover
//! - Request distribution and load balancing
//! - Health monitoring and circuit breaker patterns

pub mod connection_pool;
pub mod enhanced_error_handling;
pub mod manager;
pub mod rate_limiter;

pub use rate_limiter::{
    AdvancedRateLimiter, RateLimitConfig, RateLimitPermit, RateLimitStats, RateLimiterManager,
    RequestPriority,
};

pub use connection_pool::{
    EndpointHealth, EndpointMetrics, EndpointStatus, RpcConnection, RpcConnectionPool, RpcEndpoint,
    RpcEndpointConfig,
};

pub use manager::{ApiError, ApiManager, ApiManagerConfig, ApiRequest, ApiResponse};

pub use enhanced_error_handling::{
    ApiErrorType, BackoffStrategy, BanDetectionConfig, BanStatusReport, EnhancedApiErrorHandler,
    EnhancedRetryExecutor,
};
