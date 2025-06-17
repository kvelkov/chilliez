// Enhanced API error handling with ban detection and jitter
// Add to src/api/enhanced_error_handling.rs

use anyhow::{anyhow, Result};
use log::{error, info, warn};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// Enhanced error classification for better handling
#[derive(Debug, Clone, PartialEq)]
pub enum ApiErrorType {
    RateLimit,          // 429 Too Many Requests
    Banned,             // 403 Forbidden, account suspended
    Unauthorized,       // 401 Invalid API key
    ServiceUnavailable, // 503 Service down
    BadGateway,         // 502 Gateway issues
    Timeout,            // Request timeout
    NetworkError,       // Connection failures
    InvalidRequest,     // 400 Bad Request
    InternalError,      // 500 Internal Server Error
    Unknown,            // Other errors
}

/// Ban detection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BanDetectionConfig {
    /// Keywords that indicate a ban in error messages
    pub ban_keywords: Vec<String>,
    /// HTTP status codes that indicate bans
    pub ban_status_codes: Vec<u16>,
    /// Maximum consecutive 403s before considering banned
    pub max_consecutive_403s: u32,
    /// Ban detection cooldown period
    pub ban_detection_cooldown: Duration,
    /// Automatic ban recovery attempt interval
    pub ban_recovery_interval: Duration,
}

impl Default for BanDetectionConfig {
    fn default() -> Self {
        Self {
            ban_keywords: vec![
                "banned".to_string(),
                "suspended".to_string(),
                "blocked".to_string(),
                "rate limit exceeded permanently".to_string(),
                "account disabled".to_string(),
                "access denied".to_string(),
                "forbidden".to_string(),
            ],
            ban_status_codes: vec![403, 406], // 403 Forbidden, 406 Not Acceptable
            max_consecutive_403s: 3,
            ban_detection_cooldown: Duration::from_secs(15 * 60), // 15 minutes
            ban_recovery_interval: Duration::from_secs(60 * 60),  // 1 hour
        }
    }
}

/// Enhanced backoff strategy with jitter
#[derive(Debug, Clone)]
pub struct BackoffStrategy {
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub backoff_multiplier: f64,
    pub jitter_percent: f64, // 0.0 - 1.0
    pub enable_jitter: bool,
}

impl Default for BackoffStrategy {
    fn default() -> Self {
        Self {
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(300), // 5 minutes max
            backoff_multiplier: 2.0,
            jitter_percent: 0.1, // ±10% jitter
            enable_jitter: true,
        }
    }
}

/// Enhanced API error handler
#[derive(Debug)]
pub struct EnhancedApiErrorHandler {
    provider_name: String,
    ban_detection: BanDetectionConfig,
    backoff_strategy: BackoffStrategy,
    consecutive_403s: u32,
    #[allow(dead_code)]
    last_ban_check: Instant,
    is_banned: bool,
    ban_detected_at: Option<Instant>,
    error_history: Vec<(Instant, ApiErrorType)>,
}

impl EnhancedApiErrorHandler {
    pub fn new(provider_name: String) -> Self {
        Self {
            provider_name,
            ban_detection: BanDetectionConfig::default(),
            backoff_strategy: BackoffStrategy::default(),
            consecutive_403s: 0,
            last_ban_check: Instant::now(),
            is_banned: false,
            ban_detected_at: None,
            error_history: Vec::new(),
        }
    }

    /// Classify HTTP error response
    pub async fn classify_error(&mut self, status_code: u16, error_body: &str) -> ApiErrorType {
        let error_type = match status_code {
            429 => ApiErrorType::RateLimit,
            403 => {
                self.consecutive_403s += 1;
                if self.is_ban_indicated(status_code, error_body) {
                    ApiErrorType::Banned
                } else {
                    ApiErrorType::Unauthorized
                }
            }
            401 => ApiErrorType::Unauthorized,
            400 => ApiErrorType::InvalidRequest,
            502 => ApiErrorType::BadGateway,
            503 => ApiErrorType::ServiceUnavailable,
            500..=599 => ApiErrorType::InternalError,
            _ => ApiErrorType::Unknown,
        };

        // Record error for pattern analysis
        self.error_history
            .push((Instant::now(), error_type.clone()));
        self.cleanup_old_errors();

        // Check for ban patterns
        if error_type == ApiErrorType::Banned || self.check_ban_pattern() {
            self.handle_ban_detection().await;
        }

        error_type
    }

