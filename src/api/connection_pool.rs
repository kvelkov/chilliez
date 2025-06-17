// src/api/connection_pool.rs
//! Advanced RPC Connection Pool with Automatic Failover
//!
//! Provides:
//! - Connection pooling for multiple RPC endpoints
//! - Automatic failover between providers
//! - Load balancing and request distribution
//! - Health monitoring and circuit breaker patterns

use crate::api::rate_limiter::{AdvancedRateLimiter, RequestPriority};
use anyhow::{anyhow, Result};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient as NonBlockingRpcClient;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};

/// RPC endpoint configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcEndpointConfig {
    pub url: String,
    pub name: String,
    pub priority: u32, // Lower = higher priority
    pub max_connections: usize,
    pub timeout_ms: u64,
    pub max_retries: u32,
    pub health_check_interval_secs: u64,
}

impl RpcEndpointConfig {
    /// Create primary Helius endpoint config
    pub fn helius_primary(url: String) -> Self {
        Self {
            url,
            name: "Helius-Primary".to_string(),
            priority: 1,
            max_connections: 20,
            timeout_ms: 5000,
            max_retries: 3,
            health_check_interval_secs: 30,
        }
    }

    /// Create secondary RPC endpoint config
    pub fn secondary_rpc(url: String, name: String) -> Self {
        Self {
            url,
            name,
            priority: 2,
            max_connections: 10,
            timeout_ms: 8000,
            max_retries: 2,
            health_check_interval_secs: 60,
        }
    }

    /// Create backup RPC endpoint config
    pub fn backup_rpc(url: String, name: String) -> Self {
        Self {
            url,
            name,
            priority: 3,
            max_connections: 5,
            timeout_ms: 10000,
            max_retries: 1,
            health_check_interval_secs: 120,
        }
    }
}

/// Health status of an RPC endpoint
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EndpointHealth {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

/// Performance metrics for an endpoint
#[derive(Debug, Clone)]
pub struct EndpointMetrics {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub average_latency_ms: f64,
    pub last_success: Option<Instant>,
    pub last_failure: Option<Instant>,
    pub consecutive_failures: u32,
}

/// Serializable version of EndpointMetrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableEndpointMetrics {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub average_latency_ms: f64,
    pub consecutive_failures: u32,
    pub last_success_ago_secs: Option<u64>,
    pub last_failure_ago_secs: Option<u64>,
}

impl From<&EndpointMetrics> for SerializableEndpointMetrics {
    fn from(metrics: &EndpointMetrics) -> Self {
        let now = Instant::now();
        Self {
            total_requests: metrics.total_requests,
            successful_requests: metrics.successful_requests,
            failed_requests: metrics.failed_requests,
            average_latency_ms: metrics.average_latency_ms,
            consecutive_failures: metrics.consecutive_failures,
            last_success_ago_secs: metrics
                .last_success
                .map(|t| now.duration_since(t).as_secs()),
            last_failure_ago_secs: metrics
                .last_failure
                .map(|t| now.duration_since(t).as_secs()),
        }
    }
}

impl Default for EndpointMetrics {
    fn default() -> Self {
        Self {
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            average_latency_ms: 0.0,
            last_success: None,
            last_failure: None,
            consecutive_failures: 0,
        }
    }
}

/// RPC endpoint with health monitoring
pub struct RpcEndpoint {
    pub config: RpcEndpointConfig,
    pub client: Arc<NonBlockingRpcClient>,
    pub health: Arc<RwLock<EndpointHealth>>,
    pub metrics: Arc<RwLock<EndpointMetrics>>,
    pub rate_limiter: Option<Arc<AdvancedRateLimiter>>,
    active_connections: Arc<RwLock<usize>>,
    circuit_breaker: Arc<RwLock<CircuitBreaker>>,
}

