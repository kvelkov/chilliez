// src/dex/orca.rs

use crate::arbitrage::headers_with_api_key; // Assuming this is the correct path
use crate::dex::http_utils::HttpRateLimiter;
use crate::dex::quote::{DexClient, Quote};
use crate::utils::{DexType, PoolInfo, PoolParser as UtilsPoolParser, PoolToken}; // Renamed PoolParser to avoid conflict
use crate::cache::Cache; // Import the Redis Cache

use anyhow::{anyhow, Result as AnyhowResult};
use async_trait::async_trait;
use log::{debug, error, info, warn}; // Added debug and info
use reqwest::Client as ReqwestClient; // Alias to avoid conflict
use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;
use std::env;
use std::str::FromStr;
use std::sync::Arc; // For Arc<Cache>
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH}; // Import SystemTime and UNIX_EPOCH
use once_cell::sync::Lazy;


// Define OrcaPoolParser (assuming it's specific to Orca legacy pools if not using Whirlpool parser for everything)
// If Orca only uses Whirlpools now, this parser might be for older pool types or could be removed.
pub struct OrcaPoolParser;
pub const ORCA_SWAP_PROGRAM_ID_V1: &str = "DjVE6JtD4T4VYZySjYh5mcWJ1yY1arYVQhNE8tXKSbS"; // Example for older version
pub const ORCA_SWAP_PROGRAM_ID_V2: &str = "9W959DqEETiGZoccp2FfeJNjCagVfgtsJy72RykeK2rK"; // From original file

impl UtilsPoolParser for OrcaPoolParser {
    fn parse_pool_data(address: Pubkey, data: &[u8]) -> AnyhowResult<PoolInfo> {
        // This is a simplified stub. A real parser would decode Orca's specific pool account structure.
        // The complexity depends on whether it's a token-swap pool, stable-swap, etc.
        // For AMMv1/v2 pools, data includes mints, amounts, fees.
        if data.len() < 100 { // Arbitrary minimum length, adjust based on actual struct
            error!(
                "Orca pool parsing failed for {} - Insufficient data length: {}",
                address,
                data.len()
            );
            return Err(anyhow!("Data too short for Orca pool: {}", address));
        }
        warn!("Using STUB OrcaPoolParser for address {}. Implement actual parsing logic.", address);
        // Placeholder parsing logic
        Ok(PoolInfo {
            address,
            name: format!("OrcaStubPool/{}", address.to_string().chars().take(6).collect::<String>()),
            token_a: PoolToken {
                mint: Pubkey::new_unique(), // Placeholder
                symbol: "TKA".to_string(),
                decimals: 6, // Placeholder
                reserve: 1_000_000_000, // Placeholder
            },
            token_b: PoolToken {
                mint: Pubkey::new_unique(), // Placeholder
                symbol: "TKB".to_string(),
                decimals: 6, // Placeholder
                reserve: 1_000_000_000, // Placeholder
            },
            fee_numerator: 30, // Example: 0.3%
            fee_denominator: 10000,
            last_update_timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
            dex_type: DexType::Orca,
        })
    }

    fn get_program_id() -> Pubkey { // Changed to return Pubkey directly
        Pubkey::from_str(ORCA_SWAP_PROGRAM_ID_V2)
            .map_err(|e| anyhow!("Failed to parse Orca program ID: {}", e))
            .expect("Static Orca program ID should be valid") // Panic if invalid
    }

    fn get_dex_type() -> DexType {
        DexType::Orca
    }
}


#[derive(Debug, Clone)]
pub struct OrcaClient {
    api_key: String,
    http_client: ReqwestClient,
    cache: Arc<Cache>, // Add cache field
    // Cache TTL for Orca quotes, can be made configurable
    quote_cache_ttl_secs: u64,
}

