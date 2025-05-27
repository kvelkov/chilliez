// src/dex/lifinity.rs

use crate::cache::Cache;
use crate::dex::http_utils::HttpRateLimiter;
use crate::dex::quote::{DexClient, Quote as CanonicalQuote};
use crate::utils::{DexType, PoolInfo, PoolParser as UtilsPoolParser, PoolToken}; // Renamed PoolParser

use anyhow::{anyhow, Result as AnyhowResult};
use async_trait::async_trait;
use log::{debug, error, info, warn};
use once_cell::sync::Lazy;
use reqwest::Client as ReqwestClient;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::env;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static LIFINITY_RATE_LIMITER: Lazy<HttpRateLimiter> = Lazy::new(|| {
    HttpRateLimiter::new(4, Duration::from_millis(250), 3, Duration::from_millis(500), vec![])
});

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifinityApiQuote {
    #[serde(rename = "inputToken")]
    pub input_token: String,
    #[serde(rename = "outputToken")]
    pub output_token: String,
    #[serde(rename = "inputAmount")]
    pub input_amount: u64,
    #[serde(rename = "outputAmount")]
    pub output_amount: u64,
    pub dex: String,
}

#[derive(Debug, Clone)] // LifinityClient already derives Debug
pub struct LifinityClient {
    api_key: String,
    http_client: ReqwestClient,
    cache: Arc<Cache>, // Cache itself now derives Debug
    quote_cache_ttl_secs: u64,
}

impl LifinityClient {
    pub fn new(cache: Arc<Cache>, quote_cache_ttl_secs: Option<u64>) -> Self {
        let api_key = env::var("LIFINITY_API_KEY").unwrap_or_else(|_| {
            warn!("LIFINITY_API_KEY environment variable is not set. API calls may be limited or fail.");
            String::new()
        });
        Self {
            api_key,
            http_client: ReqwestClient::builder()
                .timeout(Duration::from_secs(10))
                .user_agent(format!("RhodesArbBot/{}", env!("CARGO_PKG_VERSION")))
                .build()
                .unwrap_or_else(|e| {
                    warn!("Failed to build ReqwestClient for Lifinity, using default: {}", e);
                    ReqwestClient::new()
                }),
            cache,
            quote_cache_ttl_secs: quote_cache_ttl_secs.unwrap_or(60),
        }
    }

    fn build_request_with_api_key(&self, url: &str) -> reqwest::RequestBuilder {
        let mut req_builder = self.http_client.get(url);
        if !self.api_key.is_empty() {
            req_builder = req_builder.header("api-key", &self.api_key);
        }
        req_builder
    }

    #[allow(dead_code)]
    pub fn get_api_key(&self) -> &str {
        &self.api_key
    }
}

#[async_trait]
impl DexClient for LifinityClient {
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
        let cache_prefix = "quote:lifinity";

        if let Ok(Some(cached_quote)) = self.cache.get_json::<CanonicalQuote>(cache_prefix, &cache_key_params).await {
            debug!("Lifinity quote cache HIT for {}->{} amount {}", input_token_mint, output_token_mint, amount_in_atomic_units);
            return Ok(cached_quote);
        }
        debug!("Lifinity quote cache MISS for {}->{} amount {}", input_token_mint, output_token_mint, amount_in_atomic_units);

        let url = format!(
            "https://api.lifinity.io/quote?input={}&output={}&amount={}",
            input_token_mint, output_token_mint, amount_in_atomic_units
        );
        info!("Requesting Lifinity quote from URL: {}", url);

        let request_start_time = Instant::now();
        let response_result = LIFINITY_RATE_LIMITER
            .get_with_backoff(&url, |request_url| { // Removed &self.http_client
                self.build_request_with_api_key(request_url) // http_client is captured by build_request_with_api_key
            })
            .await;
        let request_duration_ms = request_start_time.elapsed().as_millis() as u64;

