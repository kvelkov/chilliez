//! Shared async HTTP utilities for DEX API requests: handles rate limiting, exponential backoff, and fallback URLs.

use anyhow::{anyhow, Result};
use log::warn;
use reqwest::{RequestBuilder, Response};
use std::{
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Mutex, Semaphore};
use tokio::time::sleep;

/// A rate limiter and backoff state, shared across DEX clients.
pub struct HttpRateLimiter {
    pub min_delay: Duration,
    pub max_retries: usize,
    pub base_backoff: Duration,
    pub fallback_urls: Vec<String>,
    semaphore: Arc<Semaphore>,
    last_request: Arc<Mutex<std::time::Instant>>,
}

impl HttpRateLimiter {
    /// Creates a new HttpRateLimiter with the given parameters.
    pub fn new(
        max_concurrent_param: usize,
        min_delay: Duration,
        max_retries: usize,
        base_backoff: Duration,
        fallback_urls: Vec<String>,
    ) -> Self {
        Self {
            min_delay,
            max_retries,
            base_backoff,
            fallback_urls,
            semaphore: Arc::new(Semaphore::new(max_concurrent_param)),
            last_request: Arc::new(Mutex::new(std::time::Instant::now())),
        }
    }

    /// Performs an HTTP GET request with rate limiting, exponential backoff, and fallback to alternative URLs.
    /// The function accepts:
    /// - `initial_url`: The primary endpoint URL.
    /// - `build_req`: A closure that constructs a RequestBuilder when given a URL # string.
    ///
    /// This function will try the primary URL and, if necessary, fallback URLs. It will retry each URL
    /// up to `max_retries` times with exponential backoff.
    pub async fn get_with_backoff(
        &self,
        initial_url: &str,
        build_req: impl Fn(&str) -> RequestBuilder,
    ) -> Result<Response> {
        let mut urls_to_try = vec![initial_url.to_string()];
        urls_to_try.extend(self.fallback_urls.clone());

        for current_url_str in urls_to_try {
            let mut attempt = 0;
            loop {
                // Acquire permit to enforce concurrency limits.
                let _permit = self.semaphore.acquire().await.unwrap();

                // Ensure minimum delay between requests.
                {
                    let mut last = self.last_request.lock().await;
                    let elapsed = last.elapsed();
                    if elapsed < self.min_delay {
                        sleep(self.min_delay - elapsed).await;
                    }
                    *last = std::time::Instant::now();
                }

                let req = build_req(&current_url_str);
                match req.send().await {
                    Ok(resp) if resp.status().is_success() => return Ok(resp),
                    Ok(resp) => {
                        warn!("HTTP error {} from {}", resp.status(), current_url_str);
                        if attempt >= self.max_retries {
                            break;
                        }
                    }
                    Err(e) => {
                        warn!("HTTP request error to {}: {}", current_url_str, e);
                        if attempt >= self.max_retries {
                            break;
                        }
                    }
                }
                attempt += 1;
                // Backoff calculation: increase delay exponentially, capping the multiplier.
                let backoff_multiplier = (1 << attempt).min(32);
                let backoff = self.base_backoff * backoff_multiplier;
                sleep(backoff).await;
            }
        }
        Err(anyhow!("All HTTP attempts failed for all endpoints"))
    }
}
