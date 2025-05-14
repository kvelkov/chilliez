// src/dex/dex_api_templates.rs

use once_cell::sync::Lazy;
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE},
    Client,
};
use std::env;
use std::time::Duration;
use tracing::{error, info};

static HTTP_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .timeout(Duration::from_secs(5))
        .user_agent("rhodes-arb-bot/1.0")
        .build()
        .expect("Failed to build HTTP client")
});

pub fn get_http_client() -> &'static Client {
    &HTTP_CLIENT
}

pub fn headers_with_api_key(api_key_env_name: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    match env::var(api_key_env_name) {
        Ok(key_string) => {
            match HeaderValue::from_str(&key_string) {
                Ok(header_val) => {
                    headers.insert(AUTHORIZATION, header_val);
                }
                Err(e) => {
                    // Log the error: the API key was found but couldn't be parsed as a valid HeaderValue.
                    // To avoid logging the full key, we can show a part of it or just the error.
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
            }
        }
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