impl RpcEndpoint {
    /// Create a new RPC endpoint
    pub fn new(config: RpcEndpointConfig, rate_limiter: Option<Arc<AdvancedRateLimiter>>) -> Self {
        let client = Arc::new(NonBlockingRpcClient::new_with_timeout(
            config.url.clone(),
            Duration::from_millis(config.timeout_ms),
        ));

        let circuit_breaker = CircuitBreaker::new(
            5,                       // failure_threshold
            Duration::from_secs(30), // timeout_duration
            Duration::from_secs(60), // recovery_timeout
        );

        info!(
            "🔗 Created RPC endpoint: {} (priority: {}, max_conn: {})",
            config.name, config.priority, config.max_connections
        );

        Self {
            config,
            client,
            health: Arc::new(RwLock::new(EndpointHealth::Unknown)),
            metrics: Arc::new(RwLock::new(EndpointMetrics::default())),
            rate_limiter,
            active_connections: Arc::new(RwLock::new(0)),
            circuit_breaker: Arc::new(RwLock::new(circuit_breaker)),
        }
    }

    /// Check if endpoint can accept new connections
    pub async fn can_accept_connection(&self) -> bool {
        let active = *self.active_connections.read().await;
        let health = *self.health.read().await;
        let circuit_open = self.circuit_breaker.read().await.is_open();

        active < self.config.max_connections && health != EndpointHealth::Unhealthy && !circuit_open
    }

    /// Acquire a connection to this endpoint
    pub async fn acquire_connection(&self) -> Result<RpcConnection> {
        if !self.can_accept_connection().await {
            return Err(anyhow!(
                "Endpoint {} cannot accept new connections",
                self.config.name
            ));
        }

        // Check rate limits if configured
        if let Some(rate_limiter) = &self.rate_limiter {
            if !rate_limiter.can_make_request().await {
                return Err(anyhow!(
                    "Rate limit exceeded for endpoint {}",
                    self.config.name
                ));
            }
        }

        {
            let mut active = self.active_connections.write().await;
            *active += 1;
        }

        debug!("📞 Acquired connection to {}", self.config.name);

        Ok(RpcConnection::new(
            self.client.clone(),
            self.config.clone(),
            self.active_connections.clone(),
            self.metrics.clone(),
            self.circuit_breaker.clone(),
            self.rate_limiter.clone(),
        ))
    }

    /// Perform health check on this endpoint
    pub async fn health_check(&self) -> EndpointHealth {
        let start = Instant::now();

        match self.client.get_slot().await {
            Ok(_) => {
                let latency_ms = start.elapsed().as_millis() as f64;

                // Update circuit breaker
                self.circuit_breaker.write().await.record_success();

                // Update health based on latency
                let health = if latency_ms < 1000.0 {
                    EndpointHealth::Healthy
                } else if latency_ms < 5000.0 {
                    EndpointHealth::Degraded
                } else {
                    EndpointHealth::Unhealthy
                };

                *self.health.write().await = health;

                // Update metrics
                {
                    let mut metrics = self.metrics.write().await;
                    metrics.last_success = Some(Instant::now());
                    metrics.consecutive_failures = 0;
                    metrics.successful_requests += 1;

                    // Update rolling average latency
                    if metrics.total_requests > 0 {
                        metrics.average_latency_ms = (metrics.average_latency_ms
                            * metrics.total_requests as f64
                            + latency_ms)
                            / (metrics.total_requests + 1) as f64;
                    } else {
                        metrics.average_latency_ms = latency_ms;
                    }
                    metrics.total_requests += 1;
                }

                debug!(
                    "✅ Health check passed for {} ({}ms)",
                    self.config.name, latency_ms as u64
                );
                health
            }
            Err(e) => {
                // Update circuit breaker
                self.circuit_breaker.write().await.record_failure();

                *self.health.write().await = EndpointHealth::Unhealthy;

                // Update metrics
                {
                    let mut metrics = self.metrics.write().await;
                    metrics.last_failure = Some(Instant::now());
                    metrics.consecutive_failures += 1;
                    metrics.failed_requests += 1;
                    metrics.total_requests += 1;
                }

                error!("❌ Health check failed for {}: {}", self.config.name, e);
                EndpointHealth::Unhealthy
            }
        }
    }

    /// Get current endpoint status
    pub async fn get_status(&self) -> EndpointStatus {
        let health = *self.health.read().await;
        let metrics = self.metrics.read().await.clone();
        let active_connections = *self.active_connections.read().await;
        let circuit_open = self.circuit_breaker.read().await.is_open();

        EndpointStatus {
            name: self.config.name.clone(),
            url: self.config.url.clone(),
            priority: self.config.priority,
            health,
            metrics,
            active_connections,
            max_connections: self.config.max_connections,
            circuit_breaker_open: circuit_open,
        }
    }
}

