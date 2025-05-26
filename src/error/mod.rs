use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use thiserror::Error;
use rand::Rng;

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

// Corrected From<solana_client::client_error::ClientError> for ArbError
impl From<solana_client::client_error::ClientError> for ArbError {
    fn from(error: solana_client::client_error::ClientError) -> Self {
        let error_kind = error.kind(); // Get the ClientErrorKind
        let error_str = error.to_string(); // Keep the full string for details

        // Match on the kind for categorization
        match error_kind {
            solana_client::client_error::ClientErrorKind::RpcError(rpc_err) => {
                // Further inspect rpc_err if needed, e.g. rpc_err.to_string()
                if error_str.contains("rate limit") || error_str.contains("429") {
                    ArbError::RateLimitExceeded(error_str)
                } else if error_str.contains("timeout") || error_str.contains("timed out") {
                    ArbError::TimeoutError(error_str)
                } else if error_str.contains("insufficient") || error_str.contains("liquidity") {
                    ArbError::InsufficientLiquidity(error_str)
                } else if error_str.contains("simulation") { // This might be part of RpcResponseError
                    ArbError::SimulationFailed(error_str)
                } else if error_str.contains("congestion") || error_str.contains("busy") {
                    ArbError::NetworkCongestion(error_str)
                } else if error_str.contains("blockhash not found") || error_str.contains("slot unavailable") {
                     ArbError::Recoverable(error_str) // Specific recoverable RPC issues
                }else {
                    ArbError::RpcError(error_str) // General RPC error
                }
            }
            solana_client::client_error::ClientErrorKind::TransactionError(_tx_err) => {
                // Transaction errors are often specific and might indicate issues like
                //Lamport balance below rent-exempt threshold, Program execution error, etc.
                // These are often non-recoverable in the short term for the same transaction.
                ArbError::TransactionError(error_str)
            }
            // Handle other ClientErrorKind variants as needed
            _ => ArbError::SolanaRpcError(error_str), // Default to SolanaRpcError
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
            | ArbError::SolanaRpcError(s) if s.contains("blockhash not found") || s.contains("slot unavailable") || s.contains("connection closed") => true,
            ArbError::RpcError(s) if s.contains("blockhash not found") || s.contains("slot unavailable") || s.contains("connection closed") => true,
            _ => false,
        }
    }

    pub fn should_retry(&self) -> bool {
        self.is_recoverable() && !matches!(self, ArbError::CircuitBreakerTriggered(_))
    }

