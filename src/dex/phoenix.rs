use crate::arbitrage::headers_with_api_key;
use crate::dex::http_utils::HttpRateLimiter;
use crate::dex::quote::{DexClient, Quote};
use anyhow::anyhow;
use async_trait::async_trait;
use log::{error, warn};
use once_cell::sync::Lazy;
use reqwest::Client;
use serde::Deserialize;
use std::env;
use std::time::Duration;
use std::time::Instant;

// Define a struct that matches Phoenix's API response for a quote
#[derive(Deserialize, Debug)]
struct PhoenixApiResponse {
    // These field names MUST match the JSON from Phoenix's API
    #[serde(rename = "inputMint")]
    input_mint: String,
    #[serde(rename = "outputMint")]
    output_mint: String,
    #[serde(rename = "inAmount")]
    in_amount: String,
    #[serde(rename = "outAmount")]
    out_amount: String,
    // Add other fields as needed
}

pub const PHOENIX_PROGRAM_ID: &str = "Pnix1UuGq7bK6iF1kQyK4R6mF1QwZ5Q1QwZ5Q1QwZ5Q1";

pub struct PhoenixClient {
    api_key: String,
    http_client: Client,
}

static PHOENIX_RATE_LIMITER: Lazy<HttpRateLimiter> = Lazy::new(|| {
    HttpRateLimiter::new(
        4,
        Duration::from_millis(200),
        3,
        Duration::from_millis(250),
        vec!["https://api.phoenix.trade/v1/quote".to_string()],
    )
});

impl PhoenixClient {
    pub fn new() -> Self {
        let api_key = env::var("PHOENIX_API_KEY").unwrap_or_else(|_| {
            warn!("PHOENIX_API_KEY not set. This may affect API access.");
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
impl DexClient for PhoenixClient {
    async fn get_best_swap_quote(
        &self,
        input_token: &str,
        output_token: &str,
        amount: u64,
    ) -> anyhow::Result<Quote> {
        // Use the rate limiter's get_with_backoff to wrap the HTTP request
        let url = format!(
            "https://api.phoenix.trade/v1/quote?inputMint={}&outputMint={}&amountIn={}",
            input_token, output_token, amount
        );
        let request_start_time = Instant::now();
        let response = PHOENIX_RATE_LIMITER
            .get_with_backoff(&self.http_client, &url, |u| {
                self.http_client
                    .get(u)
                    .headers(headers_with_api_key("PHOENIX_API_KEY"))
            })
            .await?;
        let request_duration_ms = request_start_time.elapsed().as_millis() as u64;
        if response.status().is_success() {
            match response.json::<PhoenixApiResponse>().await {
                Ok(api_response) => {
                    let input_amount_u64 = api_response.in_amount.parse::<u64>().map_err(|e| {
                        anyhow!(
                            "Failed to parse Phoenix in_amount '{}': {}",
                            api_response.in_amount,
                            e
                        )
                    })?;
                    let output_amount_u64 =
                        api_response.out_amount.parse::<u64>().map_err(|e| {
                            anyhow!(
                                "Failed to parse Phoenix out_amount '{}': {}",
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
                        "Failed to deserialize Phoenix quote from URL {}: {:?}. Check PhoenixApiResponse struct against actual API output.",
                        url, e
                    );
                    Err(anyhow!(
                        "Failed to deserialize Phoenix quote from {}. Error: {}",
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
                "Failed to fetch Phoenix quote from URL {}: Status {}. Body: {}",
                url, status, error_text
            );
            Err(anyhow!(
                "Failed to fetch Phoenix quote from {}: {}. Body: {}",
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
        "Phoenix"
    }
}
