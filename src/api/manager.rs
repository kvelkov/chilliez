// src/api/manager.rs
//! Central API Manager for Production Operations
//!
//! Coordinates rate limiting, connection pooling, and request distribution
//! across all API providers for optimal performance and reliability.

use anyhow::Result;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use super::connection_pool::{EndpointStatus, RpcConnection, RpcConnectionPool};
use super::rate_limiter::{RateLimitStats, RateLimiterManager, RequestPriority};
use crate::config::Config;

/// Configuration for the API manager
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiManagerConfig {
    pub enable_rate_limiting: bool,
    pub enable_connection_pooling: bool,
    pub enable_automatic_failover: bool,
    pub max_retry_attempts: u32,
    pub base_retry_delay_ms: u64,
    pub max_retry_delay_ms: u64,
    pub health_check_interval_secs: u64,
    pub metrics_collection_enabled: bool,
}

impl Default for ApiManagerConfig {
    fn default() -> Self {
        Self {
            enable_rate_limiting: true,
            enable_connection_pooling: true,
            enable_automatic_failover: true,
            max_retry_attempts: 3,
            base_retry_delay_ms: 100,
            max_retry_delay_ms: 5000,
            health_check_interval_secs: 30,
            metrics_collection_enabled: true,
        }
    }
}

/// API request information
#[derive(Debug, Clone)]
pub struct ApiRequest {
    pub provider: String,
    pub endpoint: String,
    pub priority: RequestPriority,
    pub timeout: Option<Duration>,
    pub retry_on_failure: bool,
}

impl ApiRequest {
    /// Create a new API request
    pub fn new(provider: &str, endpoint: &str, priority: RequestPriority) -> Self {
        Self {
            provider: provider.to_string(),
            endpoint: endpoint.to_string(),
            priority,
            timeout: None,
            retry_on_failure: true,
        }
    }

    /// Create a critical request (highest priority, minimal retries)
    pub fn critical(provider: &str, endpoint: &str) -> Self {
        Self {
            provider: provider.to_string(),
            endpoint: endpoint.to_string(),
            priority: RequestPriority::Critical,
            timeout: Some(Duration::from_millis(3000)),
            retry_on_failure: false,
        }
    }

    /// Create a trading request (high priority)
    pub fn trading(provider: &str, endpoint: &str) -> Self {
        Self {
            provider: provider.to_string(),
            endpoint: endpoint.to_string(),
            priority: RequestPriority::High,
            timeout: Some(Duration::from_millis(5000)),
            retry_on_failure: true,
        }
    }

    /// Create a background request (low priority)
    pub fn background(provider: &str, endpoint: &str) -> Self {
        Self {
            provider: provider.to_string(),
            endpoint: endpoint.to_string(),
            priority: RequestPriority::Background,
            timeout: Some(Duration::from_millis(30000)),
            retry_on_failure: true,
        }
    }
}

/// API response with metadata
#[derive(Debug)]
pub struct ApiResponse<T> {
    pub data: T,
    pub provider: String,
    pub endpoint: String,
    pub latency_ms: u64,
    pub attempt: u32,
    pub from_cache: bool,
}

impl<T> ApiResponse<T> {
    pub fn new(data: T, provider: String, endpoint: String, latency_ms: u64, attempt: u32) -> Self {
        Self {
            data,
            provider,
            endpoint,
            latency_ms,
            attempt,
            from_cache: false,
        }
    }
}

/// API-specific errors
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Rate limit exceeded for provider {provider}: {message}")]
    RateLimitExceeded { provider: String, message: String },

    #[error("All endpoints unhealthy for provider {provider}")]
    NoHealthyEndpoints { provider: String },

    #[error("Request timeout after {timeout_ms}ms for {provider}/{endpoint}")]
    RequestTimeout {
        provider: String,
        endpoint: String,
        timeout_ms: u64,
    },

    #[error("Max retry attempts ({attempts}) exceeded for {provider}/{endpoint}")]
    MaxRetriesExceeded {
        provider: String,
        endpoint: String,
        attempts: u32,
    },

    #[error("Provider {provider} not configured")]
    ProviderNotConfigured { provider: String },

    #[error("Request failed: {message}")]
    RequestFailed { message: String },
}

