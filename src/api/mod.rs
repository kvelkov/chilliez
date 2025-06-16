// src/api/mod.rs
//! API Management Module
//! 
//! Provides comprehensive API management for production-scale operations:
//! - Advanced rate limiting with priority queuing
//! - RPC connection pooling with automatic failover
//! - Request distribution and load balancing
//! - Health monitoring and circuit breaker patterns

pub mod rate_limiter;
pub mod connection_pool;
pub mod manager;
pub mod enhanced_error_handling;

pub use rate_limiter::{
    AdvancedRateLimiter, 
    RateLimitPermit, 
    RateLimitConfig, 
    RateLimitStats,
    RateLimiterManager,
    RequestPriority,
};

pub use connection_pool::{
    RpcConnectionPool,
    RpcEndpoint,
    RpcConnection,
    RpcEndpointConfig,
    EndpointHealth,
    EndpointStatus,
    EndpointMetrics,
};

pub use manager::{
    ApiManager,
    ApiManagerConfig,
    ApiRequest,
    ApiResponse,
    ApiError,
};

pub use enhanced_error_handling::{
    EnhancedApiErrorHandler,
    EnhancedRetryExecutor,
    ApiErrorType,
    BanDetectionConfig,
    BackoffStrategy,
    BanStatusReport,
};