    /// Check if response indicates a ban
    fn is_ban_indicated(&self, status_code: u16, error_body: &str) -> bool {
        // Check error message keywords for explicit ban indicators
        let error_lower = error_body.to_lowercase();
        for keyword in &self.ban_detection.ban_keywords {
            if error_lower.contains(&keyword.to_lowercase()) {
                return true;
            }
        }

        // Check if we've reached the consecutive 403 threshold
        if status_code == 403 && self.consecutive_403s >= self.ban_detection.max_consecutive_403s {
            return true;
        }

        // Check other ban status codes (but not 403, which requires threshold)
        if self.ban_detection.ban_status_codes.contains(&status_code) && status_code != 403 {
            return true;
        }

        false
    }

    /// Check for ban patterns in error history
    fn check_ban_pattern(&self) -> bool {
        // Too many consecutive 403s
        if self.consecutive_403s >= self.ban_detection.max_consecutive_403s {
            return true;
        }

        // Pattern analysis: high frequency of auth errors
        let recent_errors: Vec<_> = self
            .error_history
            .iter()
            .filter(|(timestamp, _)| timestamp.elapsed() < Duration::from_secs(5 * 60)) // 5 minutes
            .collect();

        if recent_errors.len() >= 10 {
            let auth_errors = recent_errors
                .iter()
                .filter(|(_, error_type)| {
                    matches!(
                        error_type,
                        ApiErrorType::Unauthorized | ApiErrorType::Banned
                    )
                })
                .count();

            return auth_errors as f64 / recent_errors.len() as f64 > 0.8; // 80% auth errors
        }

        false
    }

    /// Handle ban detection
    async fn handle_ban_detection(&mut self) {
        if !self.is_banned {
            self.is_banned = true;
            self.ban_detected_at = Some(Instant::now());

            error!(
                "🚨 BAN DETECTED for {} API! Implementing extended backoff",
                self.provider_name
            );
            error!("   Consecutive 403s: {}", self.consecutive_403s);
            error!(
                "   Recovery attempts will start in {:?}",
                self.ban_detection.ban_recovery_interval
            );

            // Notify monitoring systems
            self.notify_ban_detected().await;
        }
    }

    /// Calculate backoff delay with jitter
    pub fn calculate_backoff_delay(&self, attempt: u32, error_type: &ApiErrorType) -> Duration {
        let base_multiplier = match error_type {
            ApiErrorType::Banned => 10.0, // Much longer delays for bans
            ApiErrorType::RateLimit => 2.0,
            ApiErrorType::ServiceUnavailable => 3.0,
            ApiErrorType::Timeout => 1.5,
            _ => 1.0,
        };

        let delay_ms = (self.backoff_strategy.base_delay.as_millis() as f64
            * base_multiplier
            * self
                .backoff_strategy
                .backoff_multiplier
                .powi(attempt as i32 - 1)) as u64;

        let mut delay =
            Duration::from_millis(delay_ms.min(self.backoff_strategy.max_delay.as_millis() as u64));

        // Add jitter to prevent thundering herd
        if self.backoff_strategy.enable_jitter {
            delay = self.add_jitter(delay);
        }

        // Special handling for bans
        if self.is_banned {
            delay = delay.max(Duration::from_secs(5 * 60)); // Minimum 5 minute delay for bans
        }

        info!(
            "🕐 {} API backoff: {:?} for error type: {:?}",
            self.provider_name, delay, error_type
        );

        delay
    }

    /// Add jitter to delay to prevent thundering herd
    fn add_jitter(&self, delay: Duration) -> Duration {
        let mut rng = rand::thread_rng();
        let jitter_range = self.backoff_strategy.jitter_percent;
        let random_value: f64 = rng.gen();
        let jitter_factor = 1.0 + (random_value - 0.5) * 2.0 * jitter_range;
        let jittered_ms = (delay.as_millis() as f64 * jitter_factor) as u64;
        Duration::from_millis(jittered_ms)
    }

