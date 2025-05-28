// src/dex/orca.rs
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
use solana_sdk::pubkey::Pubkey; // Added import for Pubkey
use reqwest::Client as ReqwestClient;
use serde::{Deserialize, Serialize};
use std::env;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const ORCA_SWAP_PROGRAM_ID_V2: &str = "9W959DqEETiGZocYWCQPaJ6sBmUzgfxXfqGeTEdp3aQP";

// --- Orca Pool Parser V2 ---
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct OrcaPoolStateV2 {
    // Based on common Orca V2 pool layouts. Ensure this matches the actual structure.
    pub _account_type: u8, // Typically 2 for Pool
    pub _bump_seed: u8,
    pub _padding0: [u8; 6],
    pub token_a_mint: Pubkey, // Uses the imported Pubkey
    pub token_b_mint: Pubkey, // Uses the imported Pubkey
    pub token_a_vault: Pubkey, // Uses the imported Pubkey
    pub token_b_vault: Pubkey, // Uses the imported Pubkey
    pub lp_token_mint: Pubkey, // Uses the imported Pubkey
    // ... other fields like fees, curve type etc.
    // For simplicity, focusing on mints and vaults for PoolInfo.
    // Ensure the total size matches the on-chain account data size you expect.
    // This is a minimal example. A full struct would be larger.
    pub _padding1: [u8; 128], // 128 bytes. [u8; 128] is Pod.
    pub _padding2: [u8; 30],  // 30 bytes. [u8; 30] is Pod. Total padding = 158 bytes.
}
const ORCA_POOL_STATE_V2_SIZE: usize = std::mem::size_of::<OrcaPoolStateV2>(); // Should be 326 if padding is correct

pub struct OrcaPoolParser;

#[async_trait]
impl UtilsPoolParser for OrcaPoolParser {
    async fn parse_pool_data(
        &self,
        address: Pubkey,
        data: &[u8], // address is Pubkey
        rpc_client: &Arc<SolanaRpcClient>,
    ) -> AnyhowResult<PoolInfo> {
        if data.len() < ORCA_POOL_STATE_V2_SIZE {
            return Err(anyhow!(
                "Data too short for Orca V2 pool {}: expected {} bytes, got {}",
                address,
                ORCA_POOL_STATE_V2_SIZE,
                data.len()
            ));
        }

        let state: &OrcaPoolStateV2 = bytemuck::from_bytes(&data[..ORCA_POOL_STATE_V2_SIZE]);

        // Fetch reserves and mint decimals in parallel
        let (reserve_a, reserve_b, decimals_a, decimals_b): (u64, u64, u8, u8) = tokio::try_join!(
            rpc_client.get_token_account_balance(&state.token_a_vault),
            rpc_client.get_token_account_balance(&state.token_b_vault),
            rpc_client.get_token_mint_decimals(&state.token_a_mint),
            rpc_client.get_token_mint_decimals(&state.token_b_mint)
        )?;

        Ok(PoolInfo {
            address,
            name: format!("OrcaV2-{}", address.to_string().chars().take(4).collect::<String>()),
            token_a: PoolToken {
                mint: state.token_a_mint,
                symbol: format!("TKA-{}", state.token_a_mint.to_string().chars().take(4).collect::<String>()),
                decimals: decimals_a,
                reserve: reserve_a,
            },
            token_b: PoolToken {
                mint: state.token_b_mint,
                symbol: format!("TKB-{}", state.token_b_mint.to_string().chars().take(4).collect::<String>()),
                decimals: decimals_b,
                reserve: reserve_b,
            },
            // Placeholder fees, actual fees depend on the curve and pool config
            fee_numerator: 30, // e.g., 0.30%
            fee_denominator: 10000,
            last_update_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            dex_type: DexType::Orca,
        })
    }

    fn get_program_id(&self) -> Pubkey {
        ORCA_SWAP_PROGRAM_ID_V2.parse().unwrap() // Pubkey::from_str is an alias for str::parse::<Pubkey>
    }
}

// --- Orca API Client (for V1 API / Legacy Pools) ---

#[derive(Deserialize, Serialize, Debug, Clone)]
struct OrcaApiRoute {
    #[serde(rename = "outAmount")]
    out_amount: String,
    // ... other fields like `amount`, `networkFee`, `minAmountOut` might be present
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct OrcaApiQuoteResponse {
    route: Vec<OrcaApiRoute>, // Orca API returns an array of routes, usually we take the first.
    // ... other top-level fields
}

static ORCA_API_RATE_LIMITER: Lazy<HttpRateLimiter> = Lazy::new(|| {
    HttpRateLimiter::new(
        5, // Max concurrent requests
        Duration::from_millis(200), // min_delay
        3, // max_retries
        Duration::from_millis(500), // base_backoff
        vec![], // fallback_urls
    )
});

#[derive(Debug, Clone)]
pub struct OrcaClient {
    api_key: String,
    http_client: ReqwestClient,
    cache: Arc<Cache>,
    quote_cache_ttl_secs: u64,
}

impl OrcaClient {
    pub fn new(cache: Arc<Cache>, quote_cache_ttl_secs: Option<u64>) -> Self {
        let api_key = env::var("ORCA_API_KEY").unwrap_or_else(|_| {
            warn!("ORCA_API_KEY not set. This might affect API access for Orca legacy pools.");
            String::new()
        });
        Self {
            api_key,
            http_client: ReqwestClient::builder()
                .timeout(Duration::from_secs(10))
                .user_agent(format!("RhodesArbBot/{}", env!("CARGO_PKG_VERSION")))
                .build()
                .unwrap_or_else(|e| {
                    warn!("Failed to build ReqwestClient for Orca, using default: {}", e);
                    ReqwestClient::new()
                }),
            cache,
            quote_cache_ttl_secs: quote_cache_ttl_secs.unwrap_or(60), // Example TTL
        }
    }
}

#[async_trait]
impl DexClient for OrcaClient {
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
        let cache_prefix = "quote:orca_v1";

