// src/dex/phoenix.rs

use crate::cache::Cache;
use crate::dex::http_utils::HttpRateLimiter;
use crate::dex::quote::{DexClient, Quote as CanonicalQuote};
use crate::dex::http_utils_shared::log_timed_request; // Added for potential future use

use anyhow::{anyhow, Result as AnyhowResult};
use async_trait::async_trait;
use log::{debug, error, info, warn};
use once_cell::sync::Lazy;
use reqwest::Client as ReqwestClient;
use serde::{Deserialize, Serialize};
use std::env;
use std::sync::Arc;
use std::time::{Duration, Instant};

static PHOENIX_RATE_LIMITER: Lazy<HttpRateLimiter> = Lazy::new(|| {
    HttpRateLimiter::new(
        5,
        Duration::from_millis(200),
        3,
        Duration::from_millis(500),
        vec![],
    )
});

#[derive(Deserialize, Serialize, Debug, Clone)]
struct PhoenixApiResponse {
    #[serde(rename = "inputMint")]
    input_mint: String,
    #[serde(rename = "outputMint")]
    output_mint: String,
    #[serde(rename = "inAmount")]
    in_amount: String,
    #[serde(rename = "outAmount")]
    out_amount: String,
    #[serde(rename = "priceImpactPercent")] 
    price_impact_percent: Option<String>, 
}

pub const _PHOENIX_PROGRAM_ID: &str = "PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY"; // Prefixed

#[derive(Debug, Clone)]
pub struct PhoenixClient {
    _api_key: String, // Prefixed
    http_client: ReqwestClient,
    cache: Arc<Cache>,
    quote_cache_ttl_secs: u64,
}

impl PhoenixClient {
    pub fn new(cache: Arc<Cache>, quote_cache_ttl_secs: Option<u64>) -> Self {
        let api_key = env::var("PHOENIX_API_KEY").unwrap_or_else(|_| {
            warn!("PHOENIX_API_KEY not set. This may affect API access for Phoenix.");
            String::new()
        });
        Self {
            _api_key: api_key, // Used prefixed field
            http_client: ReqwestClient::builder()
                .timeout(Duration::from_secs(10))
                .user_agent(format!("RhodesArbBot/{}", env!("CARGO_PKG_VERSION")))
                .build()
                .unwrap_or_else(|e| {
                    warn!("Failed to build ReqwestClient for Phoenix, using default: {}", e);
                    ReqwestClient::new()
                }),
            cache,
            quote_cache_ttl_secs: quote_cache_ttl_secs.unwrap_or(10), 
        }
    }

    pub fn _get_api_key(&self) -> &str { // Prefixed
        &self._api_key
    }
}

#[async_trait]
impl DexClient for PhoenixClient {
    async fn get_best_swap_quote(
        &self,
        input_token_mint: &str,
        output_token_mint: &str,
        amount_in_atomic_units: u64,
    ) -> AnyhowResult<CanonicalQuote> {
        let operation_label = format!("PhoenixClient_GetQuote_{}_{}", input_token_mint, output_token_mint);
        log_timed_request(&operation_label, async {
            let cache_key_params = [
                input_token_mint,
                output_token_mint,
                &amount_in_atomic_units.to_string(),
            ];
            let cache_prefix = "quote:phoenix";

            if let Ok(Some(cached_quote)) = self.cache.get_json::<CanonicalQuote>(cache_prefix, &cache_key_params).await {
                debug!("Phoenix quote cache HIT for {}->{} amount {}", input_token_mint, output_token_mint, amount_in_atomic_units);
                return Ok(cached_quote);
            }
            debug!("Phoenix quote cache MISS for {}->{} amount {}", input_token_mint, output_token_mint, amount_in_atomic_units);

            let url = format!(
                "https://api.phoenix.trade/v1/quote?inputMint={}&outputMint={}&amountIn={}", 
                input_token_mint, output_token_mint, amount_in_atomic_units
            );
            info!("Requesting Phoenix quote from URL: {}", url);

            let request_start_time = Instant::now();
            let response_result = PHOENIX_RATE_LIMITER // Corrected call
                .get_with_backoff(&url, |request_url| {
                    let mut req_builder = self.http_client.get(request_url);
                    if !self._api_key.is_empty() { // Used prefixed field
                        req_builder = req_builder.header("X-API-KEY", &self._api_key); // Assuming X-API-KEY for Phoenix, adjust if needed
                    }
                    req_builder
                })
                .await;
            let request_duration_ms = request_start_time.elapsed().as_millis() as u64;

            match response_result {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        let text = response.text().await.map_err(|e| anyhow!("Failed to read Phoenix response text: {}", e))?;
                        debug!("Phoenix API response text for {}->{}: {}", input_token_mint, output_token_mint, text);

                        match serde_json::from_str::<PhoenixApiResponse>(&text) {
                            Ok(api_response) => {
                                let input_amount_u64 = api_response.in_amount.parse::<u64>().map_err(|e| anyhow!("Parse Phoenix in_amount: {}", e))?;
                                let output_amount_u64 = api_response.out_amount.parse::<u64>().map_err(|e| anyhow!("Parse Phoenix out_amount: {}", e))?;
                                
                                let slippage_estimate = api_response.price_impact_percent
                                    .as_ref()
                                    .and_then(|s| s.trim_end_matches('%').parse::<f64>().ok().map(|val| val / 100.0));

                                let canonical_quote = CanonicalQuote {
                                    input_token: api_response.input_mint,
                                    output_token: api_response.output_mint,
                                    input_amount: input_amount_u64,
                                    output_amount: output_amount_u64,
                                    dex: self.get_name().to_string(),
                                    route: vec![input_token_mint.to_string(), output_token_mint.to_string()],
                                    latency_ms: Some(request_duration_ms),
                                    execution_score: None,
                                    route_path: None,
                                    slippage_estimate,
                                };

                                if let Err(e) = self.cache.set_ex(cache_prefix, &cache_key_params, &canonical_quote, Some(self.quote_cache_ttl_secs)).await {
                                    warn!("Failed to cache Phoenix quote for {}->{}: {}", input_token_mint, output_token_mint, e);
                                }
                                Ok(canonical_quote)
                            }
                            Err(e) => {
                                error!("Deserialize Phoenix quote: URL {}, Error: {:?}, Body: {}", url, e, text);
                                Err(anyhow!("Deserialize Phoenix: {}. Body: {}", e, text))
                            }
                        }
                    } else {
                        let error_text = response.text().await.unwrap_or_else(|_| "No error body".to_string());
                        error!("Fetch Phoenix quote failed: Status {}, URL {}, Body: {}", status, url, error_text);
                        Err(anyhow!("Fetch Phoenix quote: Status {}, Body: {}", status, error_text))
                    }
                }
                Err(e) => {
                     error!("HTTP request to Phoenix failed for URL {}: {}", url, e);
                     Err(e)
                }
            }
        }).await
    }

    fn get_supported_pairs(&self) -> Vec<(String, String)> {
        warn!("PhoenixClient::get_supported_pairs returning empty list.");
        vec![]
    }

    fn get_name(&self) -> &str {
        "Phoenix"
    }
}