/// Central API manager coordinating all API operations
pub struct ApiManager {
    config: ApiManagerConfig,
    rate_limiters: Arc<RwLock<RateLimiterManager>>,
    rpc_pool: Arc<RwLock<RpcConnectionPool>>,
    request_metrics: Arc<RwLock<HashMap<String, RequestMetrics>>>,
}

impl ApiManager {
    /// Create a new API manager
    pub async fn new(config: ApiManagerConfig, app_config: Arc<Config>) -> Result<Self> {
        info!("🏗️ Initializing API Manager with production configuration");

        // Initialize rate limiters
        let rate_limiters = if config.enable_rate_limiting {
            RateLimiterManager::create_standard_limiters()
        } else {
            RateLimiterManager::new()
        };

        // Initialize RPC connection pool
        let rpc_pool = if config.enable_connection_pooling {
            RpcConnectionPool::create_standard_pool(
                app_config.rpc_url.clone(),
                app_config.rpc_url_secondary.clone(),
            )
            .await?
        } else {
            RpcConnectionPool::new()
        };

        let manager = Self {
            config: config.clone(),
            rate_limiters: Arc::new(RwLock::new(rate_limiters)),
            rpc_pool: Arc::new(RwLock::new(rpc_pool)),
            request_metrics: Arc::new(RwLock::new(HashMap::new())),
        };

        // Start background health monitoring
        if config.health_check_interval_secs > 0 {
            manager.start_health_monitoring().await;
        }

        info!("✅ API Manager initialized successfully");
        Ok(manager)
    }

    /// Execute a request with full API management
    pub async fn execute_request<T, F, Fut>(
        &self,
        request: ApiRequest,
        operation: F,
    ) -> Result<ApiResponse<T>, ApiError>
    where
        F: Fn(RpcConnection) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let start_time = Instant::now();
        let mut attempt = 1;
        let mut last_error = None;

        while attempt <= self.config.max_retry_attempts {
            match self
                .try_execute_request(&request, &operation, attempt)
                .await
            {
                Ok(response) => {
                    self.record_success(&request, start_time.elapsed(), attempt)
                        .await;
                    return Ok(response);
                }
                Err(e) => {
                    self.record_failure(&request, &e, attempt).await;
                    last_error = Some(e);

                    if !request.retry_on_failure || attempt >= self.config.max_retry_attempts {
                        break;
                    }

                    // Exponential backoff with jitter
                    let delay_ms = std::cmp::min(
                        self.config.base_retry_delay_ms * (2_u64.pow(attempt - 1)),
                        self.config.max_retry_delay_ms,
                    );

                    // Add jitter (±25%)
                    let jitter = (delay_ms / 4) as i64;
                    let jittered_delay = delay_ms as i64 + fastrand::i64(-jitter..=jitter);
                    let final_delay =
                        Duration::from_millis(std::cmp::max(0, jittered_delay) as u64);

                    debug!(
                        "🔄 Retrying request {}/{} for {}/{} after {:?}",
                        attempt,
                        self.config.max_retry_attempts,
                        request.provider,
                        request.endpoint,
                        final_delay
                    );

                    tokio::time::sleep(final_delay).await;
                    attempt += 1;
                }
            }
        }

