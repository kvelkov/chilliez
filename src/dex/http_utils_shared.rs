// src/dex/http_utils_shared.rs
//! Centralized HTTP and logging utilities for DEX API access.

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use std::env;
// Ensure tracing or log is imported if used. The original file uses `tracing::error, info;`
// If you've standardized on `log` crate, it should be `log::{error, info};`
// For consistency with other files, I'll use `log`.
use log::{error, info, warn};


/// Returns a HeaderMap with the API key from the given environment variable name, if present.
pub fn headers_with_api_key(api_key_env_name: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    match env::var(api_key_env_name) {
        Ok(key_string) => match HeaderValue::from_str(&key_string) {
            Ok(header_val) => {
                headers.insert(AUTHORIZATION, header_val);
            }
            Err(e) => {
                let partial_key_display = if key_string.len() > 5 {
                    format!("{}...", &key_string[..5])
                } else {
                    key_string.to_string()
                };
                error!(
                        "API key for {} (starting with '{}') found but failed to parse as HeaderValue: {}. Proceeding without this header.",
                        api_key_env_name,
                        partial_key_display,
                        e
                    );
            }
        },
        Err(_) => {
            // Changed from error! to warn! as missing API key might be optional
            warn!(
                "API key environment variable {} not found. Proceeding without this header (if optional).",
                api_key_env_name
            );
        }
    }
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers
}

/// Logs HTTP request start and duration.
// This function is now used in orca.rs
pub async fn log_timed_request<T>(label: &str, f: impl std::future::Future<Output = T>) -> T {
    let start = std::time::Instant::now();
    info!("Starting: {}", label); // Log start
    let result = f.await;
    let duration = start.elapsed().as_millis();
    info!("Completed: {} in {} ms", label, duration);
    result
}

// Example integration in a DEX client HTTP request:
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_headers_with_api_key() {
        let api_key = "testkey";
        let headers = headers_with_api_key(api_key);
        assert_eq!(headers.get(AUTHORIZATION).unwrap(), "testkey");
    }
}

// Usage in DEX client (pseudo-code):
// let client = reqwest::Client::new();
// let headers = headers_with_api_key(&self.api_key);
// let resp = client.get(url).headers(headers).send().await?;