use crate::dex::http_utils::HttpRateLimiter;
use crate::utils::{PoolInfo, PoolToken, DexType, PoolParser};
use crate::dex::quote::{DexClient, Quote as CanonicalQuote};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use log::error;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::env;
use std::str::FromStr; // Enables logging for debugging
use std::time::Duration;
use std::time::Instant;
use once_cell::sync::Lazy;

static LIFINITY_RATE_LIMITER: Lazy<HttpRateLimiter> = Lazy::new(|| {
    HttpRateLimiter::new(
        4,
        Duration::from_millis(200),
        3,
        Duration::from_millis(250),
        vec!["https://api.lifinity.io/quote".to_string()],
    )
});

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifinityApiQuote {
    pub input_token: String,
    pub output_token: String,
    pub input_amount: u64,
    pub output_amount: u64,
    pub dex: String,
}

pub struct LifinityClient {
    api_key: String,
    http_client: Client,
}

impl LifinityClient {
    pub fn new() -> Self {
        let api_key = env::var("LIFINITY_API_KEY").unwrap_or_else(|_| {
            error!("LIFINITY_API_KEY environment variable is not set. Using a dummy key.");
            "dummy_lifinity_api_key".to_string()
        });
        Self {
            api_key,
            http_client: Client::new(),
        }
    }

    pub fn build_request_with_api_key(&self, url: &str) -> reqwest::RequestBuilder {
        let mut req = self.http_client.get(url);
        if !self.api_key.is_empty() {
            req = req.header("api-key", &self.api_key);
        }
        req
    }

    pub fn get_api_key(&self) -> &str {
        &self.api_key
    }
}

#[async_trait]
impl DexClient for LifinityClient {
    async fn get_best_swap_quote(
        &self,
        input_token: &str,
        output_token: &str,
        amount: u64,
    ) -> anyhow::Result<CanonicalQuote> {
        let url = format!(
            "https://api.lifinity.io/quote?input={}&output={}&amount={}",
            input_token, output_token, amount
        );
        let request_start_time = Instant::now();
        let response = LIFINITY_RATE_LIMITER
            .get_with_backoff(&self.http_client, &url, |u| self.build_request_with_api_key(u))
            .await?;
        let request_duration_ms = request_start_time.elapsed().as_millis() as u64;
        if response.status().is_success() {
            match response.json::<LifinityApiQuote>().await {
                Ok(lifinity_quote) => {
                    Ok(CanonicalQuote {
                        input_token: lifinity_quote.input_token,
                        output_token: lifinity_quote.output_token,
                        input_amount: lifinity_quote.input_amount,
                        output_amount: lifinity_quote.output_amount,
                        dex: self.get_name().to_string(),
                        route: vec![input_token.to_string(), output_token.to_string()],
                        latency_ms: Some(request_duration_ms),
                        execution_score: None,
                        route_path: Some(vec![input_token.to_string(), output_token.to_string()]),
                        slippage_estimate: None,
                    })
                }
                Err(e) => {
                    error!("Failed to deserialize Lifinity quote: {:?}", e);
                    Err(anyhow!("Failed to deserialize Lifinity quote: {}", e))
                }
            }
        } else {
            let status = response.status(); // Save status before consuming response
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "Could not retrieve error body".to_string());
            error!(
                "Failed to fetch Lifinity quote: Status {}, Body: {}",
                status, error_body
            );
            Err(anyhow!(
                "Failed to fetch Lifinity quote: Status {}, Body: {}",
                status,
                error_body
            ))
        }
    }

    fn get_supported_pairs(&self) -> Vec<(String, String)> {
        vec![]
    }

    fn get_name(&self) -> &str {
        "Lifinity"
    }
}

// Define LifinityPoolParser
pub struct LifinityPoolParser;
pub const LIFINITY_PROGRAM_ID: &str = "EewxydAPCCVuNEyrVN68PuSYdQ7wKn27V9GJEbpNcHcn"; // Lifinity v2 program ID

impl PoolParser for LifinityPoolParser {
    fn parse_pool_data(address: Pubkey, data: &[u8]) -> Result<PoolInfo> {
        if data.len() < 100 {
            error!(
                "Pool parsing failed for {} - Insufficient data length: {}",
                address,
                data.len()
            );
            return Err(anyhow!("Data too short for Lifinity pool: {}", address));
        }

        Ok(PoolInfo {
            address,
            name: format!(
                "LifinityPool/{}",
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
            dex_type: DexType::Lifinity,
        })
    }

    fn get_program_id() -> Pubkey {
        Pubkey::from_str(LIFINITY_PROGRAM_ID).unwrap()
    }

    fn get_dex_type() -> DexType {
        DexType::Lifinity
    }
}

impl LifinityPoolParser {
    pub fn parse_pool_data(address: Pubkey, data: &[u8]) -> anyhow::Result<PoolInfo> {
        <Self as PoolParser>::parse_pool_data(address, data)
    }
    pub fn get_program_id() -> Pubkey {
        <Self as PoolParser>::get_program_id()
    }
}