    /// Check if API should be avoided due to ban
    pub fn should_avoid_api(&self) -> bool {
        if !self.is_banned {
            return false;
        }

        // Check if it's time to attempt recovery
        if let Some(ban_time) = self.ban_detected_at {
            ban_time.elapsed() < self.ban_detection.ban_recovery_interval
        } else {
            false
        }
    }

    /// Attempt ban recovery check
    pub async fn attempt_ban_recovery(&mut self) -> bool {
        if !self.is_banned {
            return true;
        }

        if let Some(ban_time) = self.ban_detected_at {
            if ban_time.elapsed() >= self.ban_detection.ban_recovery_interval {
                info!("🔄 Attempting ban recovery for {} API", self.provider_name);

                // Reset ban status for testing
                self.is_banned = false;
                self.consecutive_403s = 0;
                self.ban_detected_at = None;

                return true;
            }
        }

        false
    }

    /// Record successful request (resets error counters)
    pub fn record_success(&mut self) {
        self.consecutive_403s = 0;
        if self.is_banned {
            info!("✅ {} API ban recovery successful!", self.provider_name);
            self.is_banned = false;
            self.ban_detected_at = None;
        }
    }

    /// Clean up old error history
    fn cleanup_old_errors(&mut self) {
        let cutoff = Instant::now() - Duration::from_secs(60 * 60); // 1 hour
        self.error_history
            .retain(|(timestamp, _)| *timestamp > cutoff);
    }

    /// Notify monitoring systems of ban detection
    async fn notify_ban_detected(&self) {
        // In a real implementation, this would send alerts to monitoring systems
        warn!(
            "📧 ALERT: {} API ban detected - switching to fallback providers",
            self.provider_name
        );

        // You could integrate with:
        // - Slack notifications
        // - Email alerts
        // - Monitoring dashboards
        // - Automated failover systems
    }

    /// Get ban status summary
    pub fn get_ban_status(&self) -> BanStatusReport {
        BanStatusReport {
            provider_name: self.provider_name.clone(),
            is_banned: self.is_banned,
            ban_detected_at: self.ban_detected_at,
            consecutive_403s: self.consecutive_403s,
            time_until_recovery: self
                .ban_detected_at
                .map(|ban_time| {
                    let elapsed = ban_time.elapsed();
                    if elapsed < self.ban_detection.ban_recovery_interval {
                        Some(self.ban_detection.ban_recovery_interval - elapsed)
                    } else {
                        None
                    }
                })
                .flatten(),
            recent_error_count: self
                .error_history
                .iter()
                .filter(|(timestamp, _)| timestamp.elapsed() < Duration::from_secs(10 * 60)) // 10 minutes
                .count(),
        }
    }

    /// Parse error string to determine error type
    pub fn parse_error_type(&self, error_str: &str) -> ApiErrorType {
        let error_lower = error_str.to_lowercase();

        // Extract status code if present
        let status_code = if error_lower.contains("429") {
            429
        } else if error_lower.contains("403") {
            403
        } else if error_lower.contains("401") {
            401
        } else if error_lower.contains("500") {
            500
        } else if error_lower.contains("502") {
            502
        } else if error_lower.contains("503") {
            503
        } else {
            0
        };

        // Use the error handler's classification
        if status_code > 0 {
            if status_code == 429 {
                ApiErrorType::RateLimit
            } else if status_code == 403 {
                if self.is_ban_indicated(status_code, error_str) {
                    ApiErrorType::Banned
                } else {
                    ApiErrorType::Unauthorized
                }
            } else {
                match status_code {
                    401 => ApiErrorType::Unauthorized,
                    400 => ApiErrorType::InvalidRequest,
                    502 => ApiErrorType::BadGateway,
                    503 => ApiErrorType::ServiceUnavailable,
                    500..=599 => ApiErrorType::InternalError,
                    _ => ApiErrorType::Unknown,
                }
            }
        } else if error_lower.contains("timeout") {
            ApiErrorType::Timeout
        } else if error_lower.contains("network") || error_lower.contains("connection") {
            ApiErrorType::NetworkError
        } else {
            ApiErrorType::Unknown
        }
    }
}

