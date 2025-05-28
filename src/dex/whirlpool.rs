// src/dex/whirlpool.rs
//! API Client for Orca Whirlpools.

use crate::cache::Cache;
use crate::dex::http_utils::HttpRateLimiter;
use crate::dex::quote::{DexClient, Quote as CanonicalQuote};
use anyhow::{anyhow, Result as AnyhowResult};
use async_trait::async_trait;
use log::{debug, error, info, warn};
use once_cell::sync::Lazy;
use reqwest::Client as ReqwestClient;
use serde::{Deserialize, Serialize};
use std::env;
use std::sync::Arc;
use std::time::{Duration, Instant};

static WHIRLPOOL_API_RATE_LIMITER: Lazy<HttpRateLimiter> = Lazy::new(|| {
    HttpRateLimiter::new(
        5,
        Duration::from_millis(200),
        3,
        Duration::from_millis(500),
        vec![],
    )
});

#[derive(Deserialize, Serialize, Debug, Clone)]
struct WhirlpoolApiResponse {
    #[serde(rename = "inputMint")]
    input_mint: String,
    #[serde(rename = "outputMint")]
    output_mint: String,
    #[serde(rename = "inAmount")]
    in_amount: String,
    #[serde(rename = "outAmount")]
    out_amount: String,
    // Additional fields like priceImpactPct or feeAmount can be added here if provided.
}

#[derive(Debug, Clone)]
pub struct WhirlpoolClient {
    api_key: String,
    http_client: ReqwestClient,
    cache: Arc<Cache>,
    quote_cache_ttl_secs: u64,
}

impl WhirlpoolClient {
    pub fn new(cache: Arc<Cache>, quote_cache_ttl_secs: Option<u64>) -> Self {
        let api_key = env::var("WHIRLPOOL_API_KEY").unwrap_or_else(|_| {
            debug!("WHIRLPOOL_API_KEY not set (usually not required for Orca public quotes).");
            String::new()
        });
        Self {
            api_key,
            http_client: ReqwestClient::builder()
                .timeout(Duration::from_secs(10))
                .user_agent(format!("RhodesArbBot/{}", env!("CARGO_PKG_VERSION")))
                .build()
                .unwrap_or_else(|e| {
                    warn!(
                        "Failed to build ReqwestClient for Whirlpool, using default: {}",
                        e
                    );
                    ReqwestClient::new()
                }),
            cache,
            quote_cache_ttl_secs: quote_cache_ttl_secs.unwrap_or(30),
        }
    }
}

#[async_trait]
impl DexClient for WhirlpoolClient {
    async fn get_best_swap_quote(
        &self,
        input_token_mint: &str,
        output_token_mint: &str,
        amount_in_atomic_units: u64,
    ) -> AnyhowResult<CanonicalQuote> {
        let cache_key_params = [
            input_token_mint,
            output_token_mint,
            &amount_in_atomic_units.to_string(),
        ];
        let cache_prefix = "quote:whirlpool";

        // Try to get a cached quote first.
        if let Ok(Some(cached_quote)) = self
            .cache
            .get_json::<CanonicalQuote>(cache_prefix, &cache_key_params)
            .await
        {
            debug!(
                "Whirlpool quote cache HIT for {}->{} amount {}",
                input_token_mint, output_token_mint, amount_in_atomic_units
            );
            return Ok(cached_quote);
        }
        debug!(
            "Whirlpool quote cache MISS for {}->{} amount {}",
            input_token_mint, output_token_mint, amount_in_atomic_units
        );

        // Build the request URL. Here we force the strategy to WHIRLPOOL_ONLY.
        let url = format!(
            "https://api.orca.so/v2/solana/quote?inputMint={}&outputMint={}&amountIn={}&strategy=WHIRLPOOL_ONLY",
            input_token_mint, output_token_mint, amount_in_atomic_units
        );
        info!("Requesting Whirlpool quote from URL: {}", url);

        let request_start_time = Instant::now();
        let response_result = WHIRLPOOL_API_RATE_LIMITER.get_with_backoff(&url, |request_url| {
            let mut req_builder = self.http_client.get(request_url);
            if !self.api_key.is_empty() {
                req_builder = req_builder.header("X-API-KEY", &self.api_key);
            }
            req_builder
        }).await;
        let request_duration_ms = request_start_time.elapsed().as_millis() as u64;

        match response_result {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    let text = response
                        .text()
                        .await
                        .map_err(|e| anyhow!("Failed to read Whirlpool response text: {}", e))?;
                    debug!(
                        "Whirlpool API response for {}->{}: {}",
                        input_token_mint, output_token_mint, text
                    );

                    match serde_json::from_str::<WhirlpoolApiResponse>(&text) {
                        Ok(api_response) => {
                            let input_amount_u64 = api_response
                                .in_amount
                                .parse::<u64>()
                                .map_err(|e| anyhow!("Parse Whirlpool in_amount: {}", e))?;
                            let output_amount_u64 = api_response
                                .out_amount
                                .parse::<u64>()
                                .map_err(|e| anyhow!("Parse Whirlpool out_amount: {}", e))?;

                            let canonical_quote = CanonicalQuote {
                                input_token: api_response.input_mint,
                                output_token: api_response.output_mint,
                                input_amount: input_amount_u64,
                                output_amount: output_amount_u64,
                                dex: self.get_name().to_string(),
                                route: vec![
                                    input_token_mint.to_string(),
                                    output_token_mint.to_string(),
                                ],
                                latency_ms: Some(request_duration_ms),
                                execution_score: None,
                                route_path: None,
                                slippage_estimate: None, // Populate if available from API response.
                            };

                            // Cache the result for future calls.
                            if let Err(e) = self
                                .cache
                                .set_ex(
                                    cache_prefix,
                                    &cache_key_params,
                                    &canonical_quote,
                                    Some(self.quote_cache_ttl_secs),
                                )
                                .await
                            {
                                warn!(
                                    "Failed to cache Whirlpool quote for {}->{}: {}",
                                    input_token_mint, output_token_mint, e
                                );
                            }
                            Ok(canonical_quote)
                        }
                        Err(e) => {
                            error!(
                                "Failed to deserialize Whirlpool quote: URL {}, Error: {:?}, Body: {}",
                                url, e, text
                            );
                            Err(anyhow!("Deserialize Whirlpool quote error: {}. Body: {}", e, text))
                        }
                    }
                } else {
                    let error_text = response.text().await.unwrap_or_else(|_| "No error body".to_string());
                    error!(
                        "Whirlpool quote fetch failed: Status {}, URL {}, Body: {}",
                        status, url, error_text
                    );
                    Err(anyhow!("Fetch Whirlpool quote failed: Status {}, Body: {}", status, error_text))
                }
            }
            Err(e) => {
                error!(
                    "HTTP request to Whirlpool API failed for URL {}: {}",
                    url, e
                );
                Err(e)
            }
        }
    }

    fn get_supported_pairs(&self) -> Vec<(String, String)> {
        warn!("WhirlpoolClient::get_supported_pairs returning empty list.");
        vec![]
    }

    fn get_name(&self) -> &str {
        "Whirlpool"
    }
}