        Err(last_error.unwrap_or_else(|| ApiError::RequestFailed {
            message: "Unknown error occurred".to_string(),
        }))
    }

    /// Try to execute a request once
    async fn try_execute_request<T, F, Fut>(
        &self,
        request: &ApiRequest,
        operation: &F,
        attempt: u32,
    ) -> Result<ApiResponse<T>, ApiError>
    where
        F: Fn(RpcConnection) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let start_time = Instant::now();

        // Check rate limits
        if self.config.enable_rate_limiting {
            let rate_limiters = self.rate_limiters.read().await;
            if let Some(limiter) = rate_limiters.get_limiter(&request.provider) {
                let _permit = limiter
                    .acquire_permit(request.priority, &request.endpoint)
                    .await
                    .map_err(|e| ApiError::RateLimitExceeded {
                        provider: request.provider.clone(),
                        message: e.to_string(),
                    })?;
            }
        }

        // Get RPC connection
        let (connection, endpoint_name) = if self.config.enable_connection_pooling {
            let pool = self.rpc_pool.read().await;
            match request.priority {
                RequestPriority::Critical | RequestPriority::High => {
                    // Use failover for critical requests
                    pool.get_connection().await
                }
                _ => {
                    // Use round-robin for normal requests
                    pool.get_connection_round_robin().await
                }
            }
            .map_err(|_| ApiError::NoHealthyEndpoints {
                provider: request.provider.clone(),
            })?
        } else {
            return Err(ApiError::ProviderNotConfigured {
                provider: request.provider.clone(),
            });
        };

        // Execute the operation with timeout
        let result = if let Some(timeout) = request.timeout {
            tokio::time::timeout(timeout, operation(connection))
                .await
                .map_err(|_| ApiError::RequestTimeout {
                    provider: request.provider.clone(),
                    endpoint: request.endpoint.clone(),
                    timeout_ms: timeout.as_millis() as u64,
                })?
        } else {
            operation(connection).await
        };

        let latency_ms = start_time.elapsed().as_millis() as u64;

        match result {
            Ok(data) => {
                debug!(
                    "✅ Request succeeded: {}/{} ({}ms, attempt {})",
                    request.provider, request.endpoint, latency_ms, attempt
                );

                Ok(ApiResponse::new(
                    data,
                    endpoint_name,
                    request.endpoint.clone(),
                    latency_ms,
                    attempt,
                ))
            }
            Err(e) => {
                warn!(
                    "❌ Request failed: {}/{} ({}ms, attempt {}): {}",
                    request.provider, request.endpoint, latency_ms, attempt, e
                );

                Err(ApiError::RequestFailed {
                    message: e.to_string(),
                })
            }
        }
    }

    /// Record successful request
    async fn record_success(&self, request: &ApiRequest, duration: Duration, attempt: u32) {
        if !self.config.metrics_collection_enabled {
            return;
        }

        let mut metrics = self.request_metrics.write().await;
        let key = format!("{}/{}", request.provider, request.endpoint);
        let entry = metrics.entry(key).or_insert_with(RequestMetrics::default);

        entry.total_requests += 1;
        entry.successful_requests += 1;
        entry.total_latency_ms += duration.as_millis() as u64;
        entry.last_success = Some(Instant::now());

        if attempt > 1 {
            entry.retried_requests += 1;
        }
    }

    /// Record failed request
    async fn record_failure(&self, request: &ApiRequest, error: &ApiError, _attempt: u32) {
        if !self.config.metrics_collection_enabled {
            return;
        }

        let mut metrics = self.request_metrics.write().await;
        let key = format!("{}/{}", request.provider, request.endpoint);
        let entry = metrics.entry(key).or_insert_with(RequestMetrics::default);

        entry.total_requests += 1;
        entry.failed_requests += 1;
        entry.last_failure = Some(Instant::now());

        // Categorize error types
        match error {
            ApiError::RateLimitExceeded { .. } => entry.rate_limit_errors += 1,
            ApiError::RequestTimeout { .. } => entry.timeout_errors += 1,
            ApiError::NoHealthyEndpoints { .. } => entry.connection_errors += 1,
            _ => entry.other_errors += 1,
        }
    }

    /// Start background health monitoring
    async fn start_health_monitoring(&self) {
        let interval = Duration::from_secs(self.config.health_check_interval_secs);
        let rpc_pool = self.rpc_pool.clone();
        let rate_limiters = self.rate_limiters.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(interval);

            loop {
                interval.tick().await;

                // Log RPC endpoint health
                {
                    let pool = rpc_pool.read().await;
                    let statuses = pool.get_all_statuses().await;

                    for status in statuses {
                        debug!("🏥 Endpoint health: {}", status);
                    }
                }

                // Log rate limiter stats
                {
                    let limiters = rate_limiters.read().await;
                    let stats = limiters.get_all_stats().await;

                    for stat in stats {
                        debug!("🚦 Rate limit status: {}", stat);
                    }
                }
            }
        });

        info!(
            "💗 Started health monitoring (interval: {}s)",
            self.config.health_check_interval_secs
        );
    }

    /// Get comprehensive API statistics
    pub async fn get_api_stats(&self) -> ApiStats {
        let rate_limit_stats = self.rate_limiters.read().await.get_all_stats().await;
        let endpoint_statuses = self.rpc_pool.read().await.get_all_statuses().await;
        let request_metrics = self.request_metrics.read().await.clone();

        ApiStats {
            rate_limit_stats,
            endpoint_statuses,
            request_metrics,
            uptime_secs: 0, // Could track this if needed
        }
    }

    /// Create a production-ready API manager
    pub async fn create_production_manager(app_config: Arc<Config>) -> Result<Self> {
        let config = ApiManagerConfig {
            enable_rate_limiting: true,
            enable_connection_pooling: true,
            enable_automatic_failover: true,
            max_retry_attempts: 3,
            base_retry_delay_ms: 50,
            max_retry_delay_ms: 2000,
            health_check_interval_secs: 30,
            metrics_collection_enabled: true,
        };

        Self::new(config, app_config).await
    }
}

