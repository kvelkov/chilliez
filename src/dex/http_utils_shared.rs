//! Centralized HTTP and logging utilities for DEX API access.

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE};
use log::{error, info};

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
                    // Log only a part of the key for security reasons.
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
/// This function wraps any asynchronous operation and logs its execution duration.
///
/// # Arguments
///
/// * `label` - A string label identifying the HTTP request.
/// * `f` - The asynchronous operation (future) to wrap.
pub async fn log_timed_request<T>(label: &str, f: impl std::future::Future<Output = T>) -> T {
    let start = std::time::Instant::now();
    info!("Starting: {}", label);
    let result = f.await;
    let duration = start.elapsed().as_millis();
    info!("Completed: {} in {} ms", label, duration);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::AUTHORIZATION;
    use reqwest::header::HeaderName;

    #[test]
    fn test_headers_with_api_key() {
        // Test with Authorization header and a Bearer prefix.
        let headers1 = build_auth_headers(Some("testkey123"), AUTHORIZATION, Some("Bearer "));
        assert_eq!(headers1.get(AUTHORIZATION).unwrap(), "Bearer testkey123");
        assert!(headers1.contains_key(CONTENT_TYPE));

        // Test with a custom header and no prefix.
        let custom_header_name = HeaderName::from_static("x-custom-api-key");
        let headers2 = build_auth_headers(Some("customvalue"), custom_header_name.clone(), None);
        assert_eq!(headers2.get(custom_header_name).unwrap(), "customvalue");
        assert!(headers2.contains_key(CONTENT_TYPE));

        // Test with no API key.
        let headers3 = build_auth_headers(None, AUTHORIZATION, None);
        assert!(!headers3.contains_key(AUTHORIZATION)); // Should not include the auth header if no API key.
        assert!(headers3.contains_key(CONTENT_TYPE));
    }
}
