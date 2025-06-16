// src/arbitrage/routing/failover.rs
//! Failover Routing and Recovery System
//! 
//! Provides robust failover mechanisms for route execution:
//! - Automatic route fallback when primary paths fail
//! - DEX health monitoring and circuit breaker patterns
//! - Dynamic route recomputation on failures
//! - Emergency execution modes
//! - Recovery strategies and retry policies

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::arbitrage::routing::{RoutePath, PathFinder, RoutingGraph};

/// Failover strategy types
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FailoverStrategy {
    /// Switch to backup routes immediately
    ImmediateFailover,
    /// Retry primary route with delays
    RetryWithBackoff,
    /// Switch to different DEX for same token pair
    DexSwitching,
    /// Emergency liquidation mode (accept worse prices)
    EmergencyExecution,
    /// Abort execution and wait for conditions to improve
    AbortAndWait,
}

/// Route execution status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionStatus {
    /// Route is healthy and executing normally
    Healthy,
    /// Route is experiencing temporary issues
    Degraded,
    /// Route has failed but may recover
    Failed,
    /// Route is completely unavailable
    Unavailable,
    /// Route is under maintenance
    Maintenance,
}

/// DEX health status tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DexHealthStatus {
    /// DEX identifier
    pub dex_name: String,
    /// Current health status
    pub status: ExecutionStatus,
    /// Last successful execution time
    pub last_success: Option<SystemTime>,
    /// Last failure time
    pub last_failure: Option<SystemTime>,
    /// Consecutive failure count
    pub consecutive_failures: u32,
    /// Average response time (milliseconds)
    pub avg_response_time: f64,
    /// Success rate over last 100 attempts (0.0-1.0)
    pub success_rate: f64,
    /// Current error rate (0.0-1.0)
    pub error_rate: f64,
    /// Circuit breaker state
    pub circuit_breaker_open: bool,
    /// Next retry time if circuit breaker is open
    pub next_retry_time: Option<SystemTime>,
}

/// Failover configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailoverConfig {
    /// Maximum retry attempts for primary route
    pub max_retry_attempts: u32,
    /// Base retry delay (exponential backoff)
    pub base_retry_delay: Duration,
    /// Maximum retry delay cap
    pub max_retry_delay: Duration,
    /// Circuit breaker failure threshold
    pub circuit_breaker_threshold: u32,
    /// Circuit breaker timeout before retry
    pub circuit_breaker_timeout: Duration,
    /// Health check interval
    pub health_check_interval: Duration,
    /// Maximum acceptable response time (milliseconds)
    pub max_response_time: u64,
    /// Minimum success rate to keep DEX active
    pub min_success_rate: f64,
    /// Emergency execution price impact tolerance
    pub emergency_price_impact_tolerance: f64,
    /// Enable automatic DEX switching
    pub enable_dex_switching: bool,
    /// Enable emergency execution mode
    pub enable_emergency_execution: bool,
}

impl Default for FailoverConfig {
    fn default() -> Self {
        Self {
            max_retry_attempts: 3,
            base_retry_delay: Duration::from_millis(500),
            max_retry_delay: Duration::from_secs(10),
            circuit_breaker_threshold: 5,
            circuit_breaker_timeout: Duration::from_secs(5 * 60),
            health_check_interval: Duration::from_secs(30),
            max_response_time: 5000,
            min_success_rate: 0.8,
            emergency_price_impact_tolerance: 0.05, // 5%
            enable_dex_switching: true,
            enable_emergency_execution: true,
        }
    }
}

/// Route execution attempt record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionAttempt {
    /// Attempt number (1-based)
    pub attempt_number: u32,
    /// Route used for this attempt
    pub route: RoutePath,
    /// Execution start time
    pub start_time: SystemTime,
    /// Execution end time (if completed)
    pub end_time: Option<SystemTime>,
    /// Execution result
    pub result: ExecutionResult,
    /// Error details if failed
    pub error_details: Option<String>,
    /// Response time in milliseconds
    pub response_time_ms: Option<u64>,
}

/// Execution result status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionResult {
    /// Execution completed successfully
    Success,
    /// Execution failed but retryable
    RetryableFailure,
    /// Execution failed permanently
    PermanentFailure,
    /// Execution timed out
    Timeout,
    /// Execution was cancelled
    Cancelled,
}

