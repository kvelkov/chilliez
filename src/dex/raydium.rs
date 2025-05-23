// src/dex/raydium.rs

use crate::arbitrage::headers_with_api_key;
use crate::cache::Cache; // Import the Redis Cache
use crate::dex::http_utils::HttpRateLimiter;
use crate::dex::quote::{DexClient, Quote};
use crate::utils::{DexType, PoolInfo, PoolParser as UtilsPoolParser, PoolToken}; // Renamed PoolParser

use anyhow::{anyhow, Result as AnyhowResult};
use async_trait::async_trait;
use log::{debug, error, info, warn};
use once_cell::sync::Lazy;
use reqwest::Client as ReqwestClient;
use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;
use std::env;
use std::str::FromStr;
use std::sync::Arc; // For Arc<Cache>
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};


// Define RaydiumPoolParser
pub struct RaydiumPoolParser;
// Ensure this is the correct Program ID for the Raydium pools you intend to parse.
// There are multiple Raydium program IDs (Liquidity Pool V4, AMM V5, Stableswap, etc.)
pub const RAYDIUM_LIQUIDITY_PROGRAM_V4: &str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";

impl UtilsPoolParser for RaydiumPoolParser {
    fn parse_pool_data(address: Pubkey, data: &[u8]) -> AnyhowResult<PoolInfo> {
        // Placeholder: Implement actual Raydium pool data parsing.
        // Raydium's AMM account structures are complex and vary by version.
        // Refer to Raydium SDK or on-chain program details.
        if data.len() < 300 { // Arbitrary minimum length
            error!("Raydium pool parsing failed for {} - Insufficient data length: {}", address, data.len());
            return Err(anyhow!("Data too short for Raydium pool: {}", address));
        }
        warn!("Using STUB RaydiumPoolParser for address {}. Implement actual parsing logic.", address);
        Ok(PoolInfo {
            address,
            name: format!("RaydiumStubPool/{}", address.to_string().chars().take(6).collect::<String>()),
            token_a: PoolToken { mint: Pubkey::new_unique(), symbol: "TKA".to_string(), decimals: 6, reserve: 1_000_000_000 },
            token_b: PoolToken { mint: Pubkey::new_unique(), symbol: "TKB".to_string(), decimals: 6, reserve: 1_000_000_000 },
            fee_numerator: 25, // Example: 0.25% (numerator for fee, denominator for amount)
            fee_denominator: 10000, // For a 0.25% fee, this means (amount * 25) / 10000 is the fee.
                                   // Raydium fees are typically 0.25% (25 BPS), with 0.22% to LPs and 0.03% to RAY buyback/burn.
            last_update_timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
            dex_type: DexType::Raydium,
        })
    }

    fn get_program_id() -> Pubkey { // Changed to return Pubkey directly
        Pubkey::from_str(RAYDIUM_LIQUIDITY_PROGRAM_V4)
            .map_err(|e| anyhow!("Failed to parse Raydium program ID: {}", e))
            .expect("Static Raydium program ID should be valid") // Panic if invalid
    }

    fn get_dex_type() -> DexType {
        DexType::Raydium
    }
}


#[derive(Deserialize, Debug)]
struct RaydiumApiResponse {
    #[serde(rename = "inputMint")]
    input_mint: String,
    #[serde(rename = "outputMint")]
    output_mint: String,
    #[serde(rename = "inAmount")]
    in_amount: String,
    #[serde(rename = "outAmount")]
    out_amount: String,
    // Raydium's /v2/sdk/token/quote might include other fields like:
    // "amountIn", "amountOut", "priceImpactPct", "lpFee", "platformFee", "route"
    // Add them here if needed for the canonical Quote struct.
    #[serde(rename = "priceImpactPct")]
    price_impact_pct: Option<f64>,
    #[serde(rename = "fee")] // Assuming 'fee' contains total fee amount in output token units, or similar
    fee_amount: Option<String>, // Or f64 if it's a number
}

#[derive(Debug, Clone)] // Added Clone
pub struct RaydiumClient {
    api_key: String, // Raydium public APIs usually don't require keys, but good to have.
    http_client: ReqwestClient,
    cache: Arc<Cache>,
    quote_cache_ttl_secs: u64,
}

impl RaydiumClient {
    pub fn new(cache: Arc<Cache>, quote_cache_ttl_secs: Option<u64>) -> Self {
        let api_key = env::var("RAYDIUM_API_KEY").unwrap_or_else(|_| {
            // Raydium's main quote endpoint is public, API key might be for other services.
            debug!("RAYDIUM_API_KEY not set (usually not required for public quotes).");
            String::new()
        });
        Self {
            api_key,
            http_client: ReqwestClient::builder()
                .timeout(Duration::from_secs(10))
                .user_agent(format!("RhodesArbBot/{}", env!("CARGO_PKG_VERSION")))
                .build()
                .unwrap_or_else(|e| {
                    warn!("Failed to build ReqwestClient for Raydium, using default: {}", e);
                    ReqwestClient::new()
                }),
            cache,
            quote_cache_ttl_secs: quote_cache_ttl_secs.unwrap_or(30), // Raydium prices can change fast; shorter TTL
        }
    }

    pub fn get_api_key(&self) -> &str {
        &self.api_key
    }
}

