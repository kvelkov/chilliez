// src/dex/raydium.rs
use crate::cache::Cache;
use crate::dex::http_utils::HttpRateLimiter;
use crate::dex::http_utils_shared::build_auth_headers;
use crate::dex::quote::{DexClient, Quote};
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{DexType, PoolInfo, PoolParser as UtilsPoolParser, PoolToken};
use anyhow::{anyhow, Result as AnyhowResult, Context};
use async_trait::async_trait;
use bytemuck::{Pod, Zeroable};
use log::{debug, error, warn};
use once_cell::sync::Lazy;
use reqwest::header::HeaderName;
use reqwest::Client as ReqwestClient;
use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;
use std::env;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const RAYDIUM_LIQUIDITY_PROGRAM_V4: &str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";

#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct FeesLayout {
    pub min_separate_numerator: u64,
    pub min_separate_denominator: u64,
    pub trade_fee_numerator: u64,
    pub trade_fee_denominator: u64,
    pub pnl_numerator: u64,
    pub pnl_denominator: u64,
    pub swap_fee_numerator: u64,
    pub swap_fee_denominator: u64,
}

// This struct is taken directly from the Raydium SDK (simplified names for clarity)
// It is 625 bytes in size and designed to be Pod.
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct LiquidityStateV4 {
    pub status: u64,
    pub nonce: u64,
    pub order_num: u64,
    pub depth: u64,
    pub base_decimal: u64,
    pub quote_decimal: u64,
    pub state: u64,
    pub reset_flag: u64,
    pub min_size: u64,
    pub vol_max_cut_ratio: u64,
    pub amount_wave: u64,
    pub base_lot_size: u64,
    pub quote_lot_size: u64,
    pub min_price_multiplier: u64,
    pub max_price_multiplier: u64,
    pub system_decimal_value: u64, // 16 * 8 = 128 bytes

    pub fees: FeesLayout, // 64 bytes. Total = 192 bytes

    pub base_need_take_pnl: u64,    // For OutputData part
    pub quote_need_take_pnl: u64,
    pub surplus_base_token: u64,
    pub surplus_quote_token: u64,   // +32 bytes. Total = 224 bytes
    pub base_total_pnl: u128,
    pub quote_total_pnl: u128,      // +32 bytes. Total = 256 bytes
    pub pool_open_time: u64,
    pub punishment_numerator: u64,
    pub punishment_denominator: u64, // +24 bytes. Total = 280 bytes
    pub amm_owner: Pubkey,          // +32 bytes. Total = 312 bytes

    pub base_reserve_amount: u64,
    pub quote_reserve_amount: u64,
    pub evaluate_base_amount: u64,
    pub evaluate_quote_amount: u64, // +32 bytes. Total = 344 bytes
    pub target_orders: Pubkey,      // +32 bytes. Total = 376 bytes
    pub lp_mint: Pubkey,            // +32 bytes. Total = 408 bytes

    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
    pub market_id: Pubkey,
    pub market_program_id: Pubkey,  // +128 bytes. Total = 536 bytes
    
    // Final padding to make the struct exactly 625 bytes.
    // Original explicit padding was 89 bytes (padding0: [u64;11] + padding1: u8).
    // Compiler makes struct 640 bytes. Implicit padding = 640 - (536 + 89) = 15 bytes.
    // New total explicit padding = 89 + 15 = 104 bytes.
    // Use types that are Pod. [u64; N] and u8 are Pod.
    pub padding: [u64; 13], // 13 * 8 = 104 bytes. Total size: 536 + 104 = 640 bytes.
}
// Statically assert that the size of LiquidityStateV4 is 625 bytes.
const _: [(); std::mem::size_of::<LiquidityStateV4>()] = [(); 640];

// Minimal Serum Market State to extract mints
#[derive(Clone, Copy, Debug)] // Removed Pod and Zeroable derives
#[repr(C)]
struct SerumMarketStateMinimal {
    // Other fields before base_mint are skipped by using a byte array for padding
    _padding0: [u8; 53], // Account flags (5) + Own Address (32) + Vault Signer Nonce (8) + Coin Mint (32) -> We need Coin Mint
    pub base_mint: Pubkey, // Offset 53
    _padding1: [u8; 0], // PC Mint is next
    pub quote_mint: Pubkey, // Offset 53 + 32 = 85
    // ... other fields
}
const SERUM_MARKET_BASE_MINT_OFFSET: usize = 53;
const SERUM_MARKET_QUOTE_MINT_OFFSET: usize = 85;