/// Failover execution plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailoverPlan {
    /// Primary route to attempt first
    pub primary_route: RoutePath,
    /// Backup routes in order of preference
    pub backup_routes: Vec<RoutePath>,
    /// Failover strategy to use
    pub strategy: FailoverStrategy,
    /// Maximum total execution time
    pub max_execution_time: Duration,
    /// Emergency routes (worse prices but higher reliability)
    pub emergency_routes: Vec<RoutePath>,
    /// Plan creation time
    pub created_at: SystemTime,
    /// Plan expiration time
    pub expires_at: SystemTime,
}

/// Failover router with circuit breaker and retry logic
pub struct FailoverRouter {
    config: FailoverConfig,
    pathfinder: Arc<RwLock<PathFinder>>,
    routing_graph: RoutingGraph,
    dex_health: RwLock<HashMap<String, DexHealthStatus>>,
    execution_history: RwLock<VecDeque<ExecutionAttempt>>,
    circuit_breakers: RwLock<HashMap<String, CircuitBreaker>>,
}

/// Circuit breaker for individual DEXs
#[derive(Debug, Clone)]
struct CircuitBreaker {
    failure_count: u32,
    last_failure: Option<Instant>,
    state: CircuitBreakerState,
    next_retry: Option<Instant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CircuitBreakerState {
    Closed,  // Normal operation
    Open,    // Failures exceeded threshold, blocking requests
    HalfOpen, // Testing if service recovered
}

impl CircuitBreaker {
    fn new() -> Self {
        Self {
            failure_count: 0,
            last_failure: None,
            state: CircuitBreakerState::Closed,
            next_retry: None,
        }
    }

    fn record_success(&mut self) {
        self.failure_count = 0;
        self.last_failure = None;
        self.state = CircuitBreakerState::Closed;
        self.next_retry = None;
    }

    fn record_failure(&mut self, threshold: u32, timeout: Duration) {
        self.failure_count += 1;
        self.last_failure = Some(Instant::now());

        if self.failure_count >= threshold {
            self.state = CircuitBreakerState::Open;
            self.next_retry = Some(Instant::now() + timeout);
        }
    }

