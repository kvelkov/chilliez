// Removed: use anyhow::Result as AnyhowResult; // This was unused
use log::{debug, error, info, warn};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::time::sleep; // Import thiserror::Error

#[derive(Debug, Clone, Error)] // Add thiserror::Error derive
pub enum ArbError {
    /// Unsupported DEX or operation
    #[error("Unsupported DEX: {0}")]
    UnsupportedDex(String),

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

    /// Instruction building errors
    #[error("Instruction Error: {0}")]
    InstructionError(String),

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

    /// Jupiter API specific errors
    #[error("Jupiter API Error: {0}")]
    JupiterApiError(String),

    /// Jupiter rate limiting error
    #[error("Jupiter API rate limit exceeded")]
    JupiterRateLimitError,

    /// Jupiter timeout error
    #[error("Jupiter API timeout: {0}")]
    JupiterTimeoutError(String),

    /// Cache/Redis errors
    #[error("Cache Error: {0}")]
    CacheError(String),

    /// Timeout errors for operations
    #[error("Timeout Error: {0}")]
    TimeoutError(String),

    /// Pool state validation errors
    #[error("Invalid Pool State: {0}")]
    InvalidPoolState(String),

    /// Invalid amount errors
    #[error("Invalid Amount: {0}")]
    InvalidAmount(String),

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

    /// Insufficient profit for trade execution
    #[error("Insufficient Profit: {0}")]
    InsufficientProfit(String),

    /// Transaction failed after retries
    #[error("Transaction Failed: {0}")]
    TransactionFailed(String),

    /// Invalid input parameters
    #[error("Invalid Input: {0}")]
    InvalidInput(String),

    /// Execution disabled
    #[error("Execution Disabled: {0}")]
    ExecutionDisabled(String),

    /// Errors that should not be retried
    #[error("Non-Recoverable Error: {0}")]
    NonRecoverable(String),

    /// Resource exhausted (rate limits, concurrency limits, etc.)
    #[error("Resource Exhausted: {0}")]
    ResourceExhausted(String),

    /// Deadlock prevention error
    #[error("Deadlock Prevention: {0}")]
    DeadlockPrevention(String),

    /// Safety mode is active, preventing operations
    #[error("Safety Mode Active: {0}")]
    SafetyModeActive(String),

    /// Account not found or not monitored
    #[error("Account Not Found: {0}")]
    AccountNotFound(String),

    /// Configuration error
    #[error("Configuration Error: {0}")]
    ConfigurationError(String),
}

// Implement From<serde_json::Error> for ArbError
impl From<serde_json::Error> for ArbError {
    fn from(err: serde_json::Error) -> Self {
        ArbError::ParseError(format!("JSON serialization/deserialization error: {}", err))
    }
}

// Implement From<anyhow::Error> for ArbError
impl From<anyhow::Error> for ArbError {
    fn from(err: anyhow::Error) -> Self {
        ArbError::ConfigError(format!("Anyhow error: {}", err))
    }
}

// Implement From<solana_client::client_error::ClientError> for ArbError
impl From<solana_client::client_error::ClientError> for ArbError {
    fn from(err: solana_client::client_error::ClientError) -> Self {
        ArbError::RpcError(format!("Solana client error: {}", err))
    }
}

