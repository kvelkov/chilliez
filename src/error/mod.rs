use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::time::error::Elapsed; // For timeout errors specifically from tokio::time::timeout

// Import fastrand if it's a direct dependency, or use another RNG like rand crate.
// For now, assuming fastrand is intended to be added or replaced.
// use fastrand; // If you `cargo add fastrand`
use rand::Rng; // Using rand crate as a common alternative

#[derive(Error, Debug)]
pub enum ArbError {
    #[error("RPC error: {0}")]
    RpcError(String),

    #[error("WebSocket error: {0}")]
    WebSocketError(String),

    #[error("DEX error: {0}")]
    DexError(String),

    #[error("Execution error: {0}")]
    ExecutionError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Lock acquisition error: {0}")] // Added LockError variant
    LockError(String),

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

    #[error("Unknown error: {0}")]
    Unknown(String),

    #[error("Failed to load keypair: {0}")]
    KeypairLoadError(String), // Added

    #[error("Cache initialization failed: {0}")]
    CacheInitializationError(String), // Added
}

// Implement From traits for common error types
impl From<solana_client::client_error::ClientError> for ArbError {
    fn from(error: solana_client::client_error::ClientError) -> Self {
        // Categorize Solana client errors
        let error_str = error.to_string();

        if error_str.contains("rate limit") || error_str.contains("429") {
            ArbError::RateLimitExceeded(error_str)
        } else if error_str.contains("timeout") || error_str.contains("timed out") {
            ArbError::TimeoutError(error_str)
        } else if error_str.contains("insufficient") || error_str.contains("liquidity") {
            ArbError::InsufficientLiquidity(error_str)
        } else if error_str.contains("simulation") {
            ArbError::SimulationFailed(error_str)
        } else if error_str.contains("congestion") || error_str.contains("busy") {
            ArbError::NetworkCongestion(error_str)
        } else {
            ArbError::RpcError(error_str)
        }
    }
}

