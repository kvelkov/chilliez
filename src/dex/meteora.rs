use crate::dex::http_utils::HttpRateLimiter;
use crate::dex::quote::{DexClient, Quote};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use log::{error, warn};
use reqwest::Client;
use serde::Deserialize;
use std::env;
use std::time::Duration;
use std::time::Instant;
use once_cell::sync::Lazy;

// Define a struct that matches Meteora's API response for a quote
#[derive(Deserialize, Debug)]
struct MeteoraApiResponse {
    #[serde(rename = "inputMint")]
    input_mint: String,
    #[serde(rename = "outputMint")]
    output_mint: String,
    #[serde(rename = "inAmount")]
    in_amount: String,
    #[serde(rename = "outAmount")]
    out_amount: String,
    // Add other fields as needed based on Meteora's API
}

pub struct MeteoraClient {
    api_key: String,
    http_client: Client,
}

static METEORA_RATE_LIMITER: Lazy<HttpRateLimiter> = Lazy::new(|| {
    HttpRateLimiter::new(
        4,
        Duration::from_millis(200),
        3,
        Duration::from_millis(250),
        vec!["https://api.meteora.ag/v1/quote".to_string()],
    )
});

impl MeteoraClient {
    pub fn new() -> Self {
        let api_key = env::var("METEORA_API_KEY").unwrap_or_else(|_| {
            warn!("METEORA_API_KEY not set. This might be required for some API calls.");
            String::new()
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
}

#[async_trait]
impl DexClient for MeteoraClient {
    async fn get_best_swap_quote(
        &self,
        input_token: &str,
        output_token: &str,
        amount: u64,
    ) -> anyhow::Result<Quote> {
        let url = format!(
            "https://api.meteora.ag/v1/quote?inputMint={}&outputMint={}&amount={}",
            input_token, output_token, amount
        );
        let request_start_time = Instant::now();
        let response = METEORA_RATE_LIMITER
            .get_with_backoff(&self.http_client, &url, |u| self.build_request_with_api_key(u))
            .await?;
        let request_duration_ms = request_start_time.elapsed().as_millis() as u64;
        if response.status().is_success() {
            match response.json::<MeteoraApiResponse>().await {
                Ok(api_response) => {
                    let input_amount_u64 = api_response.in_amount.parse::<u64>().map_err(|e| {
                        anyhow!(
                            "Failed to parse Meteora in_amount '{}': {}",
                            api_response.in_amount,
                            e
                        )
                    })?;
                    let output_amount_u64 =
                        api_response.out_amount.parse::<u64>().map_err(|e| {
                            anyhow!(
                                "Failed to parse Meteora out_amount '{}': {}",
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
                    error!("Failed to deserialize Meteora quote from URL {}: {:?}. Check MeteoraApiResponse struct against actual API output.", url, e);
                    Err(anyhow!(
                        "Failed to deserialize Meteora quote from {}. Error: {}",
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
                "Failed to fetch Meteora quote from URL {}: Status {}. Body: {}",
                url, status, error_text
            );
            Err(anyhow!(
                "Failed to fetch Meteora quote from {}: {}. Body: {}",
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
        "Meteora"
    }
}