/// RAII connection wrapper
pub struct RpcConnection {
    client: Arc<NonBlockingRpcClient>,
    config: RpcEndpointConfig,
    active_connections: Arc<RwLock<usize>>,
    metrics: Arc<RwLock<EndpointMetrics>>,
    circuit_breaker: Arc<RwLock<CircuitBreaker>>,
    rate_limiter: Option<Arc<AdvancedRateLimiter>>,
    start_time: Instant,
}

impl RpcConnection {
    fn new(
        client: Arc<NonBlockingRpcClient>,
        config: RpcEndpointConfig,
        active_connections: Arc<RwLock<usize>>,
        metrics: Arc<RwLock<EndpointMetrics>>,
        circuit_breaker: Arc<RwLock<CircuitBreaker>>,
        rate_limiter: Option<Arc<AdvancedRateLimiter>>,
    ) -> Self {
        Self {
            client,
            config,
            active_connections,
            metrics,
            circuit_breaker,
            rate_limiter,
            start_time: Instant::now(),
        }
    }

    /// Get the underlying RPC client
    pub fn client(&self) -> &NonBlockingRpcClient {
        &self.client
    }

    /// Record a successful request
    pub async fn record_success(&self) {
        let duration_ms = self.start_time.elapsed().as_millis() as u64;

        // Update circuit breaker
        self.circuit_breaker.write().await.record_success();

        // Update metrics
        {
            let mut metrics = self.metrics.write().await;
            metrics.successful_requests += 1;
            metrics.consecutive_failures = 0;
            metrics.last_success = Some(Instant::now());
        }

        // Update rate limiter if configured
        if let Some(rate_limiter) = &self.rate_limiter {
            rate_limiter
                .record_request("rpc_call", duration_ms, RequestPriority::Medium)
                .await;
        }

        debug!(
            "✅ RPC request succeeded on {} ({}ms)",
            self.config.name, duration_ms
        );
    }

    /// Record a failed request
    pub async fn record_failure(&self, error: &str) {
        // Update circuit breaker
        self.circuit_breaker.write().await.record_failure();

        // Update metrics
        {
            let mut metrics = self.metrics.write().await;
            metrics.failed_requests += 1;
            metrics.consecutive_failures += 1;
            metrics.last_failure = Some(Instant::now());
        }

        warn!("❌ RPC request failed on {}: {}", self.config.name, error);
    }
}

impl Drop for RpcConnection {
    fn drop(&mut self) {
        // Release the connection count when dropped
        tokio::spawn({
            let active_connections = self.active_connections.clone();
            let endpoint_name = self.config.name.clone();
            async move {
                {
                    let mut active = active_connections.write().await;
                    if *active > 0 {
                        *active -= 1;
                    }
                }
                debug!("📞 Released connection to {}", endpoint_name);
            }
        });
    }
}

/// Circuit breaker for endpoint protection
#[derive(Debug)]
pub struct CircuitBreaker {
    failure_threshold: u32,
    #[allow(dead_code)]
    timeout_duration: Duration,
    recovery_timeout: Duration,
    failure_count: u32,
    last_failure_time: Option<Instant>,
    state: CircuitBreakerState,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CircuitBreakerState {
    Closed, // Normal operation
    Open,   // Failing fast
    #[allow(dead_code)]
    HalfOpen, // Testing recovery
}

impl CircuitBreaker {
    fn new(failure_threshold: u32, timeout_duration: Duration, recovery_timeout: Duration) -> Self {
        Self {
            failure_threshold,
            timeout_duration,
            recovery_timeout,
            failure_count: 0,
            last_failure_time: None,
            state: CircuitBreakerState::Closed,
        }
    }

    fn record_success(&mut self) {
        self.failure_count = 0;
        self.state = CircuitBreakerState::Closed;
    }

    fn record_failure(&mut self) {
        self.failure_count += 1;
        self.last_failure_time = Some(Instant::now());

        if self.failure_count >= self.failure_threshold {
            self.state = CircuitBreakerState::Open;
        }
    }