impl OrcaClient {
    pub fn new(cache: Arc<Cache>, quote_cache_ttl_secs: Option<u64>) -> Self {
        let api_key = env::var("ORCA_API_KEY").unwrap_or_else(|_| {
            warn!("ORCA_API_KEY not set. API calls might be limited or fail.");
            String::new()
        });
        Self {
            api_key,
            http_client: ReqwestClient::builder()
                .timeout(Duration::from_secs(10)) // Example timeout
                .user_agent(format!("RhodesArbBot/{}", env!("CARGO_PKG_VERSION"))) // Use package version
                .build()
                .unwrap_or_else(|e| {
                    warn!("Failed to build ReqwestClient for Orca, using default: {}", e);
                    ReqwestClient::new()
                }),
            cache,
            quote_cache_ttl_secs: quote_cache_ttl_secs.unwrap_or(60), // Default to 60 seconds TTL for quotes
        }
    }

    pub fn get_api_key(&self) -> &str {
        &self.api_key
    }
}

static ORCA_RATE_LIMITER: Lazy<HttpRateLimiter> = Lazy::new(|| {
    HttpRateLimiter::new(
        5,                          // max concurrent requests
        Duration::from_millis(200), // min delay between requests from this client instance
        3,                          // max retries for a single request
        Duration::from_millis(500), // base backoff delay
        // Orca doesn't have documented public fallback URLs for their quote API in the same way some others might.
        // If specific mirrors are known, they can be added.
        vec![],
    )
});

