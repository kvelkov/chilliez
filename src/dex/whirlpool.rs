use crate::arbitrage::headers_with_api_key;
use crate::dex::http_utils::HttpRateLimiter;
use crate::dex::quote::{DexClient, Quote};
use crate::utils::{DexType, PoolInfo, PoolParser, PoolToken};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use log::{error, warn};
use once_cell::sync::Lazy;
use reqwest::Client;
use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;
use std::env;
use std::str::FromStr;
use std::time::Duration;
use std::time::Instant; // For measuring request latency

static WHIRLPOOL_RATE_LIMITER: Lazy<HttpRateLimiter> = Lazy::new(|| {
    HttpRateLimiter::new(
        4,
        Duration::from_millis(200),
        3,
        Duration::from_millis(250),
        vec!["https://api.orca.so/whirlpools/quote".to_string()],
    )
});

#[derive(Deserialize, Debug)]
struct WhirlpoolApiResponse {
    #[serde(rename = "inputMint")]
    input_mint: String,
    #[serde(rename = "outputMint")]
    output_mint: String,
    #[serde(rename = "inAmount")]
    in_amount: String, // Using string to handle various number formats
    #[serde(rename = "outAmount")]
    out_amount: String, // Using string to handle various number formats
                        // ... other fields specific to Whirlpool API
}

pub struct WhirlpoolClient {
    api_key: String,
    http_client: Client,
}

impl WhirlpoolClient {
    pub fn new() -> Self {
        let api_key = env::var("WHIRLPOOL_API_KEY").unwrap_or_else(|_| {
            warn!("WHIRLPOOL_API_KEY not set. This might affect API functionality.");
            String::new()
        });
        Self {
            api_key,
            http_client: Client::new(),
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
        input_token: &str,
        output_token: &str,
        amount: u64,
    ) -> anyhow::Result<Quote> {
        // Use the rate limiter's get_with_backoff to wrap the HTTP request
        let url = format!(
            "https://api.orca.so/whirlpools/quote?inputMint={}&outputMint={}&amount={}",
            input_token, output_token, amount
        );
        let request_start_time = Instant::now();
        let response = WHIRLPOOL_RATE_LIMITER
            .get_with_backoff(&self.http_client, &url, |u| {
                self.http_client
                    .get(u)
                    .headers(headers_with_api_key("WHIRLPOOL_API_KEY"))
            })
            .await?;
        let request_duration_ms = request_start_time.elapsed().as_millis() as u64;
        let status = response.status();
        if status.is_success() {
            let response_text = response.text().await.unwrap_or_default();
            match serde_json::from_str::<WhirlpoolApiResponse>(&response_text) {
                Ok(api_response) => {
                    let input_amount_u64 = api_response.in_amount.parse::<u64>().map_err(|e| {
                        anyhow!(
                            "Failed to parse Whirlpool in_amount '{}': {}",
                            api_response.in_amount,
                            e
                        )
                    })?;
                    let output_amount_u64 =
                        api_response.out_amount.parse::<u64>().map_err(|e| {
                            anyhow!(
                                "Failed to parse Whirlpool out_amount '{}': {}",
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
                    error!(
                        "Failed to deserialize Whirlpool quote from URL {}: {:?}. Response body: {}",
                        url, e, response_text
                    );
                    Err(anyhow!(
                        "Failed to deserialize Whirlpool quote from {}. Error: {}. Body: {}",
                        url,
                        e,
                        response_text
                    ))
                }
            }
        } else {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error response body".to_string());
            error!(
                "Failed to fetch Whirlpool quote from URL {}: Status {}. Body: {}",
                url, status, error_text
            );
            Err(anyhow!(
                "Failed to fetch Whirlpool quote from {}: {}. Body: {}",
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
        "Whirlpool"
    }
}

// Define WhirlpoolPoolParser
pub struct WhirlpoolPoolParser;
pub const ORCA_WHIRLPOOL_PROGRAM_ID: &str = "whirLbmvGdJ8kT34DbDZpeMZQRAu8da5nq7WaRDRtyQ";

impl PoolParser for WhirlpoolPoolParser {
    fn parse_pool_data(address: Pubkey, data: &[u8]) -> Result<PoolInfo> {
        if data.len() < 340 {
            error!(
                "Pool parsing failed for {} - Insufficient data length: {}",
                address,
                data.len()
            );
            return Err(anyhow!("Data too short for Whirlpool pool: {}", address));
        }

        Ok(PoolInfo {
            address,
            name: format!(
                "WhirlpoolPool/{}",
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
            fee_numerator: 30,
            fee_denominator: 10000,
            last_update_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            dex_type: DexType::Whirlpool,
        })
    }

    fn get_program_id() -> Pubkey {
        Pubkey::from_str(ORCA_WHIRLPOOL_PROGRAM_ID).unwrap()
    }

    fn get_dex_type() -> DexType {
        DexType::Whirlpool
    }
}

impl WhirlpoolPoolParser {
    pub fn parse_pool_data(address: Pubkey, data: &[u8]) -> anyhow::Result<PoolInfo> {
        <Self as PoolParser>::parse_pool_data(address, data)
    }
    pub fn get_program_id() -> Pubkey {
        <Self as PoolParser>::get_program_id()
    }
}