    fn is_open(&self) -> bool {
        match self.state {
            CircuitBreakerState::Open => {
                if let Some(last_failure) = self.last_failure_time {
                    Instant::now().duration_since(last_failure) < self.recovery_timeout
                } else {
                    true
                }
            }
            _ => false,
        }
    }
}

/// RPC connection pool with automatic failover
pub struct RpcConnectionPool {
    endpoints: Vec<Arc<RpcEndpoint>>,
    current_primary: Arc<RwLock<usize>>,
    round_robin_counter: Arc<Mutex<usize>>,
}

impl RpcConnectionPool {
    /// Create a new connection pool
    pub fn new() -> Self {
        Self {
            endpoints: Vec::new(),
            current_primary: Arc::new(RwLock::new(0)),
            round_robin_counter: Arc::new(Mutex::new(0)),
        }
    }

    /// Add an endpoint to the pool
    pub async fn add_endpoint(&mut self, endpoint: RpcEndpoint) {
        let endpoint = Arc::new(endpoint);

        // Insert in priority order
        let insert_pos = self
            .endpoints
            .iter()
            .position(|e| e.config.priority > endpoint.config.priority)
            .unwrap_or(self.endpoints.len());

        self.endpoints.insert(insert_pos, endpoint.clone());

        info!(
            "🏊 Added endpoint to pool: {} (total: {})",
            endpoint.config.name,
            self.endpoints.len()
        );

        // Start health check task for this endpoint
        self.start_health_check_task(endpoint).await;
    }

    /// Get the best available connection
    pub async fn get_connection(&self) -> Result<(RpcConnection, String)> {
        // Try primary endpoint first
        let primary_idx = *self.current_primary.read().await;
        if let Some(endpoint) = self.endpoints.get(primary_idx) {
            if let Ok(conn) = endpoint.acquire_connection().await {
                return Ok((conn, endpoint.config.name.clone()));
            }
        }

        // Try other endpoints in priority order
        for (idx, endpoint) in self.endpoints.iter().enumerate() {
            if idx == primary_idx {
                continue; // Already tried
            }

            if let Ok(conn) = endpoint.acquire_connection().await {
                // Update primary if this is higher priority
                if endpoint.config.priority < self.endpoints[primary_idx].config.priority {
                    *self.current_primary.write().await = idx;
                    info!("🔄 Switched primary endpoint to: {}", endpoint.config.name);
                }

                return Ok((conn, endpoint.config.name.clone()));
            }
        }

        Err(anyhow!("No healthy RPC endpoints available"))
    }

    /// Get connection with round-robin load balancing
    pub async fn get_connection_round_robin(&self) -> Result<(RpcConnection, String)> {
        if self.endpoints.is_empty() {
            return Err(anyhow!("No RPC endpoints configured"));
        }

        let mut counter = self.round_robin_counter.lock().await;
        let start_idx = *counter;

        // Try each endpoint in round-robin order
        for _ in 0..self.endpoints.len() {
            let idx = *counter % self.endpoints.len();
            *counter = (*counter + 1) % self.endpoints.len();

            if let Some(endpoint) = self.endpoints.get(idx) {
                if let Ok(conn) = endpoint.acquire_connection().await {
                    return Ok((conn, endpoint.config.name.clone()));
                }
            }
        }

        // Reset counter if we couldn't find any healthy endpoint
        *counter = start_idx;

        Err(anyhow!(
            "No healthy RPC endpoints available for round-robin"
        ))
    }

    /// Get all endpoint statuses
    pub async fn get_all_statuses(&self) -> Vec<EndpointStatus> {
        let mut statuses = Vec::new();

        for endpoint in &self.endpoints {
            statuses.push(endpoint.get_status().await);
        }

        statuses
    }