    pub fn categorize(self) -> Self { // Keep this as is for now
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
    pub fn record_success(&self) {
        if self.is_open.load(Ordering::Relaxed) {
            let success_count_val = self.success_count.fetch_add(1, Ordering::Relaxed) + 1;
            if success_count_val >= self.success_threshold {
                log::info!("Circuit breaker '{}' closing after {} consecutive successes", self.name, success_count_val);
                self.is_open.store(false, Ordering::Relaxed);
                self.success_count.store(0, Ordering::Relaxed);
                self.error_count.store(0, Ordering::Relaxed);
                *self.opened_at.lock().unwrap() = None;
            }
        } else {
            self.error_count.store(0, Ordering::Relaxed); self.success_count.store(0, Ordering::Relaxed);
        }
    }
    pub fn record_failure(&self) -> Result<(), ArbError> {
        let now = Instant::now();
        *self.last_error.lock().unwrap() = Some(now);

        if self.is_open.load(Ordering::Relaxed) {
            let mut opened_at_guard = self.opened_at.lock().unwrap();
            if let Some(opened_time) = *opened_at_guard {
                if now.duration_since(opened_time) > self.reset_timeout {
                    log::info!("Circuit breaker '{}' entering half-open state after timeout", self.name);
                    self.success_count.store(0, Ordering::Relaxed);
                } else {
                    return Err(ArbError::CircuitBreakerTriggered(format!("Circuit '{}' is open", self.name)));
                }
            } else { // Should not happen if is_open is true
                 *opened_at_guard = Some(now); // Initialize if it was None
            }
        }
        
        let current_error_count = self.error_count.fetch_add(1, Ordering::Relaxed) + 1;
        if current_error_count >= self.error_threshold {
            log::warn!("Circuit breaker '{}' opened after {} errors.", self.name, current_error_count);
            self.is_open.store(true, Ordering::Relaxed);
            *self.opened_at.lock().unwrap() = Some(now);
            return Err(ArbError::CircuitBreakerTriggered(format!("Circuit '{}' opened after {} errors", self.name, current_error_count)));
        }
        Ok(())
    }
    pub async fn execute<F, Fut, T, E>(&self, f: F) -> Result<T, ArbError>
    where F: Fn() -> Fut, Fut: std::future::Future<Output = Result<T, E>>, E: Into<ArbError>,
    {
        if self.is_open.load(Ordering::Relaxed) {
            let opened_at_guard = self.opened_at.lock().unwrap();
            if let Some(opened_time) = *opened_at_guard {
                if Instant::now().duration_since(opened_time) > self.reset_timeout {
                    log::info!("Circuit breaker '{}' attempting half-open state execution.", self.name);
                } else {
                    return Err(ArbError::CircuitBreakerTriggered(format!("Circuit '{}' is open.", self.name)));
                }
            } else {
                return Err(ArbError::CircuitBreakerTriggered(format!("Circuit '{}' is open but opened_at is None.", self.name)));
            }
        }
        match f().await {
            Ok(result) => { self.record_success(); Ok(result) }
            Err(e) => {
                let arb_error: ArbError = e.into();
                if let Err(cb_err) = self.record_failure() { Err(cb_err) } else { Err(arb_error) }
            }
        }
    }
    pub fn reset(&self) {
        self.is_open.store(false, Ordering::Relaxed); self.error_count.store(0, Ordering::Relaxed);
        self.success_count.store(0, Ordering::Relaxed); *self.opened_at.lock().unwrap() = None;
        *self.last_error.lock().unwrap() = None;
        log::info!("Circuit breaker '{}' has been reset.", self.name);
    }
}

pub struct RetryPolicy {
    max_attempts: u32, base_delay_ms: u64, max_delay_ms: u64, jitter_factor: f64,
}
impl RetryPolicy {
    pub fn new(max_attempts: u32, base_delay_ms: u64, max_delay_ms: u64, jitter_factor: f64) -> Self {
        Self { max_attempts, base_delay_ms, max_delay_ms, jitter_factor: jitter_factor.clamp(0.0, 1.0) }
    }
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if attempt == 0 { return Duration::from_millis(0); }
        let exp_backoff = self.base_delay_ms.saturating_mul(2u64.saturating_pow(attempt.saturating_sub(1)));
        let capped_delay = exp_backoff.min(self.max_delay_ms);
        let jitter_range = (capped_delay as f64 * self.jitter_factor) as u64;
        let jitter = if jitter_range > 0 { rand::thread_rng().gen_range(0..=jitter_range) } else { 0 };
        Duration::from_millis(capped_delay.saturating_sub(jitter_range / 2).saturating_add(jitter))
    }
    pub async fn execute<F, Fut, T>(&self, f: F) -> Result<T, ArbError>
    where F: Fn() -> Fut, Fut: std::future::Future<Output = Result<T, ArbError>>,
    {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match f().await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    if !e.should_retry() { return Err(e); }
                    if attempt >= self.max_attempts {
                        return Err(ArbError::Recoverable(format!("Max retry attempts ({}) exceeded for: {}", self.max_attempts, e)));
                    }
                    let delay = self.delay_for_attempt(attempt);
                    log::info!("Retrying after error (attempt {}/{}), waiting {:?}: {}", attempt, self.max_attempts, delay, e);
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }
}