//! Centralized HTTP and logging utilities for DEX API access.

use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use std::env;
use tracing::{error, info};

/// Returns a HeaderMap with the API key from the given environment variable name, if present.
pub fn headers_with_api_key(api_key_env_name: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    match env::var(api_key_env_name) {
        Ok(key_string) => match HeaderValue::from_str(&key_string) {
            Ok(header_val) => {
                headers.insert("api-key", header_val);
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
            error!(
                "API key environment variable {} not found. Proceeding without this header.",
                api_key_env_name
            );
        }
    }
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers
}

/// Logs HTTP request start and duration.
pub async fn log_timed_request<T>(label: &str, f: impl std::future::Future<Output = T>) -> T {
    let start = std::time::Instant::now();
    let result = f.await;
    let duration = start.elapsed().as_millis();
    info!("{} completed in {} ms", label, duration);
    result
}