    /// Start health check task for an endpoint
    async fn start_health_check_task(&self, endpoint: Arc<RpcEndpoint>) {
        let check_interval = Duration::from_secs(endpoint.config.health_check_interval_secs);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(check_interval);

            loop {
                interval.tick().await;
                endpoint.health_check().await;
            }
        });
    }

    /// Create standard connection pool with default endpoints
    pub async fn create_standard_pool(
        primary_rpc: String,
        secondary_rpc: Option<String>,
    ) -> Result<Self> {
        let mut pool = Self::new();

        // Add primary Helius endpoint
        let helius_config = RpcEndpointConfig::helius_primary(primary_rpc);
        let helius_endpoint = RpcEndpoint::new(helius_config, None);
        pool.add_endpoint(helius_endpoint).await;

        // Add secondary endpoint if provided
        if let Some(secondary_url) = secondary_rpc {
            let secondary_config =
                RpcEndpointConfig::secondary_rpc(secondary_url, "Secondary-RPC".to_string());
            let secondary_endpoint = RpcEndpoint::new(secondary_config, None);
            pool.add_endpoint(secondary_endpoint).await;
        }

        // Add public Solana RPC as backup
        let backup_config = RpcEndpointConfig::backup_rpc(
            "https://api.mainnet-beta.solana.com".to_string(),
            "Solana-Public".to_string(),
        );
        let backup_endpoint = RpcEndpoint::new(backup_config, None);
        pool.add_endpoint(backup_endpoint).await;

        info!(
            "🏊‍♂️ Created standard RPC connection pool with {} endpoints",
            pool.endpoints.len()
        );

        Ok(pool)
    }
}

/// Endpoint status information
#[derive(Debug, Clone)]
pub struct EndpointStatus {
    pub name: String,
    pub url: String,
    pub priority: u32,
    pub health: EndpointHealth,
    pub metrics: EndpointMetrics,
    pub active_connections: usize,
    pub max_connections: usize,
    pub circuit_breaker_open: bool,
}

/// Serializable version of EndpointStatus
#[derive(Debug, Clone, Serialize)]
pub struct SerializableEndpointStatus {
    pub name: String,
    pub url: String,
    pub priority: u32,
    pub health: EndpointHealth,
    pub metrics: SerializableEndpointMetrics,
    pub active_connections: usize,
    pub max_connections: usize,
    pub circuit_breaker_open: bool,
}

impl From<&EndpointStatus> for SerializableEndpointStatus {
    fn from(status: &EndpointStatus) -> Self {
        Self {
            name: status.name.clone(),
            url: status.url.clone(),
            priority: status.priority,
            health: status.health,
            metrics: (&status.metrics).into(),
            active_connections: status.active_connections,
            max_connections: status.max_connections,
            circuit_breaker_open: status.circuit_breaker_open,
        }
    }
}

impl std::fmt::Display for EndpointStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {:?} ({}ms avg, {}/{} conn, success: {}/{})",
            self.name,
            self.health,
            self.metrics.average_latency_ms as u64,
            self.active_connections,
            self.max_connections,
            self.metrics.successful_requests,
            self.metrics.total_requests
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connection_pool_creation() {
        let mut pool = RpcConnectionPool::new();
        assert_eq!(pool.endpoints.len(), 0);

        let config = RpcEndpointConfig::helius_primary("https://test.helius.com".to_string());
        let endpoint = RpcEndpoint::new(config, None);

        pool.add_endpoint(endpoint).await;
        assert_eq!(pool.endpoints.len(), 1);
    }

    #[tokio::test]
    async fn test_endpoint_priority_ordering() {
        let mut pool = RpcConnectionPool::new();

        // Add endpoints in reverse priority order
        let low_priority =
            RpcEndpointConfig::backup_rpc("https://backup.com".to_string(), "Backup".to_string());
        let high_priority = RpcEndpointConfig::helius_primary("https://primary.com".to_string());

        pool.add_endpoint(RpcEndpoint::new(low_priority, None))
            .await;
        pool.add_endpoint(RpcEndpoint::new(high_priority, None))
            .await;

        // Primary should be first due to priority ordering
        assert_eq!(pool.endpoints[0].config.priority, 1);
        assert_eq!(pool.endpoints[1].config.priority, 3);
    }

    #[test]
    fn test_circuit_breaker() {
        let mut cb = CircuitBreaker::new(3, Duration::from_secs(10), Duration::from_secs(30));

        // Should start closed
        assert!(!cb.is_open());

        // Record failures
        cb.record_failure();
        cb.record_failure();
        assert!(!cb.is_open()); // Not open yet

        cb.record_failure(); // Should trigger open
        assert!(cb.is_open());

        // Success should close it
        cb.record_success();
        assert!(!cb.is_open());
    }
}