impl ArbError {
    /// Determines if an error is recoverable through retry
    pub fn is_recoverable(&self) -> bool {
        match self {
            ArbError::UnsupportedDex(_) => false, // DEX not supported
            ArbError::NetworkError(_) => true,
            ArbError::WebSocketError(_) => true,
            ArbError::DexError(msg) => {
                // Some DEX errors are recoverable (temporary slippage, rate limits)
                !msg.contains("insufficient_funds") && !msg.contains("invalid_signature")
            }
            ArbError::RpcError(_) => true,
            ArbError::ParseError(_) => false, // Data format issues aren't recoverable
            ArbError::InsufficientBalance(_) => false, // Need to wait for balance
            ArbError::CircuitBreakerTriggered(_) => false, // Manual intervention needed
            ArbError::CircuitBreakerOpen => false, // Not recoverable by immediate retry
            ArbError::ConfigError(_) => false, // Config needs fixing
            ArbError::JupiterApiError(_) => true, // Jupiter API errors are usually recoverable
            ArbError::JupiterRateLimitError => true, // Rate limits are recoverable after waiting
            ArbError::JupiterTimeoutError(_) => true, // Timeouts are recoverable
            ArbError::CacheError(_) => true,  // Redis might recover
            ArbError::TimeoutError(_) => true, // Timeouts are usually recoverable
            ArbError::InvalidPoolState(_) => false, // Invalid state needs intervention
            ArbError::InvalidAmount(_) => false, // Invalid amount needs fixing
            ArbError::PoolNotFound(_) => false, // Pool not found is not recoverable by retry
            ArbError::ExecutionError(msg) => {
                // Some execution errors are recoverable (slippage, temporary network issues)
                msg.contains("slippage") || msg.contains("temporary") || msg.contains("retry")
            }
            ArbError::TransactionError(msg) => {
                // Some transaction errors are recoverable (network issues, not signature errors)
                !msg.contains("signature")
                    && !msg.contains("invalid")
                    && (msg.contains("network")
                        || msg.contains("timeout")
                        || msg.contains("congestion"))
            }
            ArbError::SimulationFailed(_) => true, // Simulations can be retried
            ArbError::Unknown(_) => true,          // Unknown errors might be recoverable
            ArbError::NonRecoverable(_) => false,  // Explicitly non-recoverable
            ArbError::ExecutionDisabled(_) => false, // Execution disabled, manual intervention needed
            ArbError::ResourceExhausted(_) => true,  // Resources might become available
            ArbError::DeadlockPrevention(_) => true, // Deadlock prevention, can retry later
            ArbError::SafetyModeActive(_) => false,  // Safety mode needs manual intervention
            ArbError::AccountNotFound(_) => false,   // Account not found, not recoverable by retry
            ArbError::ConfigurationError(_) => false, // Configuration needs fixing
            ArbError::InstructionError(_) => false,  // Instruction errors usually need code fixes
            ArbError::InsufficientProfit(_) => false, // Profit threshold not met, not recoverable by retry
            ArbError::TransactionFailed(_) => true,   // Transaction failures can be retried
            ArbError::InvalidInput(_) => false, // Invalid input needs fixing, not recoverable by retry
        }
    }

    /// Determines if operation should be retried immediately
    pub fn should_retry(&self) -> bool {
        self.is_recoverable()
            && match self {
                ArbError::NetworkError(_) => true,
                ArbError::WebSocketError(_) => true,
                ArbError::RpcError(_) => true,
                ArbError::CacheError(_) => true,
                ArbError::TimeoutError(_) => true,
                ArbError::SimulationFailed(_) => true,
                ArbError::DexError(msg) => {
                    // Retry on rate limits and temporary issues
                    msg.contains("rate_limit")
                        || msg.contains("temporary")
                        || msg.contains("timeout")
                }
                ArbError::ExecutionError(msg) => {
                    // Retry on temporary execution issues
                    msg.contains("slippage") || msg.contains("temporary")
                }
                ArbError::TransactionError(msg) => {
                    // Retry on network-related transaction issues
                    msg.contains("network") || msg.contains("timeout") || msg.contains("congestion")
                }
                ArbError::Unknown(_) => false, // Don't immediately retry unknown errors
                ArbError::InsufficientProfit(_) => false, // Profit threshold not met, no point in immediate retry
                ArbError::TransactionFailed(_) => true, // Transaction failures might be recoverable
                ArbError::InvalidInput(_) => false,     // Invalid input needs fixing first
                _ => false,
            }
    }

