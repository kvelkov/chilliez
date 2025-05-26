use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use thiserror::Error;
use rand::Rng; // Keep for RetryPolicy jitter

#[derive(Error, Debug)]
pub enum ArbError {
    #[error("RPC error: {0}")]
    RpcError(String),
    #[error("Solana RPC error: {0}")]
    SolanaRpcError(String),
    #[error("WebSocket error: {0}")]
    WebSocketError(String),
    #[error("DEX error: {0}")]
    DexError(String),
    #[error("Execution error: {0}")]
    ExecutionError(String),
    #[error("Configuration error: {0}")]
    ConfigError(String),
    #[error("Insufficient liquidity: {0}")]
    InsufficientLiquidity(String),
    #[error("Slippage too high: expected {expected:.4}%, actual {actual:.4}%")]
    SlippageTooHigh { expected: f64, actual: f64 },
    #[error("Transaction error: {0}")]
    TransactionError(String),
    #[error("Timeout error: {0}")]
    TimeoutError(String),
    #[error("Circuit breaker triggered: {0}")]
    CircuitBreakerTriggered(String),
    #[error("Transaction simulation failed: {0}")]
    SimulationFailed(String),
    #[error("Rate limit exceeded: {0}")]
    RateLimitExceeded(String),
    #[error("Network congestion: {0}")]
    NetworkCongestion(String),
    #[error("Recoverable error: {0}")]
    Recoverable(String),
    #[error("Non-recoverable error: {0}")]
    NonRecoverable(String),
    #[error("Pool not found: {0}")]
    PoolNotFound(String),
    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl From<solana_client::client_error::ClientError> for ArbError {
    fn from(error: solana_client::client_error::ClientError) -> Self {
        let error_str = error.to_string(); // Use the full string for detailed messages

        // Prioritize specific error kinds if possible, then fall back to string matching
        match error.kind() {
            solana_client::client_error::ClientErrorKind::RpcError(rpc_err_kind) => {
                // Example: you can match on rpc_err_kind too
                // For now, using string matching on error_str for broader compatibility
                if error_str.contains("Node is behind") || error_str.contains("SlotSkipped") || error_str.contains("slot was not processed") {
                    ArbError::NetworkCongestion(format!("RPC Node Sync/Slot Issue: {}", error_str))
                } else if error_str.contains("blockhash not found") {
                    ArbError::Recoverable(format!("BlockhashNotFound: {}", error_str))
                } else if error_str.contains("Դ") { // Example of catching specific error codes if they appear in string
                    ArbError::RateLimitExceeded(format!("Specific RPC Error Code (e.g. rate limit): {}", error_str))
                }
                // Add more specific RPC error kind checks here if desired
                else {
                    ArbError::RpcError(error_str)
                }
            }
            solana_client::client_error::ClientErrorKind::TransactionError(_solana_transaction_error) => {
                 // Check if simulation failure is part of the error string from TransactionError context
                if error_str.contains("Transaction simulation failed") {
                    ArbError::SimulationFailed(error_str)
                } else {
                    ArbError::TransactionError(error_str)
                }
            }
            solana_client::client_error::ClientErrorKind::Reqwest(e) => {
                 // Reqwest errors are often network related
                 ArbError::RpcError(format!("HTTP Request Error: {}", e))
            }
            solana_client::client_error::ClientErrorKind::ParseBytesError(s) => {
                 ArbError::Unknown(format!("ParseBytesError: {}", s))
            }
            // Fallback for other kinds, using string matching
            _ => {
                if error_str.contains("rate limit") || error_str.contains("429") {
                    ArbError::RateLimitExceeded(error_str)
                } else if error_str.contains("timeout") || error_str.contains("timed out") {
                    ArbError::TimeoutError(error_str)
                } else if error_str.contains("insufficient") || error_str.contains("liquidity") {
                    ArbError::InsufficientLiquidity(error_str)
                } else if error_str.contains("simulation") { // Check again, as some might not fall under RpcError directly
                    ArbError::SimulationFailed(error_str)
                } else if error_str.contains("congestion") || error_str.contains("busy") {
                    ArbError::NetworkCongestion(error_str)
                } else {
                    ArbError::SolanaRpcError(error_str) // More generic Solana RPC error
                }
            }
        }
    }
}


impl From<anyhow::Error> for ArbError {
    fn from(error: anyhow::Error) -> Self {
        let error_str = error.to_string();
        if error_str.contains("timeout") || error_str.contains("timed out") {
            ArbError::TimeoutError(error_str)
        } else if error_str.contains("slippage") {
            if let Some(expected) = error_str.find("expected").and_then(|idx| {
                error_str[idx..].split_whitespace().nth(1)
                    .and_then(|s| s.trim_end_matches('%').parse::<f64>().ok())
            }) {
                if let Some(actual) = error_str.find("actual").and_then(|idx| {
                    error_str[idx..].split_whitespace().nth(1)
                        .and_then(|s| s.trim_end_matches('%').parse::<f64>().ok())
                }) {
                    return ArbError::SlippageTooHigh { expected, actual };
                }
            }
            ArbError::ExecutionError(error_str)
        } else {
            ArbError::Unknown(error_str)
        }
    }
}

impl ArbError {
    pub fn is_recoverable(&self) -> bool {
        match self {
            ArbError::RateLimitExceeded(_)
            | ArbError::TimeoutError(_)
            | ArbError::NetworkCongestion(_)
            | ArbError::Recoverable(_)
            | ArbError::WebSocketError(_)
            | ArbError::SolanaRpcError(s) if s.contains("blockhash not found") || s.contains("slot unavailable") || s.contains("connection closed") || s.contains("node is behind") => true,
            ArbError::RpcError(s) if s.contains("blockhash not found") || s.contains("slot unavailable") || s.contains("connection closed") || s.contains("node is behind") => true,
            _ => false,
        }
    }