/// Ban status report for monitoring
#[derive(Debug, Clone)]
pub struct BanStatusReport {
    pub provider_name: String,
    pub is_banned: bool,
    pub ban_detected_at: Option<Instant>,
    pub consecutive_403s: u32,
    pub time_until_recovery: Option<Duration>,
    pub recent_error_count: usize,
}

/// Enhanced retry executor with comprehensive error handling
pub struct EnhancedRetryExecutor {
    error_handler: EnhancedApiErrorHandler,
    max_attempts: u32,
}

impl EnhancedRetryExecutor {
    pub fn new(provider_name: String, max_attempts: u32) -> Self {
        Self {
            error_handler: EnhancedApiErrorHandler::new(provider_name),
            max_attempts,
        }
    }

    /// Execute request with enhanced retry logic
    pub async fn execute_with_retry<F, T, E, Fut>(&mut self, mut operation: F) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = std::result::Result<T, E>>,
        E: std::fmt::Display,
    {
        // Check if API should be avoided due to ban
        if self.error_handler.should_avoid_api() {
            return Err(anyhow!(
                "API {} is currently banned, avoiding requests",
                self.error_handler.provider_name
            ));
        }

        let mut attempt = 0;
        let mut last_error = None;

        while attempt < self.max_attempts {
            attempt += 1;

            // Attempt ban recovery if enough time has passed
            self.error_handler.attempt_ban_recovery().await;

            match operation().await {
                Ok(result) => {
                    self.error_handler.record_success();
                    return Ok(result);
                }
                Err(e) => {
                    last_error = Some(e.to_string());

                    // Parse error to classify it
                    let error_type = self.error_handler.parse_error_type(&e.to_string());

                    // Calculate backoff delay
                    let delay = self
                        .error_handler
                        .calculate_backoff_delay(attempt, &error_type);

                    warn!(
                        "🔄 {} API attempt {}/{} failed: {} (retrying in {:?})",
                        self.error_handler.provider_name, attempt, self.max_attempts, e, delay
                    );

                    // Don't wait on the last attempt
                    if attempt < self.max_attempts {
                        sleep(delay).await;
                    }
                }
            }
        }

        Err(anyhow!(
            "Operation failed after {} attempts. Last error: {}",
            self.max_attempts,
            last_error.unwrap_or_default()
        ))
    }

    /// Get current ban status
    pub fn get_ban_status(&self) -> BanStatusReport {
        self.error_handler.get_ban_status()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ban_detection() {
        let mut handler = EnhancedApiErrorHandler::new("test_api".to_string());

        // Simulate multiple 403 errors
        for i in 0..3 {
            println!(
                "Iteration {}: consecutive_403s before = {}",
                i, handler.consecutive_403s
            );
            let error_type = handler.classify_error(403, "Authentication required").await;
            println!(
                "Iteration {}: consecutive_403s after = {}, error_type = {:?}",
                i, handler.consecutive_403s, error_type
            );

            // First two should be Unauthorized, third should trigger ban detection
            if i < 2 {
                assert_eq!(error_type, ApiErrorType::Unauthorized);
            } else {
                // After threshold, should detect ban
                assert_eq!(error_type, ApiErrorType::Banned);
            }
        }

        // Should detect ban after threshold
        assert!(handler.is_banned);
    }

    #[tokio::test]
    async fn test_jitter_backoff() {
        let handler = EnhancedApiErrorHandler::new("test_api".to_string());

        let delay1 = handler.calculate_backoff_delay(1, &ApiErrorType::RateLimit);
        let delay2 = handler.calculate_backoff_delay(1, &ApiErrorType::RateLimit);

        // With jitter enabled, delays should be different
        assert_ne!(delay1, delay2);
    }

    #[test]
    fn test_error_classification() {
        let handler = EnhancedApiErrorHandler::new("test_api".to_string());

        // Test various error types
        assert_eq!(
            handler.parse_error_type("HTTP 429 Too Many Requests"),
            ApiErrorType::RateLimit
        );
        assert_eq!(
            handler.parse_error_type("Connection timeout"),
            ApiErrorType::Timeout
        );
        assert_eq!(
            handler.parse_error_type("Network error occurred"),
            ApiErrorType::NetworkError
        );
    }
}
