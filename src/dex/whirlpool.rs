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
    // Add other fields like priceImpactPct, feeAmount, etc., if provided by the API
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
                .unwrap_or_else(|e|{
                    warn!("Failed to build ReqwestClient for Whirlpool, using default: {}", e);
                    ReqwestClient::new()
                }),
            cache,
            quote_cache_ttl_secs: quote_cache_ttl_secs.unwrap_or(30),
        }
    }

    pub fn get_api_key(&self) -> &str {
        &self.api_key
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

        if let Ok(Some(cached_quote)) = self.cache.get_json::<CanonicalQuote>(cache_prefix, &cache_key_params).await {
            debug!("Whirlpool quote cache HIT for {}->{} amount {}", input_token_mint, output_token_mint, amount_in_atomic_units);
            return Ok(cached_quote);
        }
        debug!("Whirlpool quote cache MISS for {}->{} amount {}", input_token_mint, output_token_mint, amount_in_atomic_units);

        // Using the Orca v2 quote endpoint, potentially filtering for Whirlpools if the API supports it.
        // Or use a specific Whirlpool quote endpoint if one exists and is preferred.
        // The original code had "https://api.orca.so/whirlpools/quote..."
        // Let's assume for this client, we might want to ensure it's a Whirlpool route.
        // Some APIs allow specifying `dexes=Whirlpool` or `strategy=WHIRLPOOL_ONLY`.
        let url = format!(
             "https://api.orca.so/v2/solana/quote?inputMint={}&outputMint={}&amountIn={}&strategy=WHIRLPOOL_ONLY", // Example strategy param
            // "https://api.orca.so/whirlpools/quote?inputMint={}&outputMint={}&amount={}", // Alternative original
            input_token_mint, output_token_mint, amount_in_atomic_units
        );
        info!("Requesting Whirlpool quote from URL: {}", url);

        let request_start_time = Instant::now();
        let response_result = WHIRLPOOL_API_RATE_LIMITER
            .get_with_backoff(&self.http_client, &url, |request_url| {
                let req_builder = self.http_client.get(request_url);
                // Add API key if needed (usually not for Orca public quotes)
                // if !self.api_key.is_empty() { req_builder = req_builder.header("X-API-KEY", &self.api_key); }
                req_builder
            })
            .await;
        let request_duration_ms = request_start_time.elapsed().as_millis() as u64;

        match response_result {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    let text = response.text().await.map_err(|e| anyhow!("Failed to read Whirlpool response text: {}", e))?;
                    debug!("Whirlpool API response text for {}->{}: {}", input_token_mint, output_token_mint, text);
                    
                    // Assuming the response structure is WhirlpoolApiResponse or a compatible subset of OrcaQuoteResponse
                    match serde_json::from_str::<WhirlpoolApiResponse>(&text) {
                        Ok(api_response) => {
                            let input_amount_u64 = api_response.in_amount.parse::<u64>().map_err(|e| anyhow!("Parse Whirlpool in_amount: {}", e))?;
                            let output_amount_u64 = api_response.out_amount.parse::<u64>().map_err(|e| anyhow!("Parse Whirlpool out_amount: {}", e))?;

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
                                slippage_estimate: None, // Populate if API provides
                            };

                            if let Err(e) = self.cache.set_json(cache_prefix, &cache_key_params, &canonical_quote, Some(self.quote_cache_ttl_secs)).await {
                                warn!("Failed to cache Whirlpool quote for {}->{}: {}", input_token_mint, output_token_mint, e);
                            }
                            Ok(canonical_quote)
                        }
                        Err(e) => {
                            error!("Deserialize Whirlpool quote: URL {}, Error: {:?}, Body: {}", url, e, text);
                            Err(anyhow!("Deserialize Whirlpool: {}. Body: {}", e, text))
                        }
                    }
                } else {
                    let error_text = response.text().await.unwrap_or_else(|_| "No error body".to_string());
                    error!("Fetch Whirlpool quote failed: Status {}, URL {}, Body: {}", status, url, error_text);
                    Err(anyhow!("Fetch Whirlpool quote: Status {}, Body: {}", status, error_text))
                }
            }
            Err(e) => {
                error!("HTTP request to Whirlpool API failed for URL {}: {}", url, e);
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
