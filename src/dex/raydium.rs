use crate::dex::http_utils::HttpRateLimiter;
use crate::dex::pool::{DexType, PoolInfo, PoolParser, PoolToken};
use crate::dex::quote::{DexClient, Quote}; // Use the shared Quote
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use log::{error, warn};
use reqwest::Client;
use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;
use std::env;
use std::str::FromStr;
use std::time::Duration;
use std::time::Instant;

// Define a struct that matches Raydium's API response for a quote
#[derive(Deserialize, Debug)]
struct RaydiumApiResponse {
    // These field names MUST match the JSON from Raydium's API
    #[serde(rename = "inputMint")]
    input_mint: String,
    #[serde(rename = "outputMint")]
    output_mint: String,
    #[serde(rename = "inAmount")]
    in_amount: String, // Raydium often returns amounts as strings
    #[serde(rename = "outAmount")]
    out_amount: String, // Raydium often returns amounts as strings
}

pub struct RaydiumClient {
    api_key: String,
    http_client: Client,
}

impl RaydiumClient {
    pub fn new() -> Self {
        let api_key = env::var("RAYDIUM_API_KEY").unwrap_or_else(|_| {
            warn!("RAYDIUM_API_KEY not set. This might not be needed for Raydium public quotes.");
            String::new()
        });
        Self {
            api_key,
            http_client: Client::new(),
        }
    }

    fn build_request_with_api_key(&self, url: &str) -> reqwest::RequestBuilder {
        let mut req = self.http_client.get(url);
        if !self.api_key.is_empty() {
            req = req.header("api-key", &self.api_key);
        }
        req
    }
}

static RAYDIUM_RATE_LIMITER: Lazy<HttpRateLimiter> = Lazy::new(|| {
    HttpRateLimiter::new(
        4, // max concurrent
        Duration::from_millis(200), // min delay between requests
        3, // max retries
        Duration::from_millis(250), // base backoff
        vec![
            "https://api.raydium.io/v2/sdk/token/quote".to_string(), // fallback(s) if any
        ],
    )
});

#[async_trait]
impl DexClient for RaydiumClient {
    async fn get_best_swap_quote(
        &self,
        input_token: &str,
        output_token: &str,
        amount: u64,
    ) -> Result<Quote> {
        let url = format!(
            "https://api.raydium.io/v2/sdk/token/quote?inputMint={}&outputMint={}&inputAmount={}",
            input_token, output_token, amount
        );
        let request_start_time = Instant::now();
        let response = RAYDIUM_RATE_LIMITER
            .get_with_backoff(&self.http_client, &url, |u| self.build_request_with_api_key(u))
            .await?;
        let request_duration_ms = request_start_time.elapsed().as_millis() as u64;
        if response.status().is_success() {
            match response.json::<RaydiumApiResponse>().await {
                Ok(api_response) => {
                    let input_amount_u64 = api_response.in_amount.parse::<u64>().map_err(|e| {
                        anyhow!(
                            "Failed to parse Raydium in_amount '{}': {}",
                            api_response.in_amount,
                            e
                        )
                    })?;
                    let output_amount_u64 =
                        api_response.out_amount.parse::<u64>().map_err(|e| {
                            anyhow!(
                                "Failed to parse Raydium out_amount '{}': {}",
                                api_response.out_amount,
                                e
                            )
                        })?;
                    Ok(Quote {
                        input_token: api_response.input_mint,
                        output_token: api_response.output_mint,
                        input_amount: input_amount_u64,
                        output_amount: output_amount_u64,
                        dex: self.get_name().to_string(),
                        route: vec![],
                        latency_ms: Some(request_duration_ms),
                        execution_score: None,
                        route_path: None,
                        slippage_estimate: None,
                    })
                }
                Err(e) => {
                    error!("Failed to deserialize Raydium quote from URL {}: {:?}. Check RaydiumApiResponse struct against actual API output.", url, e);
                    Err(anyhow!(
                        "Failed to deserialize Raydium quote from {}. Error: {}",
                        url,
                        e
                    ))
                }
            }
        } else {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error response body".to_string());
            error!(
                "Failed to fetch Raydium quote from URL {}: Status {}. Body: {}",
                url, status, error_text
            );
            Err(anyhow!(
                "Failed to fetch Raydium quote from {}: {}. Body: {}",
                url,
                status,
                error_text
            ))
        }
    }

    fn get_supported_pairs(&self) -> Vec<(String, String)> {
        vec![]
    }

    fn get_name(&self) -> &str {
        "Raydium"
    }
}

// Define RaydiumPoolParser
pub struct RaydiumPoolParser;
pub const RAYDIUM_LIQUIDITY_PROGRAM_ID: &str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";

impl PoolParser for RaydiumPoolParser {
    fn parse_pool_data(address: Pubkey, data: &[u8]) -> Result<PoolInfo> {
        if data.len() < 300 {
            error!(
                "Pool parsing failed for {} - Insufficient data length: {}",
                address,
                data.len()
            );
            return Err(anyhow!("Data too short for Raydium pool: {}", address));
        }

        Ok(PoolInfo {
            address,
            name: format!(
                "RaydiumPool/{}",
                address.to_string().chars().take(6).collect::<String>()
            ),
            token_a: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "TKA".to_string(),
                decimals: 6,
                reserve: 1_000_000,
            },
            token_b: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "TKB".to_string(),
                decimals: 6,
                reserve: 1_000_000,
            },
            fee_numerator: 25,
            fee_denominator: 10000,
            last_update_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            dex_type: DexType::Raydium,
        })
    }

    fn get_program_id() -> Pubkey {
        Pubkey::from_str(RAYDIUM_LIQUIDITY_PROGRAM_ID).unwrap()
    }

    fn get_dex_type() -> DexType {
        DexType::Raydium
    }
}