static RAYDIUM_RATE_LIMITER: Lazy<HttpRateLimiter> = Lazy::new(|| {
    HttpRateLimiter::new(
        5,                          // max concurrent
        Duration::from_millis(200), // min delay
        3,                          // max retries
        Duration::from_millis(500), // base backoff
        // Raydium has multiple API endpoints; this is for the v2 SDK quote.
        // No official public fallbacks usually listed for this specific endpoint.
        vec![], // Example: "https://api2.raydium.io/v2/sdk/token/quote".to_string() if one existed
    )
});

#[async_trait]
impl DexClient for RaydiumClient {
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
        let cache_prefix = "quote:raydium";

        // 1. Try to get from cache
        if let Some(cached_quote) = self.cache.get_json::<Quote>(cache_prefix, &cache_key_params).await? {
            debug!("Raydium quote cache HIT for {}->{} amount {}", input_token_mint, output_token_mint, amount_in_atomic_units);
            return Ok(cached_quote);
        }
        debug!("Raydium quote cache MISS for {}->{} amount {}", input_token_mint, output_token_mint, amount_in_atomic_units);

        // 2. If not in cache, fetch from API
        // Raydium API endpoint for quotes (v2 SDK)
        let url = format!(
            "https://api.raydium.io/v2/sdk/token/quote?inputMint={}&outputMint={}&amount={}&slippage=0", // amount is preferred over inputAmount by this endpoint
            input_token_mint, output_token_mint, amount_in_atomic_units // slippage=0 to get raw output
        );

        let request_start_time = Instant::now();
        let response_result = RAYDIUM_RATE_LIMITER
            .get_with_backoff(&self.http_client, &url, |request_url| {
                let req_builder = self.http_client.get(request_url);
                // Raydium's /v2/sdk/token/quote does not typically require an API key.
                // If using a private/paid Raydium API, add headers here.
                // .headers(headers_with_api_key("RAYDIUM_API_KEY")) // If needed
                req_builder
            })
            .await;
        
        let request_duration_ms = request_start_time.elapsed().as_millis() as u64;

        match response_result {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    let text = response.text().await.map_err(|e| anyhow!("Failed to read Raydium response text: {}", e))?;
                    debug!("Raydium API response text for {}->{}: {}", input_token_mint, output_token_mint, text);

                    // Raydium's response might be a direct object or nested, e.g., in a "data" field.
                    // Assuming it's a direct object based on common patterns for this endpoint.
                    match serde_json::from_str::<RaydiumApiResponse>(&text) {
                        Ok(api_response) => {
                            let input_amount_u64 = api_response.in_amount.parse::<u64>().map_err(|e| {
                                anyhow!("Failed to parse Raydium in_amount '{}': {}", api_response.in_amount, e)
                            })?;
                            let output_amount_u64 = api_response.out_amount.parse::<u64>().map_err(|e| {
                                anyhow!("Failed to parse Raydium out_amount '{}': {}", api_response.out_amount, e)
                            })?;

                            let quote = Quote {
                                input_token: api_response.input_mint,
                                output_token: api_response.output_mint,
                                input_amount: input_amount_u64,
                                output_amount: output_amount_u64,
                                dex: self.get_name().to_string(),
                                // Raydium's SDK quote might provide a route if it goes through multiple pools.
                                // For a simple token-to-token quote, it's direct.
                                route: vec![input_token_mint.to_string(), output_token_mint.to_string()],
                                latency_ms: Some(request_duration_ms),
                                execution_score: None, // Placeholder
                                route_path: None,      // Placeholder
                                slippage_estimate: api_response.price_impact_pct, // Use priceImpactPct if available
                            };

                            // 3. Store successful response in cache
                            if let Err(e) = self.cache.set_json(cache_prefix, &cache_key_params, &quote, Some(self.quote_cache_ttl_secs)).await {
                                warn!("Failed to cache Raydium quote for {}->{}: {}", input_token_mint, output_token_mint, e);
                            }
                            Ok(quote)
                        }
                        Err(e) => {
                            error!("Failed to deserialize Raydium quote from URL {}: {:?}. Response text: {}", url, e, text);
                            Err(anyhow!("Failed to deserialize Raydium quote from {}. Error: {}. Body: {}", url, e, text))
                        }
                    }
                } else {
                    let error_text = response.text().await.unwrap_or_else(|_| "Failed to read error response body".to_string());
                    error!("Failed to fetch Raydium quote from URL {}: Status {}. Body: {}", url, status, error_text);
                    Err(anyhow!("Failed to fetch Raydium quote from {}: {}. Body: {}", url, status, error_text))
                }
            }
            Err(e) => {
                error!("HTTP request to Raydium failed for URL {}: {}", url, e);
                Err(e)
            }
        }
    }

    fn get_supported_pairs(&self) -> Vec<(String, String)> {
        // Raydium supports a vast number of pairs.
        // Dynamically fetching or using a comprehensive list from their API/SDK is ideal.
        // Returning an empty vec implies all pairs should be attempted or fetched elsewhere.
        warn!("RaydiumClient::get_supported_pairs returning empty; dynamic fetching or larger static list recommended.");
        vec![]
    }

    fn get_name(&self) -> &str {
        "Raydium"
    }
}