    /// Categorizes error for metrics and monitoring
    pub fn categorize(&self) -> ErrorCategory {
        match self {
            ArbError::UnsupportedDex(_) => ErrorCategory::Configuration,
            ArbError::NetworkError(_) | ArbError::RpcError(_) => ErrorCategory::Network,
            ArbError::WebSocketError(_) => ErrorCategory::DataFeed,
            ArbError::DexError(_) => ErrorCategory::Trading,
            ArbError::ParseError(_) => ErrorCategory::Data,
            ArbError::InsufficientBalance(_) => ErrorCategory::Balance,
            ArbError::CircuitBreakerTriggered(_) => ErrorCategory::Safety,
            ArbError::CircuitBreakerOpen => ErrorCategory::Safety,
            ArbError::ConfigError(_) => ErrorCategory::Configuration,
            ArbError::JupiterApiError(_) => ErrorCategory::Network,
            ArbError::JupiterRateLimitError => ErrorCategory::Network,
            ArbError::JupiterTimeoutError(_) => ErrorCategory::Network,
            ArbError::CacheError(_) => ErrorCategory::Infrastructure,
            ArbError::TimeoutError(_) => ErrorCategory::Network,
            ArbError::InvalidPoolState(_) => ErrorCategory::Configuration,
            ArbError::InvalidAmount(_) => ErrorCategory::Data,
            ArbError::PoolNotFound(_) => ErrorCategory::Data,
            ArbError::ExecutionError(_) => ErrorCategory::Trading,
            ArbError::TransactionError(_) => ErrorCategory::Trading,
            ArbError::SimulationFailed(_) => ErrorCategory::Trading,
            ArbError::Unknown(_) => ErrorCategory::Critical,
            ArbError::NonRecoverable(_) => ErrorCategory::Critical,
            ArbError::InstructionError(_) => ErrorCategory::Trading, // Instruction building errors
            ArbError::ExecutionDisabled(_) => ErrorCategory::Configuration,
            ArbError::ResourceExhausted(_) => ErrorCategory::Infrastructure,
            ArbError::DeadlockPrevention(_) => ErrorCategory::Infrastructure,
            ArbError::SafetyModeActive(_) => ErrorCategory::Safety,
            ArbError::AccountNotFound(_) => ErrorCategory::Configuration,
            ArbError::ConfigurationError(_) => ErrorCategory::Configuration,
            ArbError::InsufficientProfit(_) => ErrorCategory::Trading,
            ArbError::TransactionFailed(_) => ErrorCategory::Trading,
            ArbError::InvalidInput(_) => ErrorCategory::Configuration,
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
    Closed,   // Normal operation
    Open,     // Blocking all requests
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
            }
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
            warn!(
                "Circuit breaker: OPENED after {} failures",
                self.failure_count
            );
        } else {
            debug!(
                "Circuit breaker: Failure recorded ({}/{})",
                self.failure_count, self.failure_threshold
            );
        }
    }

    #[allow(dead_code)]
    pub async fn execute<F, T, E>(&mut self, operation: F) -> crate::error::Result<T>
    where
        F: std::future::Future<Output = std::result::Result<T, E>>,
        E: Into<ArbError>,
    {
        // Check if circuit breaker is open
        if self.is_open() {
            return Err(ArbError::CircuitBreakerTriggered(
                "Circuit breaker is open, operation blocked".to_string(),
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
            }
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
    pub max_attempts: u32,    // Made public
    pub base_delay: Duration, // Made public
    pub max_delay: Duration,  // Made public
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
    pub async fn execute<F, T, E, Fut>(&self, mut operation: F) -> crate::error::Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = std::result::Result<T, E>>,
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
                }
                Err(e) => {
                    let arb_error: ArbError = e.into();

                    if !arb_error.should_retry() {
                        warn!(
                            "Non-retryable error on attempt {}: {}",
                            attempt + 1,
                            arb_error
                        );
                        return Err(arb_error);
                    }

                    warn!(
                        "Attempt {} failed: {} (retrying...)",
                        attempt + 1,
                        arb_error
                    );
                    last_error = Some(arb_error);
                }
            }
        }

        error!("All {} retry attempts failed", self.max_attempts);
        Err(last_error
            .unwrap_or_else(|| ArbError::NonRecoverable("Max retries exceeded".to_string())))
    }
}

// Convenience type aliases
#[allow(dead_code)]
pub type Result<T> = std::result::Result<T, ArbError>;
