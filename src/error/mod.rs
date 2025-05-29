// Removed: use anyhow::Result as AnyhowResult; // This was unused
use std::time::{Duration, Instant};
use tokio::time::sleep;
use log::{error, warn, info, debug};
use thiserror::Error; // Import thiserror::Error

#[derive(Debug, Clone, Error)] // Add thiserror::Error derive
pub enum ArbError {
    /// Network/connectivity issues
    #[error("Network Error: {0}")]
    NetworkError(String),
    
    /// WebSocket connection/data issues - CRITICAL for real-time data
    #[error("WebSocket Error: {0}")]
    WebSocketError(String),
    
    /// DEX-specific errors (slippage, insufficient liquidity, etc.)
    #[error("DEX Error: {0}")]
    DexError(String),
    
    /// RPC/Solana network errors
    #[error("RPC Error: {0}")]
    RpcError(String),
    
    /// Parsing errors for pool data
    #[error("Parse Error: {0}")]
    ParseError(String),
    
    /// Insufficient balance for trade execution
    #[error("Insufficient Balance: {0}")]
    InsufficientBalance(String),
    
    /// Circuit breaker has been triggered
    #[error("Circuit Breaker Triggered: {0}")]
    CircuitBreakerTriggered(String),
    
    #[error("Circuit breaker is open, operation blocked")] // This attribute comes from thiserror
    CircuitBreakerOpen,
    
    /// Configuration errors
    #[error("Config Error: {0}")]
    ConfigError(String),
    
    /// Cache/Redis errors
    #[error("Cache Error: {0}")]
    CacheError(String),
    
    /// Timeout errors for operations
    #[error("Timeout Error: {0}")]
    TimeoutError(String),
    
    /// Pool not found errors
    #[error("Pool Not Found: {0}")]
    PoolNotFound(String),
    
    /// Trade execution errors
    #[error("Execution Error: {0}")]
    ExecutionError(String),
    
    /// Transaction/blockchain errors
    #[error("Transaction Error: {0}")]
    TransactionError(String),
    
    /// Simulation failed errors
    #[error("Simulation Failed: {0}")]
    SimulationFailed(String),
    
    /// Unknown/unclassified errors
    #[error("Unknown Error: {0}")]
    Unknown(String),
    
    /// Errors that should not be retried
    #[error("Non-Recoverable Error: {0}")]
    NonRecoverable(String),
}

// Implement From<serde_json::Error> for ArbError
impl From<serde_json::Error> for ArbError {
    fn from(err: serde_json::Error) -> Self {
        ArbError::ParseError(format!("JSON serialization/deserialization error: {}", err))
    }
}

impl ArbError {
    /// Determines if an error is recoverable through retry
    pub fn is_recoverable(&self) -> bool {
        match self {
            ArbError::NetworkError(_) => true,
            ArbError::WebSocketError(_) => true,
            ArbError::DexError(msg) => {
                // Some DEX errors are recoverable (temporary slippage, rate limits)
                !msg.contains("insufficient_funds") && !msg.contains("invalid_signature")
            },
            ArbError::RpcError(_) => true,
            ArbError::ParseError(_) => false, // Data format issues aren't recoverable
            ArbError::InsufficientBalance(_) => false, // Need to wait for balance
            ArbError::CircuitBreakerTriggered(_) => false, // Manual intervention needed
            ArbError::CircuitBreakerOpen => false, // Not recoverable by immediate retry
            ArbError::ConfigError(_) => false, // Config needs fixing
            ArbError::CacheError(_) => true, // Redis might recover
            ArbError::TimeoutError(_) => true, // Timeouts are usually recoverable
            ArbError::PoolNotFound(_) => false, // Pool doesn't exist
            ArbError::ExecutionError(msg) => {
                // Some execution errors are recoverable (slippage, temporary network issues)
                msg.contains("slippage") || msg.contains("temporary") || msg.contains("retry")
            },
            ArbError::TransactionError(msg) => {
                // Some transaction errors are recoverable (network issues, not signature errors)
                !msg.contains("signature") && !msg.contains("invalid") && 
                (msg.contains("network") || msg.contains("timeout") || msg.contains("congestion"))
            },
            ArbError::SimulationFailed(_) => true, // Simulations can be retried
            ArbError::Unknown(_) => true, // Unknown errors might be recoverable
            ArbError::NonRecoverable(_) => false,
        }
    }