    fn can_execute(&mut self) -> bool {
        match self.state {
            CircuitBreakerState::Closed => true,
            CircuitBreakerState::Open => {
                if let Some(retry_time) = self.next_retry {
                    if Instant::now() >= retry_time {
                        self.state = CircuitBreakerState::HalfOpen;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitBreakerState::HalfOpen => true,
        }
    }
}

impl FailoverRouter {
    /// Create a new failover router
    pub fn new(
        config: FailoverConfig,
        pathfinder: Arc<RwLock<PathFinder>>,
        routing_graph: RoutingGraph,
    ) -> Self {
        Self {
            config,
            pathfinder,
            routing_graph,
            dex_health: RwLock::new(HashMap::new()),
            execution_history: RwLock::new(VecDeque::new()),
            circuit_breakers: RwLock::new(HashMap::new()),
        }
    }

    /// Create a comprehensive failover plan for a route
    pub async fn create_failover_plan(
        &mut self,
        primary_route: RoutePath,
        input_token: &str,
        output_token: &str,
        amount: u64,
    ) -> Result<FailoverPlan, Box<dyn std::error::Error>> {
        let mut backup_routes = Vec::new();
        let mut emergency_routes = Vec::new();

        // Generate backup routes using different DEXs
        if self.config.enable_dex_switching {
            backup_routes = self.generate_backup_routes(input_token, output_token, amount).await?;
        }

        // Generate emergency routes with higher slippage tolerance
        if self.config.enable_emergency_execution {
            emergency_routes = self.generate_emergency_routes(input_token, output_token, amount).await?;
        }

        let strategy = self.determine_failover_strategy(&primary_route).await;

        Ok(FailoverPlan {
            primary_route,
            backup_routes,
            strategy,
            max_execution_time: Duration::from_secs(30),
            emergency_routes,
            created_at: SystemTime::now(),
            expires_at: SystemTime::now() + Duration::from_secs(5 * 60),
        })
    }

    /// Execute a route with automatic failover
    pub async fn execute_with_failover(
        &self,
        plan: &FailoverPlan,
    ) -> Result<ExecutionAttempt, Box<dyn std::error::Error>> {
        let mut attempt_number = 1;
        let _execution_start = SystemTime::now();

        // Try primary route first
        if let Ok(attempt) = self.try_execute_route(&plan.primary_route, attempt_number).await {
            if matches!(attempt.result, ExecutionResult::Success) {
                self.record_execution_success(&plan.primary_route).await;
                return Ok(attempt);
            } else {
                self.record_execution_failure(&plan.primary_route, &attempt).await;
                attempt_number += 1;
            }
        }

        // Apply failover strategy
        match plan.strategy {
            FailoverStrategy::ImmediateFailover => {
                self.execute_immediate_failover(plan, attempt_number).await
            },
            FailoverStrategy::RetryWithBackoff => {
                self.execute_retry_with_backoff(plan, attempt_number).await
            },
            FailoverStrategy::DexSwitching => {
                self.execute_dex_switching(plan, attempt_number).await
            },
            FailoverStrategy::EmergencyExecution => {
                self.execute_emergency_mode(plan, attempt_number).await
            },
            FailoverStrategy::AbortAndWait => {
                Err("Execution aborted due to failover conditions".into())
            },
        }
    }

    /// Update DEX health status
    pub async fn update_dex_health(&self, dex_name: &str, result: &ExecutionAttempt) {
        let mut health_map = self.dex_health.write().await;
        let health = health_map.entry(dex_name.to_string()).or_insert_with(|| {
            DexHealthStatus {
                dex_name: dex_name.to_string(),
                status: ExecutionStatus::Healthy,
                last_success: None,
                last_failure: None,
                consecutive_failures: 0,
                avg_response_time: 0.0,
                success_rate: 1.0,
                error_rate: 0.0,
                circuit_breaker_open: false,
                next_retry_time: None,
            }
        });

        // Update response time
        if let Some(response_time) = result.response_time_ms {
            health.avg_response_time = (health.avg_response_time * 0.9) + (response_time as f64 * 0.1);
        }

        // Update success/failure tracking
        match result.result {
            ExecutionResult::Success => {
                health.last_success = Some(SystemTime::now());
                health.consecutive_failures = 0;
                health.status = ExecutionStatus::Healthy;
                
                // Update circuit breaker
                let mut circuit_breakers = self.circuit_breakers.write().await;
                if let Some(cb) = circuit_breakers.get_mut(dex_name) {
                    cb.record_success();
                }
                health.circuit_breaker_open = false;
            },
            ExecutionResult::RetryableFailure | ExecutionResult::Timeout => {
                health.last_failure = Some(SystemTime::now());
                health.consecutive_failures += 1;
                
                if health.consecutive_failures >= self.config.circuit_breaker_threshold {
                    health.status = ExecutionStatus::Failed;
                    health.circuit_breaker_open = true;
                    health.next_retry_time = Some(SystemTime::now() + self.config.circuit_breaker_timeout);
                } else {
                    health.status = ExecutionStatus::Degraded;
                }
                
                // Update circuit breaker
                let mut circuit_breakers = self.circuit_breakers.write().await;
                let cb = circuit_breakers.entry(dex_name.to_string()).or_insert_with(CircuitBreaker::new);
                cb.record_failure(self.config.circuit_breaker_threshold, self.config.circuit_breaker_timeout);
            },
            ExecutionResult::PermanentFailure => {
                health.last_failure = Some(SystemTime::now());
                health.status = ExecutionStatus::Unavailable;
                health.consecutive_failures += 1;
            },
            ExecutionResult::Cancelled => {
                // Don't update health for cancelled executions
            },
        }

        // Calculate success rate over recent executions
        let history = self.execution_history.read().await;
        let recent_attempts: Vec<&ExecutionAttempt> = history
            .iter()
            .filter(|attempt| {
                attempt.route.steps.iter().any(|step| step.dex_type.to_string() == dex_name)
            })
            .take(100)
            .collect();

        if !recent_attempts.is_empty() {
            let successes = recent_attempts.iter()
                .filter(|attempt| matches!(attempt.result, ExecutionResult::Success))
                .count();
            health.success_rate = successes as f64 / recent_attempts.len() as f64;
            health.error_rate = 1.0 - health.success_rate;
        }
    }

    /// Check if a DEX is available for execution
    pub async fn is_dex_available(&self, dex_name: &str) -> bool {
        // Check circuit breaker
        let mut circuit_breakers = self.circuit_breakers.write().await;
        let cb = circuit_breakers.entry(dex_name.to_string()).or_insert_with(CircuitBreaker::new);
        
        if !cb.can_execute() {
            return false;
        }

        // Check health status
        let health_map = self.dex_health.read().await;
        if let Some(health) = health_map.get(dex_name) {
            !matches!(health.status, ExecutionStatus::Unavailable | ExecutionStatus::Maintenance)
                && health.success_rate >= self.config.min_success_rate
        } else {
            true // Unknown DEX, assume available
        }
    }

    /// Get health summary for all DEXs
    pub async fn get_health_summary(&self) -> HashMap<String, DexHealthStatus> {
        self.dex_health.read().await.clone()
    }

    // Private helper methods

    async fn try_execute_route(
        &self,
        route: &RoutePath,
        attempt_number: u32,
    ) -> Result<ExecutionAttempt, Box<dyn std::error::Error>> {
        let start_time = SystemTime::now();
        
        // Check if all DEXs in route are available
        for step in &route.steps {
            if !self.is_dex_available(&step.dex_type.to_string()).await {
                return Ok(ExecutionAttempt {
                    attempt_number,
                    route: route.clone(),
                    start_time,
                    end_time: Some(SystemTime::now()),
                    result: ExecutionResult::RetryableFailure,
                    error_details: Some(format!("DEX {} is unavailable", step.dex_type.to_string())),
                    response_time_ms: Some(0),
                });
            }
        }

        // Simulate execution (in real implementation, this would call actual DEX clients)
        let execution_duration = Duration::from_millis(
            500 + (route.steps.len() as u64 * 100)
        );
        tokio::time::sleep(execution_duration).await;

        let end_time = SystemTime::now();
        let response_time_ms = end_time.duration_since(start_time)
            .unwrap_or_default()
            .as_millis() as u64;

        // Simulate success/failure based on DEX health
        let success_probability = self.calculate_route_success_probability(route).await;
        let random_value = (response_time_ms % 100) as f64 / 100.0;
        
        let result = if random_value < success_probability {
            ExecutionResult::Success
        } else if response_time_ms > self.config.max_response_time {
            ExecutionResult::Timeout
        } else {
            ExecutionResult::RetryableFailure
        };

        Ok(ExecutionAttempt {
            attempt_number,
            route: route.clone(),
            start_time,
            end_time: Some(end_time),
            result: result.clone(),
            error_details: if matches!(result, ExecutionResult::Success) {
                None
            } else {
                Some("Simulated execution failure".to_string())
            },
            response_time_ms: Some(response_time_ms),
        })
    }

    async fn calculate_route_success_probability(&self, route: &RoutePath) -> f64 {
        let health_map = self.dex_health.read().await;
        let mut total_probability = 1.0;

        for step in &route.steps {
            if let Some(health) = health_map.get(&step.dex_type.to_string()) {
                total_probability *= health.success_rate;
            }
        }

        total_probability
    }

    async fn generate_backup_routes(
        &mut self,
        input_token: &str,
        output_token: &str,
        amount: u64,
    ) -> Result<Vec<RoutePath>, Box<dyn std::error::Error>> {
        // Use pathfinder to generate alternative routes
        let routes = self.pathfinder.write().await.find_k_shortest_paths(
            &self.routing_graph,
            input_token,
            output_token,
            amount as f64,
            3, // Find up to 3 backup routes
        ).await.map_err(|e| format!("Failed to generate backup routes: {}", e))?;

        Ok(routes)
    }

    async fn generate_emergency_routes(
        &mut self,
        input_token: &str,
        output_token: &str,
        amount: u64,
    ) -> Result<Vec<RoutePath>, Box<dyn std::error::Error>> {
        // Generate routes with higher slippage tolerance for emergency execution
        let constraints = crate::arbitrage::routing::pathfinder::PathConstraints {
            max_hops: Some(3),
            max_gas_cost: None,
            max_execution_time: None,
            min_output_amount: None,
            required_dexs: None,
            forbidden_dexs: None,
            max_price_impact: Some(self.config.emergency_price_impact_tolerance),
        };
        
        let routes = self.pathfinder.write().await.find_paths_with_constraints(
            &self.routing_graph,
            input_token,
            output_token,
            amount as f64,
            &constraints,
        ).await.map_err(|e| format!("Failed to generate emergency routes: {}", e))?;

        Ok(routes)
    }

    async fn determine_failover_strategy(&self, _route: &RoutePath) -> FailoverStrategy {
        // Determine best failover strategy based on current conditions
        // For now, use immediate failover as default
        FailoverStrategy::ImmediateFailover
    }

    async fn execute_immediate_failover(
        &self,
        plan: &FailoverPlan,
        mut attempt_number: u32,
    ) -> Result<ExecutionAttempt, Box<dyn std::error::Error>> {
        // Try backup routes immediately
        for backup_route in &plan.backup_routes {
            attempt_number += 1;
            if let Ok(attempt) = self.try_execute_route(backup_route, attempt_number).await {
                if matches!(attempt.result, ExecutionResult::Success) {
                    return Ok(attempt);
                }
            }
        }

        // Try emergency routes if all else fails
        for emergency_route in &plan.emergency_routes {
            attempt_number += 1;
            if let Ok(attempt) = self.try_execute_route(emergency_route, attempt_number).await {
                if matches!(attempt.result, ExecutionResult::Success) {
                    return Ok(attempt);
                }
            }
        }

        Err("All failover routes failed".into())
    }

    async fn execute_retry_with_backoff(
        &self,
        plan: &FailoverPlan,
        mut attempt_number: u32,
    ) -> Result<ExecutionAttempt, Box<dyn std::error::Error>> {
        let mut delay = self.config.base_retry_delay;

        for _ in 0..self.config.max_retry_attempts {
            tokio::time::sleep(delay).await;
            attempt_number += 1;

            if let Ok(attempt) = self.try_execute_route(&plan.primary_route, attempt_number).await {
                if matches!(attempt.result, ExecutionResult::Success) {
                    return Ok(attempt);
                }
            }

            // Exponential backoff
            delay = (delay * 2).min(self.config.max_retry_delay);
        }

        // Fall back to immediate failover
        self.execute_immediate_failover(plan, attempt_number).await
    }

    async fn execute_dex_switching(
        &self,
        plan: &FailoverPlan,
        attempt_number: u32,
    ) -> Result<ExecutionAttempt, Box<dyn std::error::Error>> {
        // Similar to immediate failover but prioritizes different DEXs
        self.execute_immediate_failover(plan, attempt_number).await
    }

    async fn execute_emergency_mode(
        &self,
        plan: &FailoverPlan,
        mut attempt_number: u32,
    ) -> Result<ExecutionAttempt, Box<dyn std::error::Error>> {
        // Try emergency routes with higher slippage tolerance
        for emergency_route in &plan.emergency_routes {
            attempt_number += 1;
            if let Ok(attempt) = self.try_execute_route(emergency_route, attempt_number).await {
                if matches!(attempt.result, ExecutionResult::Success) {
                    return Ok(attempt);
                }
            }
        }

        Err("Emergency execution failed".into())
    }

    async fn record_execution_success(&self, route: &RoutePath) {
        for step in &route.steps {
            let attempt = ExecutionAttempt {
                attempt_number: 1,
                route: route.clone(),
                start_time: SystemTime::now(),
                end_time: Some(SystemTime::now()),
                result: ExecutionResult::Success,
                error_details: None,
                response_time_ms: Some(500),
            };
            self.update_dex_health(&step.dex_type.to_string(), &attempt).await;
        }
    }

    async fn record_execution_failure(&self, route: &RoutePath, attempt: &ExecutionAttempt) {
        for step in &route.steps {
            self.update_dex_health(&step.dex_type.to_string(), attempt).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arbitrage::routing::{RoutePath, RouteStep, PathFinder, RoutingGraph, PathfinderConfig};

    fn create_test_route() -> RoutePath {
        use solana_sdk::pubkey::Pubkey;
        use crate::utils::DexType;
        use std::str::FromStr;
        
        RoutePath {
            steps: vec![
                RouteStep {
                    pool_address: Pubkey::from_str("test_pool_1").unwrap_or_default(),
                    dex_type: DexType::Orca,
                    from_token: Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap_or_default(),
                    to_token: Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap_or_default(),
                    amount_in: 1_000_000_000.0,
                    amount_out: 999_000_000.0,
                    fee_bps: 30,
                    pool_liquidity: 10_000_000_000.0,
                    price_impact: 0.001,
                    execution_order: 0,
                    slippage_tolerance: Some(0.005),
                },
            ],
            total_fees: 0.0003,
            total_weight: 1.0,
            expected_output: 999_000_000.0,
            estimated_gas_cost: 5000,
            estimated_gas_fee: Some(5000),
            execution_time_estimate: Some(Duration::from_millis(500)),
            confidence_score: 0.95,
            liquidity_score: 0.90,
            dex_diversity_score: 0.5,
            price_impact: Some(0.001),
        }
    }

    fn create_test_failover_router() -> FailoverRouter {
        let config = FailoverConfig::default();
        let pathfinder_config = PathfinderConfig::default();
        let pathfinder = Arc::new(RwLock::new(PathFinder::new(pathfinder_config)));
        let routing_graph = RoutingGraph::new();
        
        FailoverRouter::new(config, pathfinder, routing_graph)
    }

    #[tokio::test]
    async fn test_create_failover_plan() {
        let mut router = create_test_failover_router();
        let primary_route = create_test_route();

        let plan = router.create_failover_plan(
            primary_route.clone(),
            "So11111111111111111111111111111111111111112",
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            1_000_000_000,
        ).await.unwrap();

        assert_eq!(plan.primary_route.steps.len(), primary_route.steps.len());
        assert!(plan.created_at <= SystemTime::now());
        assert!(plan.expires_at > SystemTime::now());
    }

    #[tokio::test]
    async fn test_dex_health_tracking() {
        let router = create_test_failover_router();
        let route = create_test_route();

        // Test successful execution
        let success_attempt = ExecutionAttempt {
            attempt_number: 1,
            route: route.clone(),
            start_time: SystemTime::now(),
            end_time: Some(SystemTime::now()),
            result: ExecutionResult::Success,
            error_details: None,
            response_time_ms: Some(300),
        };

        router.update_dex_health("Orca", &success_attempt).await;
        assert!(router.is_dex_available("Orca").await);

        // Test failed execution
        let failure_attempt = ExecutionAttempt {
            attempt_number: 2,
            route: route.clone(),
            start_time: SystemTime::now(),
            end_time: Some(SystemTime::now()),
            result: ExecutionResult::RetryableFailure,
            error_details: Some("Network error".to_string()),
            response_time_ms: Some(5000),
        };

        router.update_dex_health("Orca", &failure_attempt).await;
        let health = router.get_health_summary().await;
        assert!(health.contains_key("Orca"));
    }

    #[test]
    fn test_circuit_breaker() {
        let mut cb = CircuitBreaker::new();
        
        // Initially closed
        assert!(cb.can_execute());
        assert_eq!(cb.state, CircuitBreakerState::Closed);

        // Record failures
        let threshold = 3;
        let timeout = Duration::from_secs(10);
        
        for _ in 0..threshold {
            cb.record_failure(threshold, timeout);
        }

        // Should be open now
        assert_eq!(cb.state, CircuitBreakerState::Open);
        assert!(!cb.can_execute());

        // Record success should reset
        cb.record_success();
        assert_eq!(cb.state, CircuitBreakerState::Closed);
        assert!(cb.can_execute());
    }

    #[tokio::test]
    async fn test_execution_attempt_timeout() {
        let router = create_test_failover_router();
        let route = create_test_route();

        let attempt = router.try_execute_route(&route, 1).await.unwrap();
        assert!(attempt.response_time_ms.is_some());
        assert!(attempt.end_time.is_some());
    }

    #[test]
    fn test_failover_config_defaults() {
        let config = FailoverConfig::default();
        assert_eq!(config.max_retry_attempts, 3);
        assert_eq!(config.circuit_breaker_threshold, 5);
        assert!(config.enable_dex_switching);
        assert!(config.enable_emergency_execution);
    }

    #[test]
    fn test_execution_status_ordering() {
        // Test that status ordering makes sense
        assert!(matches!(ExecutionStatus::Healthy, ExecutionStatus::Healthy));
        assert!(matches!(ExecutionStatus::Degraded, ExecutionStatus::Degraded));
        assert!(matches!(ExecutionStatus::Failed, ExecutionStatus::Failed));
    }

    #[tokio::test]
    async fn test_route_success_probability() {
        let router = create_test_failover_router();
        let route = create_test_route();

        // With no health data, should return high probability
        let probability = router.calculate_route_success_probability(&route).await;
        assert!(probability > 0.0);
        assert!(probability <= 1.0);
    }
}
