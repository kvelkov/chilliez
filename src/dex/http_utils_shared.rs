// src/dex/http_utils_shared.rs
//! Centralized HTTP and logging utilities for DEX API access.

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE}; // AUTHORIZATION moved to test module
// Ensure tracing or log is imported if used. The original file uses `tracing::error, info;`
// If you've standardized on `log` crate, it should be `log::{error, info};`
// For consistency with other files, I'll use `log`.
use log::{error, info}; // Removed warn


/// Builds a HeaderMap, including an authorization header if an API key is provided,
/// and sets the Content-Type to application/json.
pub fn build_auth_headers(
    api_key_value: Option<&str>,
    auth_header_name: HeaderName,
    auth_prefix: Option<&str>,
) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Some(key_string) = api_key_value {
        if !key_string.is_empty() {
            let header_value_str = if let Some(pfx) = auth_prefix {
                format!("{}{}", pfx, key_string)
            } else {
                key_string.to_string()
            };

            match HeaderValue::from_str(&header_value_str) {
                Ok(header_val) => {
                    headers.insert(auth_header_name, header_val);
                }
                Err(e) => {
                    // Log only a part of the key for security
                    let key_preview = &key_string[..std::cmp::min(5, key_string.len())];
                    error!("API key (value starting with '{}') failed to parse as HeaderValue: {}. Proceeding without this auth header.", key_preview, e);
                }
            }
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
    use reqwest::header::AUTHORIZATION; // Import AUTHORIZATION here as it's only used in tests
    use reqwest::header::HeaderName; // Import HeaderName for test

    #[test]
    fn test_headers_with_api_key() {
        // Test with Authorization header and Bearer prefix
        let headers1 = build_auth_headers(Some("testkey123"), AUTHORIZATION, Some("Bearer "));
        assert_eq!(headers1.get(AUTHORIZATION).unwrap(), "Bearer testkey123");
        assert!(headers1.contains_key(CONTENT_TYPE));

        // Test with a custom header and no prefix
        let custom_header_name = HeaderName::from_static("x-custom-api-key");
        let headers2 = build_auth_headers(Some("customvalue"), custom_header_name.clone(), None);
        assert_eq!(headers2.get(custom_header_name).unwrap(), "customvalue");
        assert!(headers2.contains_key(CONTENT_TYPE));

        // Test with no API key
        let headers3 = build_auth_headers(None, AUTHORIZATION, None);
        assert!(!headers3.contains_key(AUTHORIZATION)); // Auth header should not be present
        assert!(headers3.contains_key(CONTENT_TYPE));
    }
}

// Usage in DEX client (pseudo-code):
// let client = reqwest::Client::new();
// let headers = headers_with_api_key(&self.api_key);
// let resp = client.get(url).headers(headers).send().await?;