//! http_utils.rs
//! Shared async HTTP utility for rate limiting, exponential backoff, and fallback logic for DEX API requests.

use anyhow::{anyhow, Result};
use log::warn;
use reqwest::{Client, RequestBuilder, Response};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;

/// Rate limiter and backoff state shared across DEX clients
pub struct HttpRateLimiter {
    #[allow(dead_code)]
    pub max_concurrent: usize,
    pub min_delay: Duration,
    pub max_retries: usize,
    pub base_backoff: Duration,
    pub fallback_urls: Vec<String>,
    semaphore: Arc<tokio::sync::Semaphore>,
    last_request: Arc<Mutex<std::time::Instant>>,
}

impl HttpRateLimiter {
    pub fn new(
        max_concurrent: usize,
        min_delay: Duration,
        max_retries: usize,
        base_backoff: Duration,
        fallback_urls: Vec<String>,
    ) -> Self {
        Self {
            max_concurrent,
            min_delay,
            max_retries,
            base_backoff,
            fallback_urls,
            semaphore: Arc::new(tokio::sync::Semaphore::new(max_concurrent)),
            last_request: Arc::new(Mutex::new(std::time::Instant::now())),
        }
    }

    /// Perform an HTTP GET with rate limiting, exponential backoff, and fallback URLs.
    pub async fn get_with_backoff(
        &self,
        client: &Client,
        url: &str,
        build_req: impl Fn(&str) -> RequestBuilder,
    ) -> Result<Response> {
        let mut attempt = 0;
        let mut urls = vec![url.to_string()];
        urls.extend(self.fallback_urls.clone());

        for url in urls {
            attempt = 0;
            loop {
                let _permit = self.semaphore.acquire().await.unwrap();
                // Enforce min_delay between requests
                {
                    let mut last = self.last_request.lock().await;
                    let elapsed = last.elapsed();
                    if elapsed < self.min_delay {
                        sleep(self.min_delay - elapsed).await;
                    }
                    *last = std::time::Instant::now();
                }
                let req = build_req(&url);
                match req.send().await {
                    Ok(resp) if resp.status().is_success() => return Ok(resp),
                    Ok(resp) => {
                        warn!("HTTP error {} from {}", resp.status(), url);
                        if attempt >= self.max_retries {
                            break;
                        }
                    }
                    Err(e) => {
                        warn!("HTTP request error to {}: {}", url, e);
                        if attempt >= self.max_retries {
                            break;
                        }
                    }
                }
                attempt += 1;
                let backoff = self.base_backoff * (1 << attempt).min(32);
                sleep(backoff).await;
            }
        }
        Err(anyhow!("All HTTP attempts failed for all endpoints"))
    }
}