pub struct RaydiumPoolParser;

#[async_trait]
impl UtilsPoolParser for RaydiumPoolParser {
    async fn parse_pool_data(
        &self,
        address: Pubkey,
        data: &[u8],
        rpc_client: &Arc<SolanaRpcClient>,
    ) -> AnyhowResult<PoolInfo> {
        const EXPECTED_ON_CHAIN_SIZE: usize = std::mem::size_of::<LiquidityStateV4>(); // Should be 625

        if data.len() < EXPECTED_ON_CHAIN_SIZE {
            return Err(anyhow!(
                "Data too short for Raydium V4 pool {}: expected {} bytes, got {}. Data prefix: {:?}",
                address,
                EXPECTED_ON_CHAIN_SIZE,
                data.len(),
                &data[..std::cmp::min(data.len(), 64)]
            ));
        }

        let state: &LiquidityStateV4 = bytemuck::from_bytes(&data[..EXPECTED_ON_CHAIN_SIZE]);

        // Fetch Serum market account data
        let market_account_data = rpc_client.get_account_data(&state.market_id).await
            .with_context(|| format!("Failed to fetch Serum market account data for Raydium pool {}, market ID {}", address, state.market_id))?;

        // Ensure market data is long enough
        if market_account_data.len() < SERUM_MARKET_QUOTE_MINT_OFFSET + 32 {
            return Err(anyhow!(
                "Serum market account data for market {} is too short. Expected at least {} bytes, got {}.",
                state.market_id, SERUM_MARKET_QUOTE_MINT_OFFSET + 32, market_account_data.len()
            ));
        }

        // Extract base_mint and quote_mint using bytemuck::from_bytes_with_offsets or direct slicing
        // It's safer to slice and then cast to Pubkey to avoid misalignments if the struct isn't perfectly matching.
        let base_mint_bytes: &[u8; 32] = market_account_data[SERUM_MARKET_BASE_MINT_OFFSET..SERUM_MARKET_BASE_MINT_OFFSET + 32]
            .try_into()
            .map_err(|_| anyhow!("Failed to slice base_mint from market {}", state.market_id))?;
        let base_mint = Pubkey::new_from_array(*base_mint_bytes);

        let quote_mint_bytes: &[u8; 32] = market_account_data[SERUM_MARKET_QUOTE_MINT_OFFSET..SERUM_MARKET_QUOTE_MINT_OFFSET + 32]
            .try_into()
            .map_err(|_| anyhow!("Failed to slice quote_mint from market {}", state.market_id))?;
        let quote_mint = Pubkey::new_from_array(*quote_mint_bytes);

        // Fetch decimals for the actual mints
        let (base_decimals, quote_decimals) = tokio::try_join!(
            rpc_client.get_token_mint_decimals(&base_mint),
            rpc_client.get_token_mint_decimals(&quote_mint)
        ).with_context(|| format!("Failed to fetch token decimals for Raydium pool {}, market {}", address, state.market_id))?;

        Ok(PoolInfo {
            address,
            name: format!("RaydiumV4-{}", address.to_string().chars().take(4).collect::<String>()),
            token_a: PoolToken {
                mint: base_mint,
                symbol: format!("BASE-{}", base_mint.to_string().chars().take(4).collect::<String>()), // Placeholder symbol
                decimals: base_decimals,
                reserve: state.base_reserve_amount,
            },
            token_b: PoolToken {
                mint: quote_mint,
                symbol: format!("QUOTE-{}", quote_mint.to_string().chars().take(4).collect::<String>()), // Placeholder symbol
                decimals: quote_decimals,
                reserve: state.quote_reserve_amount,
            },
            fee_numerator: state.fees.swap_fee_numerator,
            fee_denominator: state.fees.swap_fee_denominator,
            last_update_timestamp: state.pool_open_time,
            dex_type: DexType::Raydium,
        })
    }

    fn get_program_id(&self) -> Pubkey {
        Pubkey::from_str(RAYDIUM_LIQUIDITY_PROGRAM_V4).unwrap()
    }
}

