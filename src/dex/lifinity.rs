// src/dex/lifinity.rs
//! Lifinity client and parser for obtaining swap quotes and parsing on-chain pool data.

use crate::cache::Cache;
use crate::dex::http_utils::HttpRateLimiter;
use crate::dex::quote::{DexClient, Quote as CanonicalQuote};
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{DexType, PoolInfo, PoolParser as UtilsPoolParser, PoolToken};
use anyhow::{anyhow, Result as AnyhowResult};
use async_trait::async_trait;
use bytemuck::{Pod, Zeroable};
use log::{debug, error, info, warn};
use once_cell::sync::Lazy;
use reqwest::Client as ReqwestClient;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
//use solana_sdk::program_pack::Pack; // Import the Pack trait
//use spl_token::state::Mint;
use std::env;
use std::str::FromStr;
use std::sync::Arc; // Keep Arc
use std::time::{Duration, Instant}; // Remove SystemTime, UNIX_EPOCH from here

#[repr(C)]
#[derive(Clone, Debug, Copy, Pod, Zeroable)]
pub struct LifinityPoolState {
    pub concentration: u64,
    pub fee_bps: u64,
    pub token_a_mint: Pubkey,
    pub token_b_mint: Pubkey,
    pub token_a_vault: Pubkey,
    pub token_b_vault: Pubkey,
    //... other fields
}

pub struct LifinityPoolParser;
pub const LIFINITY_PROGRAM_ID: &str = "EewxydAPCCVuNEyrVN68PuSYdQ7wKn27V9GJEbpNcHcn";

#[async_trait]
impl UtilsPoolParser for LifinityPoolParser {
    async fn parse_pool_data(
        &self,
        address: Pubkey,
        data: &[u8],
        rpc_client: &Arc<SolanaRpcClient>,
    ) -> AnyhowResult<PoolInfo> {
        let expected_size = std::mem::size_of::<LifinityPoolState>();
        if data.len() < expected_size {
            return Err(anyhow!(
                "Data too short for Lifinity pool {}: expected at least {} bytes, got {}",
                address,
                expected_size,
                data.len()
            ));
        }

        let state: &LifinityPoolState = bytemuck::from_bytes(&data[..expected_size]);

        info!("Parsing Lifinity pool data for address: {}", address);

        // Fetch reserves and mint account data in parallel
        // Use async blocks to convert errors to anyhow::Error for try_join!
        let (reserve_a_bytes, reserve_b_bytes, decimals_a, decimals_b): (Vec<u8>, Vec<u8>, u8, u8) = tokio::try_join!(
            async {
                rpc_client.primary_client.get_account_data(&state.token_a_vault).await.map_err(anyhow::Error::from)
            },
            async {
                rpc_client.primary_client.get_account_data(&state.token_b_vault).await.map_err(anyhow::Error::from)
            },
            async {
                rpc_client.get_token_mint_decimals(&state.token_a_mint).await.map_err(anyhow::Error::from)
            },
            async {
                rpc_client.get_token_mint_decimals(&state.token_b_mint).await.map_err(anyhow::Error::from)
            }
        )?;

        // Convert Vec<u8> to u64 for reserves
        let reserve_a = u64::from_le_bytes(reserve_a_bytes[..8].try_into().unwrap());
        let reserve_b = u64::from_le_bytes(reserve_b_bytes[..8].try_into().unwrap());

        Ok(PoolInfo {
            address,
            name: format!("Lifinity/{}", address),
            token_a: PoolToken {
                mint: state.token_a_mint,
                symbol: "TKA".to_string(),
                decimals: decimals_a,
                reserve: reserve_a,
            },
            token_b: PoolToken {
                mint: state.token_b_mint,
                symbol: "TKB".to_string(),
                decimals: decimals_b,
                reserve: reserve_b,
            },
            fee_numerator: state.fee_bps,
            fee_denominator: 10000,
            last_update_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            dex_type: DexType::Lifinity,
        })
    }

    fn get_program_id(&self) -> Pubkey {
        Pubkey::from_str(LIFINITY_PROGRAM_ID).unwrap()
    }
}
static LIFINITY_RATE_LIMITER: Lazy<HttpRateLimiter> =
    Lazy::new(|| HttpRateLimiter::new(4, Duration::from_millis(250), 3, Duration::from_millis(500), vec![]));

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



#[derive(Debug, Clone)]
pub struct LifinityClient {
    api_key: String,
    http_client: ReqwestClient,
    cache: Arc<Cache>,
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
                    warn!(
                        "Failed to build ReqwestClient for Lifinity, using default: {}",
                        e
                    );
                    ReqwestClient::new()
                }),
            cache,
            quote_cache_ttl_secs: quote_cache_ttl_secs.unwrap_or(60),
        }
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

        if let Ok(Some(cached_quote)) = self
            .cache
            .get_json::<CanonicalQuote>(cache_prefix, &cache_key_params)
            .await
        {
            debug!(
                "Lifinity quote cache HIT for {}->{} amount {}",
                input_token_mint, output_token_mint, amount_in_atomic_units
            );
            return Ok(cached_quote);
        }
        debug!(
            "Lifinity quote cache MISS for {}->{} amount {}",
            input_token_mint, output_token_mint, amount_in_atomic_units
        );

        let url = format!(
            "https://api.lifinity.io/quote?input={}&output={}&amount={}",
            input_token_mint, output_token_mint, amount_in_atomic_units
        );
        info!("Requesting Lifinity quote from URL: {}", url);

        let request_start_time = Instant::now();
        let response_result = LIFINITY_RATE_LIMITER
            .get_with_backoff(&url, |request_url| {
                let mut req_builder = self.http_client.get(request_url);
                if !self.api_key.is_empty() {
                    req_builder = req_builder.header("api-key", &self.api_key);
                }
                req_builder
            })
            .await;
        let request_duration_ms = request_start_time.elapsed().as_millis() as u64;

        match response_result {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    let text = response
                        .text()
                        .await
                        .map_err(|e| anyhow!("Failed to read Lifinity response text: {}", e))?;
                    debug!(
                        "Lifinity API response text for {}->{}: {}",
                        input_token_mint, output_token_mint, text
                    );

                    match serde_json::from_str::<LifinityApiQuote>(&text) {
                        Ok(api_quote) => {
                            let canonical_quote = CanonicalQuote {
                                input_token: api_quote.input_token.clone(),
                                output_token: api_quote.output_token.clone(),
                                input_amount: api_quote.input_amount,
                                output_amount: api_quote.output_amount,
                                dex: self.get_name().to_string(),
                                route: vec![api_quote.input_token, api_quote.output_token],
                                latency_ms: Some(request_duration_ms),
                                execution_score: None,
                                route_path: None,
                                slippage_estimate: None,
                            };

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
                                    "Failed to cache Lifinity quote for {}->{}: {}",
                                    input_token_mint, output_token_mint, e
                                );
                            }
                            Ok(canonical_quote)
                        }
                        Err(e) => {
                            error!(
                                "Failed to deserialize Lifinity quote: URL {}, Error: {:?}, Body: {}",
                                url, e, text
                            );
                            Err(anyhow!(
                                "Deserialize Lifinity quote failed: {}. Body: {}",
                                e,
                                text
                            ))
                        }
                    }
                } else {
                    let error_text = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "No error body".to_string());
                    error!(
                        "Fetch Lifinity quote failed: Status {}, URL {}, Body: {}",
                        status, url, error_text
                    );
                    Err(anyhow!(
                        "Fetch Lifinity quote: Status {}, Body: {}",
                        status,
                        error_text
                    ))
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