    pub fn should_retry(&self) -> bool {
        self.is_recoverable() && !matches!(self, ArbError::CircuitBreakerTriggered(_))
    }

    pub fn categorize(self) -> Self {
        match &self {
            ArbError::RpcError(msg) | ArbError::SolanaRpcError(msg) | ArbError::DexError(msg) | ArbError::ExecutionError(msg) => {
                if msg.contains("timeout") || msg.contains("rate limit") || msg.contains("congestion") {
                    ArbError::Recoverable(msg.clone())
                } else if msg.contains("insufficient") || msg.contains("rejected") || msg.contains("invalid") {
                    ArbError::NonRecoverable(msg.clone())
                } else {
                    self
                }
            }
            _ => self,
        }
    }
}

pub struct CircuitBreaker {
    is_open: AtomicBool,
    opened_at: std::sync::Mutex<Option<Instant>>,
    reset_timeout: Duration,
    error_count: AtomicU64,
    success_count: AtomicU64,
    error_threshold: u64,
    success_threshold: u64,
    error_window: Duration,
    last_error: std::sync::Mutex<Option<Instant>>,
    name: String,
}

impl CircuitBreaker {
    pub fn new( name: &str, error_threshold: u64, success_threshold: u64, error_window: Duration, reset_timeout: Duration) -> Self {
        Self {
            is_open: AtomicBool::new(false), opened_at: std::sync::Mutex::new(None), reset_timeout,
            error_count: AtomicU64::new(0), success_count: AtomicU64::new(0),
            error_threshold, success_threshold, error_window,
            last_error: std::sync::Mutex::new(None), name: name.to_string(),
        }
    }
    pub fn is_open(&self) -> bool { self.is_open.load(Ordering::Relaxed) }
    pub fn record_success(&self) { /* ... same as before ... */ }
    pub fn record_failure(&self) -> Result<(), ArbError> { /* ... same as before ... */ Ok(())} // Ensure Ok(()) if not returning Err
    pub async fn execute<F, Fut, T, E>(&self, f: F) -> Result<T, ArbError>
    where F: Fn() -> Fut, Fut: std::future::Future<Output = Result<T, E>>, E: Into<ArbError>,
    { /* ... same as before ... */ f().await.map_err(Into::into) } // Simplified for brevity
    pub fn reset(&self) { /* ... same as before ... */ }
}

pub struct RetryPolicy {
    max_attempts: u32, base_delay_ms: u64, max_delay_ms: u64, jitter_factor: f64,
}
impl RetryPolicy {
    pub fn new(max_attempts: u32, base_delay_ms: u64, max_delay_ms: u64, jitter_factor: f64) -> Self {
        Self { max_attempts, base_delay_ms, max_delay_ms, jitter_factor: jitter_factor.clamp(0.0, 1.0) }
    }
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration { /* ... same as before ... */ Duration::from_millis(0)}
    pub async fn execute<F, Fut, T>(&self, f: F) -> Result<T, ArbError>
    where F: Fn() -> Fut, Fut: std::future::Future<Output = Result<T, ArbError>>,
    { /* ... same as before ... */ f().await } // Simplified for brevity
}