        if let Ok(Some(cached_quote)) = self.cache.get_json::<CanonicalQuote>(cache_prefix, &cache_key_params).await {
            debug!(
                "Orca (Legacy) quote cache HIT for {}->{} amount {}",
                input_token_mint, output_token_mint, amount_in_atomic_units
            );
            return Ok(cached_quote);
        }
        debug!(
            "Orca (Legacy) quote cache MISS for {}->{} amount {}",
            input_token_mint, output_token_mint, amount_in_atomic_units
        );

        let url = format!(
            "https://api.orca.so/v1/quote/routes?input={}&output={}&amount={}&slippage=0.5", // slippage can be configurable
            input_token_mint, output_token_mint, amount_in_atomic_units
        );
        info!("Requesting Orca (Legacy) quote from URL: {}", url);

        let request_start_time = Instant::now();
        let response_result = ORCA_API_RATE_LIMITER // Now correctly refers to the static variable
            .get_with_backoff(&url, |request_url| {
                let mut req_builder = self.http_client.get(request_url);
                if !self.api_key.is_empty() {
                    req_builder = req_builder.header("X-API-KEY", &self.api_key);
                }
                req_builder
            })
            .await;
        let request_duration_ms = request_start_time.elapsed().as_millis() as u64;

        match response_result {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    let text = response.text().await.map_err(|e| anyhow!("Failed to read Orca (Legacy) response text: {}", e))?;
                    debug!("Orca (Legacy) API response text for {}->{}: {}", input_token_mint, output_token_mint, text);

                    match serde_json::from_str::<OrcaApiQuoteResponse>(&text) {
                        Ok(api_response) => {
                            if let Some(best_route) = api_response.route.get(0) {
                                let output_amount_u64 = best_route.out_amount.parse::<u64>()
                                    .map_err(|e| anyhow!("Parse Orca (Legacy) out_amount: {}", e))?;

                                let canonical_quote = CanonicalQuote {
                                    input_token: input_token_mint.to_string(),
                                    output_token: output_token_mint.to_string(),
                                    input_amount: amount_in_atomic_units,
                                    output_amount: output_amount_u64,
                                    dex: self.get_name().to_string(),
                                    route: vec![input_token_mint.to_string(), output_token_mint.to_string()], // Simplified route
                                    latency_ms: Some(request_duration_ms),
                                    execution_score: None,
                                    route_path: None, // API might provide more detailed path
                                    slippage_estimate: None, // API might provide this
                                };

                                if let Err(e) = self.cache.set_ex(cache_prefix, &cache_key_params, &canonical_quote, Some(self.quote_cache_ttl_secs)).await {
                                    warn!("Failed to cache Orca (Legacy) quote for {}->{}: {}", input_token_mint, output_token_mint, e);
                                }
                                Ok(canonical_quote)
                            } else {
                                Err(anyhow!("Orca (Legacy) API returned no routes for {}->{}", input_token_mint, output_token_mint))
                            }
                        }
                        Err(e) => {
                            error!("Deserialize Orca (Legacy) quote: URL {}, Error: {:?}, Body: {}", url, e, text);
                            Err(anyhow!("Deserialize Orca (Legacy) quote error: {}. Body: {}", e, text))
                        }
                    }
                } else {
                    let error_text = response.text().await.unwrap_or_else(|_| "No error body".to_string());
                    error!("Fetch Orca (Legacy) quote failed: Status {}, URL {}, Body: {}", status, url, error_text);
                    Err(anyhow!("Fetch Orca (Legacy) quote: Status {}, Body: {}", status, error_text))
                }
            }
            Err(e) => {
                error!("HTTP request to Orca (Legacy) failed for URL {}: {}", url, e);
                Err(anyhow!("HTTP request to Orca (Legacy) failed: {}", e))
            }
        }
    }

    fn get_supported_pairs(&self) -> Vec<(String, String)> {
        // Placeholder: Implement actual logic if needed, e.g., from API or config
        warn!("OrcaClient::get_supported_pairs returning empty list for legacy pools.");
        vec![]
    }

    fn get_name(&self) -> &str {
        "OrcaLegacy" // Differentiate from Whirlpool if necessary
    }
}