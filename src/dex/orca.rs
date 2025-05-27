// src/dex/orca.rs

use crate::dex::http_utils::HttpRateLimiter;
use crate::dex::quote::{DexClient, Quote};
use crate::utils::{DexType, PoolInfo, PoolParser as UtilsPoolParser, PoolToken};
use crate::cache::Cache;
use crate::dex::http_utils_shared::log_timed_request;
use crate::error::{ArbError, ArbResult};
use anyhow::{anyhow, Result as AnyhowResult};
use async_trait::async_trait;
use log::{debug, error, warn};
use reqwest::Client as ReqwestClient;
use reqwest::header::AUTHORIZATION;
use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;
use std::env;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use once_cell::sync::Lazy;
use super::http_utils_shared::build_auth_headers;

pub struct OrcaPoolParser;
pub const ORCA_SWAP_PROGRAM_ID_V2: &str = "9W959DqEETiGZoccp2FfeJNjCagVfgtsJy72RykeK2rK";

impl UtilsPoolParser for OrcaPoolParser {
    fn parse_pool_data(address: Pubkey, data: &[u8]) -> AnyhowResult<PoolInfo> {
        if data.len() < 100 {
            error!("Orca pool parsing failed for {} - Insufficient data length: {}", address, data.len());
            return Err(anyhow!("Data too short for Orca pool: {}", address));
        }
        warn!("Using STUB OrcaPoolParser for address {}. Implement actual parsing logic.", address);
        Ok(PoolInfo {
            address,
            name: format!("OrcaStubPool/{}", address.to_string().chars().take(6).collect::<String>()),
            token_a: PoolToken { 
                mint: Pubkey::new_unique(), 
                symbol: "TKA".to_string(), 
                decimals: 6, 
                reserve: 1_000_000_000 
            },
            token_b: PoolToken { 
                mint: Pubkey::new_unique(), 
                symbol: "TKB".to_string(), 
                decimals: 6, 
                reserve: 1_000_000_000 
            },
            fee_numerator: 30, 
            fee_denominator: 10000,
            last_update_timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
            dex_type: DexType::Orca,
        })
    }

    fn get_program_id() -> Pubkey {
        Pubkey::from_str(ORCA_SWAP_PROGRAM_ID_V2).expect("Static Orca program ID should be valid")
    }

    fn get_dex_type() -> DexType { 
        DexType::Orca 
    }
}

#[derive(Debug, Clone)]
pub struct OrcaClient {
    _api_key: String,
    http_client: ReqwestClient,
    cache: Arc<Cache>,
    quote_cache_ttl_secs: u64,
}

impl OrcaClient {
    pub fn new(cache: Arc<Cache>, quote_cache_ttl_secs: Option<u64>) -> Self {
        let api_key = env::var("ORCA_API_KEY").unwrap_or_else(|_| {
            warn!("ORCA_API_KEY not set. API calls might be limited or fail.");
            String::new()
        });

        Self {
            _api_key: api_key,
            http_client: ReqwestClient::builder()
                .timeout(Duration::from_secs(10))
                .user_agent(format!("RhodesArbBot/{}", env!("CARGO_PKG_VERSION")))
                .build()
                .unwrap_or_else(|e| { 
                    warn!("Failed to build ReqwestClient for Orca, using default: {}", e); 
                    ReqwestClient::new() 
                }),
            cache,
            quote_cache_ttl_secs: quote_cache_ttl_secs.unwrap_or(60),
        }
    }

    pub fn _get_api_key(&self) -> &str { 
        &self._api_key 
    }
}

static ORCA_RATE_LIMITER: Lazy<HttpRateLimiter> = Lazy::new(|| {
    HttpRateLimiter::new(5, Duration::from_millis(200), 3, Duration::from_millis(500), vec![])
});

#[derive(Deserialize, Debug)]
struct OrcaQuoteResponse {
    #[serde(rename = "inputMint")] 
    input_mint: String,
    #[serde(rename = "outputMint")] 
    output_mint: String,
    #[serde(rename = "inAmount")] 
    in_amount: String,
    #[serde(rename = "outAmount")] 
    out_amount: String,
    route: Option<Vec<String>>,
    #[serde(rename = "slippageBps")] 
    slippage_bps: Option<u64>,
}