impl From<anyhow::Error> for ArbError {
    fn from(error: anyhow::Error) -> Self {
        let error_str = error.to_string();

        // Try to categorize anyhow errors based on their message
        if error_str.contains("timeout") || error_str.contains("timed out") {
            ArbError::TimeoutError(error_str)
        } else if error_str.contains("slippage") {
            // Try to extract expected and actual values if available
            if let Some(expected) = error_str.find("expected").and_then(|idx| {
                error_str[idx..]
                    .split_whitespace()
                    .nth(1)
                    .and_then(|s| s.trim_end_matches('%').parse::<f64>().ok())
            }) {
                if let Some(actual) = error_str.find("actual").and_then(|idx| {
                    error_str[idx..]
                        .split_whitespace()
                        .nth(1)
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

// Determine if an error is recoverable
impl ArbError {
    pub fn is_recoverable(&self) -> bool {
        match self {
            ArbError::RateLimitExceeded(_) => true,
            ArbError::TimeoutError(_) => true,
            ArbError::NetworkCongestion(_) => true,
            ArbError::Recoverable(_) => true,
            ArbError::WebSocketError(_) => true, // Most WebSocket errors can be recovered with reconnection
            _ => false,
        }
    }

    pub fn should_retry(&self) -> bool {
        self.is_recoverable() && !matches!(self, ArbError::CircuitBreakerTriggered(_))
    }

    // Convert to a more specific error type based on analysis
    pub fn categorize(self) -> Self {
        match &self {
            ArbError::RpcError(msg) | ArbError::DexError(msg) | ArbError::ExecutionError(msg) => {
                if msg.contains("timeout")
                    || msg.contains("rate limit")
                    || msg.contains("congestion")
                {
                    ArbError::Recoverable(msg.clone())
                } else if msg.contains("insufficient")
                    || msg.contains("rejected")
                    || msg.contains("invalid")
                {
                    ArbError::NonRecoverable(msg.clone())
                } else {
                    self
                }
            }
            _ => self,
        }
    }
}

/// Circuit breaker for managing error thresholds and preventing cascading failures
pub struct CircuitBreaker {
    // Is the circuit currently open (preventing operations)
    is_open: AtomicBool,
    // When the circuit was opened
    opened_at: std::sync::Mutex<Option<Instant>>,
    // How long to keep the circuit open before trying again
    reset_timeout: Duration,
    // Error counter within the failure threshold window
    error_count: AtomicU64,
    // Success counter since last error
    success_count: AtomicU64,
    // Number of errors that will trigger the circuit to open
    error_threshold: u64,
    // Number of consecutive successes needed to close the circuit
    success_threshold: u64,
    // Time window for counting errors
    error_window: Duration,
    // Last error timestamp
    last_error: std::sync::Mutex<Option<Instant>>,
    // Name for logging
    name: String,
}

impl CircuitBreaker {
    pub fn new(
        name: &str,
        error_threshold: u64,
        success_threshold: u64,
        error_window: Duration,
        reset_timeout: Duration,
    ) -> Self {
        Self {
            is_open: AtomicBool::new(false),
            opened_at: std::sync::Mutex::new(None),
            reset_timeout,
            error_count: AtomicU64::new(0),
            success_count: AtomicU64::new(0),
            error_threshold,
            success_threshold,
            error_window,
            last_error: std::sync::Mutex::new(None),
            name: name.to_string(),
        }
    }

    /// Check if the circuit is open (preventing operations)
    pub fn is_open(&self) -> bool {
        self.is_open.load(Ordering::Relaxed)
    }

    /// Record a successful operation
    pub fn record_success(&self) {
        if self.is_open.load(Ordering::Relaxed) {
            // If circuit is open, increment success counter
            let success_count = self.success_count.fetch_add(1, Ordering::Relaxed) + 1;

            // Check if we've reached the success threshold to close the circuit
            if success_count >= self.success_threshold {
                log::info!(
                    "Circuit breaker '{}' closing after {} consecutive successes",
                    self.name,
                    success_count
                );
                self.is_open.store(false, Ordering::Relaxed);
                self.success_count.store(0, Ordering::Relaxed);
                self.error_count.store(0, Ordering::Relaxed);
                *self.opened_at.lock().unwrap() = None;
            }
        } else {
            // Reset error count on success when circuit is closed
            self.error_count.store(0, Ordering::Relaxed);
            self.success_count.store(0, Ordering::Relaxed);
        }
    }

    /// Record a failed operation
    pub fn record_failure(&self) -> Result<(), ArbError> {
        // Update last error timestamp
        let now = Instant::now();
        let mut last_error = self.last_error.lock().unwrap();
        *last_error = Some(now);

        // If circuit is already open, check if we should try half-open state
        if self.is_open.load(Ordering::Relaxed) {
            let opened_at = self.opened_at.lock().unwrap();
            if let Some(opened_time) = *opened_at {
                if now.duration_since(opened_time) > self.reset_timeout {
                    // Allow a single trial request (half-open state)
                    log::info!(
                        "Circuit breaker '{}' entering half-open state after timeout",
                        self.name
                    );
                    drop(opened_at); // Release lock before next operation
                    self.success_count.store(0, Ordering::Relaxed);
                    return Ok(());
                }
            }
            return Err(ArbError::CircuitBreakerTriggered(format!(
                "Circuit '{}' is open, preventing operations",
                self.name
            )));
        }

        // Check if previous errors are outside the error window
        if let Some(last_err_time) = *last_error {
            if now.duration_since(last_err_time) > self.error_window {
                // Reset error count if outside window
                self.error_count.store(1, Ordering::Relaxed);
                return Ok(());
            }
        }

        // Increment error count
        let error_count = self.error_count.fetch_add(1, Ordering::Relaxed) + 1;

        // Check if we've reached the threshold to open the circuit
        if error_count >= self.error_threshold {
            log::warn!(
                "Circuit breaker '{}' opened after {} errors within {:?}",
                self.name,
                error_count,
                self.error_window
            );
            self.is_open.store(true, Ordering::Relaxed);
            *self.opened_at.lock().unwrap() = Some(now);
            return Err(ArbError::CircuitBreakerTriggered(format!(
                "Circuit '{}' opened after {} errors",
                self.name, error_count
            )));
        }

        Ok(())
    }

    /// Execute a function with circuit breaker protection
    pub async fn execute<F, T, E>(&self, f: F) -> Result<T, ArbError>
    where
        F: FnOnce() -> Result<T, E>,
        E: Into<ArbError>,
    {
        if self.is_open() {
            let opened_at = self.opened_at.lock().unwrap();
            if let Some(opened_time) = *opened_at {
                let now = Instant::now();
                if now.duration_since(opened_time) > self.reset_timeout {
                    // Allow a single trial request (half-open state)
                    log::info!("Circuit breaker '{}' attempting half-open state", self.name);
                    drop(opened_at); // Release lock before next operation
                } else {
                    return Err(ArbError::CircuitBreakerTriggered(format!(
                        "Circuit '{}' is open, preventing operations",
                        self.name
                    )));
                }
            }
        }

        match f() {
            Ok(result) => {
                self.record_success();
                Ok(result)
            }
            Err(e) => {
                let arb_error = e.into();
                if let Err(cb_error) = self.record_failure() {
                    return Err(cb_error);
                }
                Err(arb_error)
            }
        }
    }

    /// Reset the circuit breaker to its initial state
    pub fn reset(&self) {
        self.is_open.store(false, Ordering::Relaxed);
        self.error_count.store(0, Ordering::Relaxed);
        self.success_count.store(0, Ordering::Relaxed);
        *self.opened_at.lock().unwrap() = None;
        *self.last_error.lock().unwrap() = None;
        log::info!("Circuit breaker '{}' has been reset", self.name);
    }
}

/// Retry policy with exponential backoff and jitter
pub struct RetryPolicy {
    // Maximum number of retry attempts
    max_attempts: u32,
    // Base delay for exponential backoff (in milliseconds)
    base_delay_ms: u64,
    // Maximum delay cap (in milliseconds)
    max_delay_ms: u64,
    // Jitter factor (0.0 to 1.0) to add randomness to delays
    jitter_factor: f64,
}

impl RetryPolicy {
    pub fn new(
        max_attempts: u32,
        base_delay_ms: u64,
        max_delay_ms: u64,
        jitter_factor: f64,
    ) -> Self {
        Self {
            max_attempts,
            base_delay_ms,
            max_delay_ms,
            jitter_factor: jitter_factor.clamp(0.0, 1.0),
        }
    }

    /// Calculate delay for a given attempt with exponential backoff and jitter
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::from_millis(0);
        }

        // Calculate exponential backoff
        let exp_backoff = self.base_delay_ms * 2u64.saturating_pow(attempt - 1);

        // Apply maximum delay cap
        let capped_delay = exp_backoff.min(self.max_delay_ms);

        // Apply jitter
        let jitter_range = (capped_delay as f64 * self.jitter_factor) as u64;
        let jitter = if jitter_range > 0 {
            // fastrand::u64(0..jitter_range)
            let mut rng = rand::thread_rng();
            rng.gen_range(0..jitter_range)
        } else {
            0
        };

        Duration::from_millis(capped_delay - (jitter_range / 2) + jitter)
    }

    /// Execute a function with retry logic
    pub async fn execute<F, Fut, T>(&self, f: F) -> Result<T, ArbError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, ArbError>>,
    {
        let mut attempt = 0;

        loop {
            attempt += 1;

            match f().await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    // Check if we should retry based on error type
                    if !e.should_retry() {
                        return Err(e);
                    }

                    // Check if we've exceeded max attempts
                    if attempt >= self.max_attempts {
                        return Err(ArbError::Recoverable(format!(
                            "Max retry attempts ({}) exceeded: {}",
                            self.max_attempts, e
                        )));
                    }

                    // Calculate delay with jitter
                    let delay = self.delay_for_attempt(attempt);

                    log::info!(
                        "Retrying after error (attempt {}/{}), waiting {:?}: {}",
                        attempt,
                        self.max_attempts,
                        delay,
                        e
                    );

                    // Wait before retrying
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }
}