    /// Determines if operation should be retried immediately
    pub fn should_retry(&self) -> bool {
        self.is_recoverable() && match self {
            ArbError::NetworkError(_) => true,
            ArbError::WebSocketError(_) => true,
            ArbError::RpcError(_) => true,
            ArbError::CacheError(_) => true,
            ArbError::TimeoutError(_) => true,
            ArbError::SimulationFailed(_) => true,
            ArbError::DexError(msg) => {
                // Retry on rate limits and temporary issues
                msg.contains("rate_limit") || msg.contains("temporary") || msg.contains("timeout")
            },
            ArbError::ExecutionError(msg) => {
                // Retry on temporary execution issues
                msg.contains("slippage") || msg.contains("temporary")
            },
            ArbError::TransactionError(msg) => {
                // Retry on network-related transaction issues
                msg.contains("network") || msg.contains("timeout") || msg.contains("congestion")
            },
            ArbError::Unknown(_) => false, // Don't immediately retry unknown errors
            _ => false,
        }
    }

    /// Categorizes error for metrics and monitoring
    pub fn categorize(&self) -> ErrorCategory {
        match self {
            ArbError::NetworkError(_) | ArbError::RpcError(_) => ErrorCategory::Network,
            ArbError::WebSocketError(_) => ErrorCategory::DataFeed,
            ArbError::DexError(_) => ErrorCategory::Trading,
            ArbError::ParseError(_) => ErrorCategory::Data,
            ArbError::InsufficientBalance(_) => ErrorCategory::Balance,
            ArbError::CircuitBreakerTriggered(_) => ErrorCategory::Safety,
            ArbError::CircuitBreakerOpen => ErrorCategory::Safety,
            ArbError::ConfigError(_) => ErrorCategory::Configuration,
            ArbError::CacheError(_) => ErrorCategory::Infrastructure,
            ArbError::TimeoutError(_) => ErrorCategory::Network,
            ArbError::PoolNotFound(_) => ErrorCategory::Data,
            ArbError::ExecutionError(_) => ErrorCategory::Trading,
            ArbError::TransactionError(_) => ErrorCategory::Trading,
            ArbError::SimulationFailed(_) => ErrorCategory::Trading,
            ArbError::Unknown(_) => ErrorCategory::Critical,
            ArbError::NonRecoverable(_) => ErrorCategory::Critical,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ErrorCategory {
    Network,
    DataFeed,
    Trading,
    Data,
    Balance,
    Safety,
    Configuration,
    Infrastructure,
    Critical,
}

/// Circuit breaker for protecting against cascading failures
#[derive(Debug, Clone)]
#[allow(dead_code)] // Struct and its fields/methods are not yet fully integrated
pub struct CircuitBreaker {
    failure_threshold: u32,
    recovery_timeout: Duration,
    failure_count: u32,
    last_failure_time: Option<Instant>,
    state: CircuitBreakerState,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // Enum and its variants are not yet fully integrated
enum CircuitBreakerState {
    Closed,  // Normal operation
    Open,    // Blocking all requests
    HalfOpen, // Testing if service recovered
}

impl CircuitBreaker {
    #[allow(dead_code)]
    pub fn new(failure_threshold: u32, recovery_timeout: Duration) -> Self {
        Self {
            failure_threshold,
            recovery_timeout,
            failure_count: 0,
            last_failure_time: None,
            state: CircuitBreakerState::Closed,
        }
    }

    #[allow(dead_code)]
    pub fn is_open(&self) -> bool {
        match self.state {
            CircuitBreakerState::Open => {
                if let Some(last_failure) = self.last_failure_time {
                    // Check if recovery timeout has passed
                    if last_failure.elapsed() >= self.recovery_timeout {
                        false // Allow transition to half-open
                    } else {
                        true // Still in open state
                    }
                } else {
                    false
                }
            },
            _ => false,
        }
    }

    #[allow(dead_code)]
    pub fn record_success(&mut self) {
        self.failure_count = 0;
        self.state = CircuitBreakerState::Closed;
        debug!("Circuit breaker: Success recorded, state reset to Closed");
    }

    #[allow(dead_code)]
    pub fn record_failure(&mut self) {
        self.failure_count += 1;
        self.last_failure_time = Some(Instant::now());
        
        if self.failure_count >= self.failure_threshold {
            self.state = CircuitBreakerState::Open;
            warn!("Circuit breaker: OPENED after {} failures", self.failure_count);
        } else {
            debug!("Circuit breaker: Failure recorded ({}/{})", self.failure_count, self.failure_threshold);
        }
    }

    #[allow(dead_code)]
    pub async fn execute<F, T, E>(&mut self, operation: F) -> Result<T, ArbError>
    where
        F: std::future::Future<Output = Result<T, E>>,
        E: Into<ArbError>,
    {
        // Check if circuit breaker is open
        if self.is_open() {
            return Err(ArbError::CircuitBreakerTriggered(
                "Circuit breaker is open, operation blocked".to_string()
            ));
        }

        // If half-open, transition to testing state
        if self.state == CircuitBreakerState::Open {
            if let Some(last_failure) = self.last_failure_time {
                if last_failure.elapsed() >= self.recovery_timeout {
                    self.state = CircuitBreakerState::HalfOpen;
                    info!("Circuit breaker: Transitioning to HalfOpen for testing");
                }
            }
        }

        // Execute operation
        match operation.await {
            Ok(result) => {
                self.record_success();
                Ok(result)
            },
            Err(e) => {
                let arb_error = e.into();
                self.record_failure();
                Err(arb_error)
            }
        }
    }

    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.failure_count = 0;
        self.last_failure_time = None;
        self.state = CircuitBreakerState::Closed;
        info!("Circuit breaker: Manually reset to Closed state");
    }
}

/// Retry policy with exponential backoff
#[derive(Debug, Clone)]
#[allow(dead_code)] // Struct and its fields/methods are not yet fully integrated
pub struct RetryPolicy {
    pub max_attempts: u32,     // Made public
    pub base_delay: Duration,      // Made public
    pub max_delay: Duration,       // Made public
}

impl RetryPolicy {
    #[allow(dead_code)]
    pub fn new(max_attempts: u32, base_delay: Duration, max_delay: Duration) -> Self {
        Self {
            max_attempts,
            base_delay,
            max_delay,
        }
    }

    /// Calculate delay for a given attempt (exponential backoff)
    #[allow(dead_code)]
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::from_millis(0);
        }
        
        let delay_ms = self.base_delay.as_millis() * (2_u128.pow(attempt - 1));
        let delay = Duration::from_millis(delay_ms.min(self.max_delay.as_millis()) as u64);
        
        debug!("Retry attempt {}: delay = {:?}", attempt, delay);
        delay
    }

    /// Execute operation with retry logic
    #[allow(dead_code)]
    pub async fn execute<F, T, E, Fut>(&self, mut operation: F) -> Result<T, ArbError>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
        E: Into<ArbError>,
    {
        let mut last_error = None;
        
        for attempt in 0..self.max_attempts {
            if attempt > 0 {
                let delay = self.delay_for_attempt(attempt);
                sleep(delay).await;
            }
            
            match operation().await {
                Ok(result) => {
                    if attempt > 0 {
                        info!("Operation succeeded after {} retries", attempt);
                    }
                    return Ok(result);
                },
                Err(e) => {
                    let arb_error = e.into();
                    
                    if !arb_error.should_retry() {
                        warn!("Non-retryable error on attempt {}: {}", attempt + 1, arb_error);
                        return Err(arb_error);
                    }
                    
                    warn!("Attempt {} failed: {} (retrying...)", attempt + 1, arb_error);
                    last_error = Some(arb_error);
                }
            }
        }
        
        error!("All {} retry attempts failed", self.max_attempts);
        Err(last_error.unwrap_or_else(|| {
            ArbError::NonRecoverable("Max retries exceeded".to_string())
        }))
    }
}

// Convenience type aliases
#[allow(dead_code)] // Type alias is not yet used
pub type ArbResult<T> = Result<T, ArbError>;

// Default configurations
impl Default for RetryPolicy {
    fn default() -> Self {
        Self::new(
            3, // max attempts
            Duration::from_millis(100), // base delay
            Duration::from_secs(5), // max delay
        )
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new(
            5, // failure threshold
            Duration::from_secs(30), // recovery timeout
        )
    }
}