// --- Client implementation (RaydiumClient) ---
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
    #[serde(rename = "priceImpactPct")]
    price_impact_pct: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct RaydiumClient {
    api_key: String,
    http_client: ReqwestClient,
    cache: Arc<Cache>,
    quote_cache_ttl_secs: u64,
}

impl RaydiumClient {
    pub fn new(cache: Arc<Cache>, quote_cache_ttl_secs: Option<u64>) -> Self {
        let api_key = env::var("RAYDIUM_API_KEY").unwrap_or_else(|_| {
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
            quote_cache_ttl_secs: quote_cache_ttl_secs.unwrap_or(30),
        }
    }
}

static RAYDIUM_RATE_LIMITER: Lazy<HttpRateLimiter> = Lazy::new(|| {
    HttpRateLimiter::new(
        5,
        Duration::from_millis(200),
        3,
        Duration::from_millis(500),
        vec![],
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

        if let Ok(Some(cached_quote)) = self
            .cache
            .get_json::<Quote>(cache_prefix, &cache_key_params)
            .await
        {
            debug!(
                "Raydium quote cache HIT for {}->{} amount {}",
                input_token_mint, output_token_mint, amount_in_atomic_units
            );
            return Ok(cached_quote);
        }
        debug!(
            "Raydium quote cache MISS for {}->{} amount {}",
            input_token_mint, output_token_mint, amount_in_atomic_units
        );

        let url = format!(
            "https://api.raydium.io/v2/sdk/token/quote?inputMint={}&outputMint={}&amount={}&slippage=0",
            input_token_mint, output_token_mint, amount_in_atomic_units
        );

        let api_key_option = if self.api_key.is_empty() {
            None
        } else {
            Some(self.api_key.as_str())
        };

        let headers = build_auth_headers(api_key_option, HeaderName::from_static("x-api-key"), None);

        let request_start_time = Instant::now();
        let response_result = RAYDIUM_RATE_LIMITER
            .get_with_backoff(&url, |request_url| {
                self.http_client.get(request_url).headers(headers.clone())
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
                        .map_err(|e| anyhow!("Failed to read Raydium response text: {}", e))?;
                    debug!(
                        "Raydium API response text for {}->{}: {}",
                        input_token_mint, output_token_mint, text
                    );

                    match serde_json::from_str::<RaydiumApiResponse>(&text) {
                        Ok(api_response) => {
                            let input_amount_u64 = api_response.in_amount.parse::<u64>().map_err(|e| {
                                anyhow!("Failed to parse Raydium in_amount '{}': {}", api_response.in_amount, e)
                            })?;
                            let output_amount_u64 = api_response.out_amount.parse::<u64>().map_err(|e| {
                                anyhow!("Failed to parse Raydium out_amount '{}': {}", api_response.out_amount, e)
                            })?;

                            let quote_obj = Quote {
                                input_token: api_response.input_mint,
                                output_token: api_response.output_mint,
                                input_amount: input_amount_u64,
                                output_amount: output_amount_u64,
                                dex: self.get_name().to_string(),
                                route: vec![input_token_mint.to_string(), output_token_mint.to_string()],
                                latency_ms: Some(request_duration_ms),
                                execution_score: None,
                                route_path: None,
                                slippage_estimate: api_response.price_impact_pct,
                            };

                            if let Err(e) = self.cache.set_ex(
                                cache_prefix,
                                &cache_key_params,
                                &quote_obj,
                                Some(self.quote_cache_ttl_secs),
                            )
                            .await
                            {
                                warn!(
                                    "Failed to cache Raydium quote for {}->{}: {}",
                                    input_token_mint, output_token_mint, e
                                );
                            }
                            Ok(quote_obj)
                        }
                        Err(e) => {
                            error!(
                                "Failed to deserialize Raydium quote from URL {}: {:?}. Response text: {}",
                                url, e, text
                            );
                            Err(anyhow!("Failed to deserialize Raydium quote from {}. Error: {}. Body: {}", url, e, text))
                        }
                    }
                } else {
                    let error_text = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "Failed to read error response body".to_string());
                    error!(
                        "Failed to fetch Raydium quote from URL {}: Status {}. Body: {}",
                        url, status, error_text
                    );
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
        warn!("RaydiumClient::get_supported_pairs returning empty; dynamic fetching or a larger static list may be implemented.");
        vec![]
    }

    fn get_name(&self) -> &str {
        "Raydium"
    }
}