        match response_result {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    let text = response.text().await.map_err(|e| anyhow!("Failed to read Lifinity response text: {}", e))?;
                    debug!("Lifinity API response text for {}->{}: {}", input_token_mint, output_token_mint, text);
                    
                    match serde_json::from_str::<LifinityApiQuote>(&text) {
                        Ok(lifinity_quote) => {
                            let canonical_quote = CanonicalQuote {
                                input_token: lifinity_quote.input_token.clone(),
                                output_token: lifinity_quote.output_token.clone(),
                                input_amount: lifinity_quote.input_amount,
                                output_amount: lifinity_quote.output_amount,
                                dex: self.get_name().to_string(), // Use self.get_name()
                                route: vec![lifinity_quote.input_token, lifinity_quote.output_token],
                                latency_ms: Some(request_duration_ms),
                                execution_score: None,
                                route_path: None,
                                slippage_estimate: None,
                            };

                            // Corrected call to set_ex
                            if let Err(e) = self.cache.set_ex(cache_prefix, &cache_key_params, &canonical_quote, Some(self.quote_cache_ttl_secs)).await {
                                warn!("Failed to cache Lifinity quote for {}->{}: {}", input_token_mint, output_token_mint, e);
                            }
                            Ok(canonical_quote)
                        }
                        Err(e) => {
                            error!("Failed to deserialize Lifinity quote from URL {}: {:?}. Response text: {}", url, e, text);
                            Err(anyhow!("Deserialize Lifinity: {}. Body: {}", e, text))
                        }
                    }
                } else {
                    let error_body = response.text().await.unwrap_or_else(|_| "Could not retrieve error body".to_string());
                    error!("Fetch Lifinity quote failed: Status {}, URL {}, Body: {}", status, url, error_body);
                    Err(anyhow!("Fetch Lifinity quote: Status {}, Body: {}", status, error_body))
                }
            }
            Err(e) => {
                error!("HTTP request to Lifinity failed for URL {}: {}", url, e);
                Err(e)
            }
        }
    }

    fn get_supported_pairs(&self) -> Vec<(String, String)> {
        warn!("LifinityClient::get_supported_pairs returning empty list.");
        vec![]
    }

    fn get_name(&self) -> &str {
        "Lifinity"
    }
}

pub struct LifinityPoolParser;
pub const LIFINITY_PROGRAM_ID: &str = "EewxydAPCCVuNEyrVN68PuSYdQ7wKn27V9GJEbpNcHcn";

impl UtilsPoolParser for LifinityPoolParser {
    fn parse_pool_data(address: Pubkey, data: &[u8]) -> AnyhowResult<PoolInfo> {
        if data.len() < 100 { // Arbitrary check
            error!("Lifinity pool parsing failed for {} - Insufficient data: {}", address, data.len());
            return Err(anyhow!("Data too short for Lifinity pool: {}", address));
        }
        warn!("Using STUB LifinityPoolParser for address {}. Implement actual parsing.", address);
        Ok(PoolInfo {
            address,
            name: format!("LifinityStub/{}", address.to_string().chars().take(6).collect::<String>()),
            token_a: PoolToken { mint: Pubkey::new_unique(), symbol: "TKA".to_string(), decimals: 6, reserve: 1_000_000 },
            token_b: PoolToken { mint: Pubkey::new_unique(), symbol: "TKB".to_string(), decimals: 6, reserve: 1_000_000 },
            fee_numerator: 30, // Example
            fee_denominator: 10000,
            last_update_timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
            dex_type: DexType::Lifinity,
        })
    }

    fn get_program_id() -> Pubkey {
        Pubkey::from_str(LIFINITY_PROGRAM_ID)
            .map_err(|e| anyhow!("Invalid Lifinity Program ID: {}", e))
            .expect("Static Lifinity program ID should be valid")
    }

    fn get_dex_type() -> DexType {
        DexType::Lifinity
    }
}