#[async_trait]
impl DexClient for OrcaClient {
    async fn get_best_swap_quote(
        &self,
        input_token_mint: &str,
        output_token_mint: &str,
        amount_in_atomic_units: u64,
    ) -> AnyhowResult<Quote> {
        let cache_key_params = [
            input_token_mint,
            output_token_mint,
            &amount_in_atomic_units.to_string(),
        ];
        let cache_prefix = "quote:orca";

        // 1. Try to get from cache
        if let Some(cached_quote) = self.cache.get_json::<Quote>(cache_prefix, &cache_key_params).await? {
            debug!(
                "Orca quote cache HIT for {}->{} amount {}",
                input_token_mint, output_token_mint, amount_in_atomic_units
            );
            return Ok(cached_quote);
        }
        debug!(
            "Orca quote cache MISS for {}->{} amount {}",
            input_token_mint, output_token_mint, amount_in_atomic_units
        );

        // 2. If not in cache, fetch from API
        // Orca's v2 API endpoint for quotes (ensure this is the correct one you intend to use)
        // The /v1/quote/sol seems to be for specific SOL pairs.
        // Jupiter API is often recommended for Orca quotes if direct API is complex or rate-limited.
        // Assuming a hypothetical direct quote endpoint for Orca for this example:
        // let url = format!(
        //     "https://api.orca.so/v2/quote?inputMint={}&outputMint={}&amount={}",
        //     input_token_mint, output_token_mint, amount_in_atomic_units
        // );
        // The provided code used: "https://api.orca.so/v2/solana/quote?inputMint={}&outputMint={}&amountIn={}"
        let url = format!(
            "https://api.orca.so/v2/solana/quote?inputMint={}&outputMint={}&amountIn={}",
            input_token_mint, output_token_mint, amount_in_atomic_units
        );


        let request_start_time = Instant::now();
        let response_result = ORCA_RATE_LIMITER
            .get_with_backoff(&self.http_client, &url, |request_url| {
                let mut req_builder = self.http_client.get(request_url);
                // Add API key if Orca requires it for this endpoint
                if !self.api_key.is_empty() {
                     // Check Orca's API documentation for the correct header name if it's not 'api-key'
                    req_builder = req_builder.header("Authorization", format!("Bearer {}", self.api_key));
                }
                req_builder
            })
            .await;

        let request_duration_ms = request_start_time.elapsed().as_millis() as u64;

        match response_result {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    let text = response.text().await.map_err(|e| anyhow!("Failed to read Orca response text: {}", e))?;
                    debug!("Orca API response text for {}->{}: {}", input_token_mint, output_token_mint, text);

                    match serde_json::from_str::<OrcaQuoteResponse>(&text) {
                        Ok(api_response) => {
                            let input_amount_u64 = api_response.in_amount.parse::<u64>().map_err(|e| {
                                anyhow!("Failed to parse Orca in_amount '{}': {}", api_response.in_amount, e)
                            })?;
                            let output_amount_u64 = api_response.out_amount.parse::<u64>().map_err(|e| {
                                anyhow!("Failed to parse Orca out_amount '{}': {}", api_response.out_amount, e)
                            })?;

                            let quote = Quote {
                                input_token: api_response.input_mint,
                                output_token: api_response.output_mint,
                                input_amount: input_amount_u64,
                                output_amount: output_amount_u64,
                                dex: self.get_name().to_string(),
                                route: api_response.route.unwrap_or_else(|| vec![input_token_mint.to_string(), output_token_mint.to_string()]), // Use API route if available
                                latency_ms: Some(request_duration_ms),
                                execution_score: None, // Placeholder
                                route_path: None,      // Placeholder, can be derived from `route`
                                slippage_estimate: api_response.slippage_bps.map(|bps| bps as f64 / 10000.0), // Convert BPS to percentage
                            };

                            // 3. Store successful response in cache
                            if let Err(e) = self.cache.set_json(cache_prefix, &cache_key_params, &quote, Some(self.quote_cache_ttl_secs)).await {
                                warn!("Failed to cache Orca quote for {}->{}: {}", input_token_mint, output_token_mint, e);
                            }
                            Ok(quote)
                        }
                        Err(e) => {
                            error!(
                                "Failed to deserialize Orca quote from URL {}: {:?}. Response body: {}",
                                url, e, text
                            );
                            Err(anyhow!("Failed to deserialize Orca quote from {}. Error: {}. Body: {}", url, e, text))
                        }
                    }
                } else {
                    let error_text = response.text().await.unwrap_or_else(|_| "Failed to read error response body".to_string());
                    error!(
                        "Failed to fetch Orca quote from URL {}: Status {}. Body: {}",
                        url, status, error_text
                    );
                    Err(anyhow!("Failed to fetch Orca quote from {}: {}. Body: {}", url, status, error_text))
                }
            }
            Err(e) => {
                 error!("HTTP request to Orca failed for URL {}: {}", url, e);
                 Err(e) // Propagate the anyhow::Error from get_with_backoff
            }
        }
    }

    fn get_supported_pairs(&self) -> Vec<(String, String)> {
        // TODO: Fetch this dynamically or from a more comprehensive static list if Orca provides one.
        // This is just a placeholder.
        warn!("OrcaClient::get_supported_pairs returning placeholder data.");
        vec![
            (
                "So11111111111111111111111111111111111111112".to_string(), // SOL
                "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(), // USDC
            ),
            (
                "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(), // USDC
                "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB".to_string(), // USDT
            ),
        ]
    }

    fn get_name(&self) -> &str {
        "Orca" // Consistent naming
    }
}

/// Represents the structure of a successful quote response from Orca's API.
/// Field names must match the JSON response. Add/remove fields as necessary.
#[derive(Deserialize, Debug)]
struct OrcaQuoteResponse {
    #[serde(rename = "inputMint")]
    input_mint: String,
    #[serde(rename = "outputMint")]
    output_mint: String,
    #[serde(rename = "inAmount")]
    in_amount: String, // Orca API might return amounts as strings
    #[serde(rename = "outAmount")]
    out_amount: String,
    // Optional fields that might be present in Orca's response
    route: Option<Vec<String>>, // If Orca provides the route path
    #[serde(rename = "slippageBps")]
    slippage_bps: Option<u64>, // Slippage in basis points
    // Add other fields like priceImpact, feeAmount, etc., if available and useful
}