#[async_trait]
impl DexClient for OrcaClient {
    async fn get_best_swap_quote(
        &self, 
        input_token_mint: &str, 
        output_token_mint: &str, 
        amount_in_atomic_units: u64
    ) -> AnyhowResult<Quote> {
        let operation_label = format!("OrcaClient_GetQuote_{}_{}", input_token_mint, output_token_mint);
        
        log_timed_request(&operation_label, async {
            let cache_key_params = [input_token_mint, output_token_mint, &amount_in_atomic_units.to_string()];
            let cache_prefix = "quote:orca";
            
            if let Ok(Some(cached_quote)) = self.cache.get_json::<Quote>(cache_prefix, &cache_key_params).await {
                debug!("Orca quote cache HIT for {}->{} amount {}", input_token_mint, output_token_mint, amount_in_atomic_units);
                return Ok(cached_quote);
            }

            debug!("Orca quote cache MISS for {}->{} amount {}", input_token_mint, output_token_mint, amount_in_atomic_units);

            let url = format!(
                "https://api.orca.so/v2/solana/quote?inputMint={}&outputMint={}&amountIn={}", 
                input_token_mint, output_token_mint, amount_in_atomic_units
            );

            let request_start_time = Instant::now();
            let api_key_option = if self._api_key.is_empty() { None } else { Some(self._api_key.as_str()) };
            let headers = build_auth_headers(api_key_option, AUTHORIZATION, Some("Bearer "));

            let response_result = ORCA_RATE_LIMITER.get_with_backoff(&url, |request_url| {
                self.http_client.get(request_url).headers(headers.clone())
            }).await;

            let request_duration_ms = request_start_time.elapsed().as_millis() as u64;

            match response_result {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        let text = response.text().await.map_err(|e| anyhow!("Failed to read Orca response text: {}", e))?;
                        debug!("Orca API response text for {}->{}: {}", input_token_mint, output_token_mint, text);
                        
                        match serde_json::from_str::<OrcaQuoteResponse>(&text) {
                            Ok(api_response) => {
                                let input_amount_u64 = api_response.in_amount.parse::<u64>()
                                    .map_err(|e| anyhow!("Parse Orca in_amount: {}", e))?;
                                let output_amount_u64 = api_response.out_amount.parse::<u64>()
                                    .map_err(|e| anyhow!("Parse Orca out_amount: {}", e))?;

                                let quote = Quote {
                                    input_token: api_response.input_mint, 
                                    output_token: api_response.output_mint,
                                    input_amount: input_amount_u64, 
                                    output_amount: output_amount_u64,
                                    dex: self.get_name().to_string(),
                                    route: api_response.route.unwrap_or_else(|| vec![
                                        input_token_mint.to_string(), 
                                        output_token_mint.to_string()
                                    ]),
                                    latency_ms: Some(request_duration_ms), 
                                    execution_score: None, 
                                    route_path: None,
                                    slippage_estimate: api_response.slippage_bps.map(|bps| bps as f64 / 10000.0),
                                };

                                if let Err(e) = self.cache.set_ex(cache_prefix, &cache_key_params, &quote, Some(self.quote_cache_ttl_secs)).await {
                                    warn!("Failed to cache Orca quote for {}->{}: {}", input_token_mint, output_token_mint, e);
                                }

                                Ok(quote)
                            }
                            Err(e) => {
                                error!("Deserialize Orca quote: URL {}, Error: {:?}, Body: {}", url, e, text);
                                Err(anyhow!("Deserialize Orca: {}. Body: {}", e, text))
                            }
                        }
                    } else {
                        let error_text = response.text().await.unwrap_or_else(|_| "No error body".to_string());
                        error!("Fetch Orca quote failed: Status {}, URL {}, Body: {}", status, url, error_text);
                        Err(anyhow!("Fetch Orca quote: Status {}, Body: {}", status, error_text))
                    }
                }
                Err(e) => { 
                    error!("HTTP request to Orca failed for URL {}: {}", url, e); 
                    Err(e) 
                }
            }
        }).await
    }

    fn get_supported_pairs(&self) -> Vec<(String, String)> {
        warn!("OrcaClient::get_supported_pairs returning placeholder data.");
        vec![(
            "So11111111111111111111111111111111111111112".to_string(), 
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string()
        )]
    }

    fn get_name(&self) -> &str { 
        "Orca" 
    }
}

impl OrcaClient {
    /// Get Orca-specific quote (returns OrcaQuoteResponse)
    pub async fn get_orca_quote(
        &self,
        input_mint: &str,
        output_mint: &str,
        amount: u64,
        slippage_bps: u16,
    ) -> Result<OrcaQuoteResponse, Box<dyn std::error::Error + Send + Sync>> {
        warn!("Using STUB Orca get_orca_quote implementation.");
        
        Ok(OrcaQuoteResponse {
            input_mint: input_mint.to_string(),
            output_mint: output_mint.to_string(),
            in_amount: amount.to_string(),
            out_amount: (amount * 99 / 100).to_string(), // Stub: 1% slippage
            route: Some(vec![input_mint.to_string(), output_mint.to_string()]),
            slippage_bps: Some(slippage_bps as u64),
        })
    }

    /// Convert OrcaQuoteResponse to generic Quote
    pub fn orca_quote_to_quote(
        &self,
        orca_quote: &OrcaQuoteResponse,
        slippage_bps: u16,
    ) -> Result<Quote, Box<dyn std::error::Error + Send + Sync>> {
        let input_amount = orca_quote.in_amount.parse::<u64>()?;
        let output_amount = orca_quote.out_amount.parse::<u64>()?;

        Ok(Quote {
            input_token: orca_quote.input_mint.clone(),
            output_token: orca_quote.output_mint.clone(),
            input_amount,
            output_amount,
            dex: "Orca".to_string(),
            route: orca_quote.route.clone().unwrap_or_default(),
            latency_ms: Some(0), // Will be set by calling code
            execution_score: None,
            route_path: None,
            slippage_estimate: Some(slippage_bps as f64 / 10000.0),
        })
    }

    /// Enhanced quote fetching with error handling
    pub async fn get_quote_with_error_handling(
        &self,
        input_mint: &str,
        output_mint: &str,
        amount: u64,
        slippage_bps: u16,
    ) -> ArbResult<Quote> {
        match self.get_orca_quote(input_mint, output_mint, amount, slippage_bps).await {
            Ok(orca_quote) => {
                match self.orca_quote_to_quote(&orca_quote, slippage_bps) {
                    Ok(quote) => Ok(quote),
                    Err(e) => Err(ArbError::DexError(format!("Failed to convert Orca quote: {}", e)))
                }
            }
            Err(e) => {
                let error_msg = e.to_string();
                if error_msg.contains("insufficient") {
                    Err(ArbError::InsufficientBalance(error_msg))
                } else if error_msg.contains("slippage") {
                    Err(ArbError::DexError(format!("Slippage too high: {}", error_msg)))
                } else if error_msg.contains("timeout") || error_msg.contains("network") {
                    Err(ArbError::NetworkError(error_msg))
                } else {
                    Err(ArbError::DexError(error_msg))
                }
            }
        }
    }
}
