use anyhow::Result as AnyhowResult;
use std::fmt;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use log::{error, warn, info, debug};

#[derive(Debug, Clone)]
pub enum ArbError {
    /// Network/connectivity issues
    NetworkError(String),
    
    /// WebSocket connection/data issues - CRITICAL for real-time data
    WebSocketError(String),
    
    /// DEX-specific errors (slippage, insufficient liquidity, etc.)
    DexError(String),
    
    /// RPC/Solana network errors
    RpcError(String),
    
    /// Parsing errors for pool data
    ParseError(String),
    
    /// Insufficient balance for trade execution
    InsufficientBalance(String),
    
    /// Circuit breaker has been triggered
    CircuitBreakerTriggered(String),
    
    /// Configuration errors
    ConfigError(String),
    
    /// Cache/Redis errors
    CacheError(String),
    
    /// Timeout errors for operations
    TimeoutError(String),
    
    /// Pool not found errors
    PoolNotFound(String),
    
    /// Trade execution errors
    ExecutionError(String),
    
    /// Transaction/blockchain errors
    TransactionError(String),
    
    /// Simulation failed errors
    SimulationFailed(String),
    
    /// Unknown/unclassified errors
    Unknown(String),
    
    /// Errors that should not be retried
    NonRecoverable(String),
}

impl fmt::Display for ArbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArbError::NetworkError(msg) => write!(f, "Network Error: {}", msg),
            ArbError::WebSocketError(msg) => write!(f, "WebSocket Error: {}", msg),
            ArbError::DexError(msg) => write!(f, "DEX Error: {}", msg),
            ArbError::RpcError(msg) => write!(f, "RPC Error: {}", msg),
            ArbError::ParseError(msg) => write!(f, "Parse Error: {}", msg),
            ArbError::InsufficientBalance(msg) => write!(f, "Insufficient Balance: {}", msg),
            ArbError::CircuitBreakerTriggered(msg) => write!(f, "Circuit Breaker Triggered: {}", msg),
            ArbError::ConfigError(msg) => write!(f, "Config Error: {}", msg),
            ArbError::CacheError(msg) => write!(f, "Cache Error: {}", msg),
            ArbError::TimeoutError(msg) => write!(f, "Timeout Error: {}", msg),
            ArbError::PoolNotFound(msg) => write!(f, "Pool Not Found: {}", msg),
            ArbError::ExecutionError(msg) => write!(f, "Execution Error: {}", msg),
            ArbError::TransactionError(msg) => write!(f, "Transaction Error: {}", msg),
            ArbError::SimulationFailed(msg) => write!(f, "Simulation Failed: {}", msg),
            ArbError::Unknown(msg) => write!(f, "Unknown Error: {}", msg),
            ArbError::NonRecoverable(msg) => write!(f, "Non-Recoverable Error: {}", msg),
        }
    }
}

impl std::error::Error for ArbError {}

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
pub struct CircuitBreaker {
    failure_threshold: u32,
    recovery_timeout: Duration,
    failure_count: u32,
    last_failure_time: Option<Instant>,
    state: CircuitBreakerState,
}

#[derive(Debug, Clone, PartialEq)]
enum CircuitBreakerState {
    Closed,  // Normal operation
    Open,    // Blocking all requests
    HalfOpen, // Testing if service recovered
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, recovery_timeout: Duration) -> Self {
        Self {
            failure_threshold,
            recovery_timeout,
            failure_count: 0,
            last_failure_time: None,
            state: CircuitBreakerState::Closed,
        }
    }

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

    pub fn record_success(&mut self) {
        self.failure_count = 0;
        self.state = CircuitBreakerState::Closed;
        debug!("Circuit breaker: Success recorded, state reset to Closed");
    }

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

    pub fn reset(&mut self) {
        self.failure_count = 0;
        self.last_failure_time = None;
        self.state = CircuitBreakerState::Closed;
        info!("Circuit breaker: Manually reset to Closed state");
    }
}

/// Retry policy with exponential backoff
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    max_attempts: u32,
    base_delay: Duration,
    max_delay: Duration,
}

impl RetryPolicy {
    pub fn new(max_attempts: u32, base_delay: Duration, max_delay: Duration) -> Self {
        Self {
            max_attempts,
            base_delay,
            max_delay,
        }
    }

    /// Calculate delay for a given attempt (exponential backoff)
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