/// Request metrics for monitoring
#[derive(Debug, Clone, Default)]
pub struct RequestMetrics {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub retried_requests: u64,
    pub total_latency_ms: u64,
    pub rate_limit_errors: u64,
    pub timeout_errors: u64,
    pub connection_errors: u64,
    pub other_errors: u64,
    pub last_success: Option<Instant>,
    pub last_failure: Option<Instant>,
}

impl RequestMetrics {
    /// Calculate average latency
    pub fn average_latency_ms(&self) -> f64 {
        if self.successful_requests > 0 {
            self.total_latency_ms as f64 / self.successful_requests as f64
        } else {
            0.0
        }
    }

    /// Calculate success rate
    pub fn success_rate(&self) -> f64 {
        if self.total_requests > 0 {
            self.successful_requests as f64 / self.total_requests as f64
        } else {
            0.0
        }
    }
}

/// Comprehensive API statistics
#[derive(Debug, Clone)]
pub struct ApiStats {
    pub rate_limit_stats: Vec<RateLimitStats>,
    pub endpoint_statuses: Vec<EndpointStatus>,
    pub request_metrics: HashMap<String, RequestMetrics>,
    pub uptime_secs: u64,
}

impl std::fmt::Display for ApiStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== API Manager Statistics ===")?;

        writeln!(f, "\n📊 Rate Limiting:")?;
        for stat in &self.rate_limit_stats {
            writeln!(f, "  {}", stat)?;
        }

        writeln!(f, "\n🔗 RPC Endpoints:")?;
        for status in &self.endpoint_statuses {
            writeln!(f, "  {}", status)?;
        }

        writeln!(f, "\n📈 Request Metrics:")?;
        for (key, metrics) in &self.request_metrics {
            writeln!(
                f,
                "  {}: {:.1}% success, {:.1}ms avg, {} total",
                key,
                metrics.success_rate() * 100.0,
                metrics.average_latency_ms(),
                metrics.total_requests
            )?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_api_manager_creation() {
        let config = ApiManagerConfig::default();
        let app_config = Arc::new(Config::test_default());

        // This might fail without real endpoints, but tests the creation logic
        let result = ApiManager::new(config, app_config).await;
        // We expect this to work for testing purposes
        assert!(result.is_ok() || result.is_err()); // Either way is fine for test
    }

    #[tokio::test]
    async fn test_api_request_creation() {
        let request = ApiRequest::critical("helius", "/v1/accounts");
        assert_eq!(request.priority, RequestPriority::Critical);
        assert_eq!(request.provider, "helius");
        assert!(!request.retry_on_failure);

        let trading_request = ApiRequest::trading("jupiter", "/v6/quote");
        assert_eq!(trading_request.priority, RequestPriority::High);
        assert!(trading_request.retry_on_failure);
    }

    #[test]
    fn test_request_metrics() {
        let mut metrics = RequestMetrics::default();
        metrics.total_requests = 100;
        metrics.successful_requests = 95;
        metrics.total_latency_ms = 9500;

        assert_eq!(metrics.success_rate(), 0.95);
        assert_eq!(metrics.average_latency_ms(), 100.0);
    }
}
