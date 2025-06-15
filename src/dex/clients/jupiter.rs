#![allow(dead_code)] // Jupiter integration in development, some structs not fully utilized

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use log::{debug, info, warn};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{
    dex::{DexClient, PoolDiscoverable, Quote, SwapInfo, CommonSwapInfo, DexHealthStatus},
    utils::PoolInfo,
    error::ArbError,
};

/// Jupiter API v6 endpoints
const JUPITER_API_BASE: &str = "https://quote-api.jup.ag/v6";
const JUPITER_QUOTE_ENDPOINT: &str = "quote";
const JUPITER_PRICE_ENDPOINT: &str = "price";
const JUPITER_TOKENS_ENDPOINT: &str = "tokens";

/// Jupiter API rate limits (conservative)
const JUPITER_REQUESTS_PER_SECOND: u32 = 10;
const JUPITER_REQUEST_TIMEOUT_MS: u64 = 5000;

/// Jupiter quote request parameters
#[derive(Debug, Serialize)]
struct JupiterQuoteRequest {
    #[serde(rename = "inputMint")]
    input_mint: String,
    #[serde(rename = "outputMint")]
    output_mint: String,
    amount: u64,
    #[serde(rename = "slippageBps")]
    slippage_bps: u16,
    #[serde(rename = "onlyDirectRoutes")]
    only_direct_routes: Option<bool>,
    #[serde(rename = "asLegacyTransaction")]
    as_legacy_transaction: Option<bool>,
    #[serde(rename = "maxAccounts")]
    max_accounts: Option<u16>,
}

/// Jupiter quote response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JupiterQuoteResponse {
    #[serde(rename = "inputMint")]
    input_mint: String,
    #[serde(rename = "inAmount")]
    in_amount: String,
    #[serde(rename = "outputMint")]
    output_mint: String,
    #[serde(rename = "outAmount")]
    out_amount: String,
    #[serde(rename = "otherAmountThreshold")]
    other_amount_threshold: String,
    #[serde(rename = "swapMode")]
    swap_mode: String,
    #[serde(rename = "slippageBps")]
    slippage_bps: u16,
    #[serde(rename = "platformFee")]
    platform_fee: Option<JupiterPlatformFee>,
    #[serde(rename = "priceImpactPct")]
    price_impact_pct: String,
    #[serde(rename = "routePlan")]
    route_plan: Vec<JupiterRoutePlan>,
    #[serde(rename = "contextSlot")]
    context_slot: Option<u64>,
    #[serde(rename = "timeTaken")]
    time_taken: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JupiterPlatformFee {
    amount: String,
    #[serde(rename = "feeBps")]
    fee_bps: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JupiterRoutePlan {
    #[serde(rename = "swapInfo")]
    swap_info: JupiterSwapInfo,
    percent: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JupiterSwapInfo {
    #[serde(rename = "ammKey")]
    amm_key: String,
    label: String,
    #[serde(rename = "inputMint")]
    input_mint: String,
    #[serde(rename = "outputMint")]
    output_mint: String,
    #[serde(rename = "inAmount")]
    in_amount: String,
    #[serde(rename = "outAmount")]
    out_amount: String,
    #[serde(rename = "feeAmount")]
    fee_amount: String,
    #[serde(rename = "feeMint")]
    fee_mint: String,
}

/// Jupiter price response
#[derive(Debug, Deserialize)]
struct JupiterPriceResponse {
    data: HashMap<String, JupiterTokenPrice>,
    #[serde(rename = "timeTaken")]
    time_taken: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct JupiterTokenPrice {
    id: String,
    #[serde(rename = "mintSymbol")]
    mint_symbol: String,
    #[serde(rename = "vsToken")]
    vs_token: String,
    #[serde(rename = "vsTokenSymbol")]
    vs_token_symbol: String,
    price: f64,
}

/// Jupiter token list response
#[derive(Debug, Deserialize)]
struct JupiterTokensResponse {
    tokens: Vec<JupiterToken>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct JupiterToken {
    address: String,
    #[serde(rename = "chainId")]
    chain_id: u16,
    decimals: u8,
    name: String,
    symbol: String,
    #[serde(rename = "logoURI")]
    logo_uri: Option<String>,
    tags: Vec<String>,
}

/// Jupiter swap request
#[derive(Debug, Serialize)]
struct JupiterSwapRequest {
    #[serde(rename = "quoteResponse")]
    quote_response: JupiterQuoteResponse,
    #[serde(rename = "userPublicKey")]
    user_public_key: String,
    #[serde(rename = "wrapAndUnwrapSol")]
    wrap_and_unwrap_sol: bool,
    #[serde(rename = "useSharedAccounts")]
    use_shared_accounts: bool,
    #[serde(rename = "feeAccount")]
    fee_account: Option<String>,
    #[serde(rename = "trackingAccount")]
    tracking_account: Option<String>,
    #[serde(rename = "computeUnitPriceMicroLamports")]
    compute_unit_price_micro_lamports: Option<u64>,
    #[serde(rename = "prioritizationFeeLamports")]
    prioritization_fee_lamports: Option<u64>,
}

/// Jupiter swap response
#[derive(Debug, Deserialize)]
struct JupiterSwapResponse {
    #[serde(rename = "swapTransaction")]
    swap_transaction: String,
    #[serde(rename = "lastValidBlockHeight")]
    last_valid_block_height: Option<u64>,
    #[serde(rename = "prioritizationFeeLamports")]
    prioritization_fee_lamports: Option<u64>,
    #[serde(rename = "computeUnitLimit")]
    compute_unit_limit: Option<u32>,
    #[serde(rename = "dynamicSlippageReport")]
    dynamic_slippage_report: Option<serde_json::Value>,
    #[serde(rename = "simulationError")]
    simulation_error: Option<serde_json::Value>,
}

/// Rate limiter for Jupiter API calls
struct RateLimiter {
    last_request: Instant,
    min_interval: Duration,
}

impl RateLimiter {
    fn new(requests_per_second: u32) -> Self {
        Self {
            last_request: Instant::now() - Duration::from_secs(1),
            min_interval: Duration::from_millis(1000 / requests_per_second as u64),
        }
    }

    async fn wait_if_needed(&mut self) {
        let elapsed = self.last_request.elapsed();
        if elapsed < self.min_interval {
            let wait_time = self.min_interval - elapsed;
            tokio::time::sleep(wait_time).await;
        }
        self.last_request = Instant::now();
    }
}

/// Jupiter aggregator client for price comparisons and routing
pub struct JupiterClient {
    client: Client,
    rate_limiter: Arc<tokio::sync::Mutex<RateLimiter>>,
    supported_tokens: Arc<tokio::sync::RwLock<HashMap<String, JupiterToken>>>,
    last_token_refresh: Arc<tokio::sync::RwLock<Instant>>,
}

impl JupiterClient {
    /// Create a new Jupiter client
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_millis(JUPITER_REQUEST_TIMEOUT_MS))
            .user_agent("SolanaArbBot/1.0")
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            rate_limiter: Arc::new(tokio::sync::Mutex::new(RateLimiter::new(JUPITER_REQUESTS_PER_SECOND))),
            supported_tokens: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            last_token_refresh: Arc::new(tokio::sync::RwLock::new(Instant::now() - Duration::from_secs(3600))),
        }
    }

    /// Get a quote from Jupiter for a specific trade
    pub async fn get_quote(
        &self,
        input_mint: &str,
        output_mint: &str,
        amount: u64,
        slippage_bps: u16,
    ) -> Result<JupiterQuoteResponse> {
        // Rate limiting
        self.rate_limiter.lock().await.wait_if_needed().await;

        let request = JupiterQuoteRequest {
            input_mint: input_mint.to_string(),
            output_mint: output_mint.to_string(),
            amount,
            slippage_bps,
            only_direct_routes: Some(false), // Allow multi-hop routes
            as_legacy_transaction: Some(false),
            max_accounts: Some(64), // Reasonable limit for transaction size
        };

        let url = format!("{}/{}", JUPITER_API_BASE, JUPITER_QUOTE_ENDPOINT);
        
        debug!("🔍 Requesting Jupiter quote: {} {} -> {}", amount, input_mint, output_mint);

        let response = self.client
            .get(&url)
            .query(&request)
            .send()
            .await
            .map_err(|e| anyhow!("Jupiter API request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!("Jupiter API error {}: {}", status, text));
        }

        let quote: JupiterQuoteResponse = response.json().await
            .map_err(|e| anyhow!("Failed to parse Jupiter quote response: {}", e))?;

        debug!("✅ Jupiter quote received: {} -> {} (impact: {}%)", 
               quote.in_amount, quote.out_amount, quote.price_impact_pct);

        Ok(quote)
    }

    /// Get current token prices from Jupiter
    pub async fn get_prices(&self, token_mints: Vec<String>) -> Result<HashMap<String, f64>> {
        // Rate limiting
        self.rate_limiter.lock().await.wait_if_needed().await;

        let ids = token_mints.join(",");
        let url = format!("{}/{}", JUPITER_API_BASE, JUPITER_PRICE_ENDPOINT);
        
        debug!("📊 Requesting Jupiter prices for {} tokens", token_mints.len());

        let response = self.client
            .get(&url)
            .query(&[("ids", &ids), ("vsToken", &"USDC".to_string())])
            .send()
            .await
            .map_err(|e| anyhow!("Jupiter price API request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!("Jupiter price API error {}: {}", status, text));
        }

        let price_response: JupiterPriceResponse = response.json().await
            .map_err(|e| anyhow!("Failed to parse Jupiter price response: {}", e))?;

        let mut prices = HashMap::new();
        for (mint, price_data) in price_response.data {
            prices.insert(mint, price_data.price);
        }

        debug!("✅ Jupiter prices received for {} tokens", prices.len());
        Ok(prices)
    }

    /// Refresh the list of supported tokens
    pub async fn refresh_token_list(&self) -> Result<()> {
        // Check if we need to refresh (every hour)
        {
            let last_refresh = self.last_token_refresh.read().await;
            if last_refresh.elapsed() < Duration::from_secs(3600) {
                return Ok(());
            }
        }

        // Rate limiting
        self.rate_limiter.lock().await.wait_if_needed().await;

        let url = format!("{}/{}", JUPITER_API_BASE, JUPITER_TOKENS_ENDPOINT);
        
        debug!("🔄 Refreshing Jupiter token list...");

        let response = self.client
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("Jupiter tokens API request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!("Jupiter tokens API error {}: {}", status, text));
        }

        let tokens_response: JupiterTokensResponse = response.json().await
            .map_err(|e| anyhow!("Failed to parse Jupiter tokens response: {}", e))?;

        // Update supported tokens cache
        {
            let mut tokens = self.supported_tokens.write().await;
            tokens.clear();
            for token in tokens_response.tokens {
                tokens.insert(token.address.clone(), token);
            }
        }

        // Update refresh timestamp
        {
            let mut last_refresh = self.last_token_refresh.write().await;
            *last_refresh = Instant::now();
        }

        let token_count = {
            let tokens = self.supported_tokens.read().await;
            tokens.len()
        };

        info!("✅ Jupiter token list refreshed: {} tokens", token_count);
        Ok(())
    }

    /// Check if a token is supported by Jupiter
    pub async fn is_token_supported(&self, mint: &str) -> bool {
        // Refresh token list if needed
        let _ = self.refresh_token_list().await;

        let tokens = self.supported_tokens.read().await;
        tokens.contains_key(mint)
    }

    /// Get token info from Jupiter
    pub async fn get_token_info(&self, mint: &str) -> Option<JupiterToken> {
        // Refresh token list if needed
        let _ = self.refresh_token_list().await;

        let tokens = self.supported_tokens.read().await;
        tokens.get(mint).cloned()
    }

    /// Find best route using Jupiter's routing
    pub async fn find_best_route(
        &self,
        input_mint: &str,
        output_mint: &str,
        amount: u64,
        max_slippage_bps: u16,
    ) -> Result<Quote> {
        let quote = self.get_quote(input_mint, output_mint, amount, max_slippage_bps).await?;

        let input_amount: u64 = quote.in_amount.parse()
            .map_err(|e| anyhow!("Invalid input amount: {}", e))?;
        let output_amount: u64 = quote.out_amount.parse()
            .map_err(|e| anyhow!("Invalid output amount: {}", e))?;

        // Extract route information as Pubkeys (AMM keys from route plan)
        let route: Vec<Pubkey> = quote.route_plan
            .iter()
            .filter_map(|plan| plan.swap_info.amm_key.parse().ok())
            .collect();

        Ok(Quote {
            input_token: input_mint.to_string(),
            output_token: output_mint.to_string(),
            input_amount,
            output_amount,
            dex: "Jupiter".to_string(),
            route,
            slippage_estimate: Some(quote.slippage_bps as f64 / 100.0),
        })
    }

    /// Compare Jupiter route with direct DEX routes
    pub async fn compare_with_dex_routes(
        &self,
        input_mint: &str,
        output_mint: &str,
        amount: u64,
        dex_quotes: &[Quote],
    ) -> Result<Option<Quote>> {
        let jupiter_quote = match self.find_best_route(input_mint, output_mint, amount, 100).await {
            Ok(quote) => quote,
            Err(e) => {
                warn!("⚠️ Jupiter quote failed: {}", e);
                return Ok(None);
            }
        };

        // Compare with best DEX quote
        let best_dex_output = dex_quotes
            .iter()
            .map(|q| q.output_amount)
            .max()
            .unwrap_or(0);

        if jupiter_quote.output_amount > best_dex_output {
            info!("🎯 Jupiter route is better: {} vs {} (improvement: {:.2}%)",
                  jupiter_quote.output_amount, 
                  best_dex_output,
                  ((jupiter_quote.output_amount as f64 / best_dex_output as f64) - 1.0) * 100.0);
            Ok(Some(jupiter_quote))
        } else {
            debug!("📊 Direct DEX routes are better than Jupiter");
            Ok(None)
        }
    }
}

impl Default for JupiterClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DexClient for JupiterClient {
    fn get_name(&self) -> &str {
        "Jupiter"
    }

    fn calculate_onchain_quote(&self, _pool: &PoolInfo, _input_amount: u64) -> Result<Quote> {
        // Jupiter is an aggregator, not a single pool DEX
        // This method doesn't apply in the traditional sense
        Err(anyhow!("Jupiter is an aggregator - use get_quote method instead"))
    }

    fn get_swap_instruction(&self, _swap_info: &SwapInfo) -> Result<Instruction> {
        // For now, return an error as this requires async Jupiter API calls
        // Use get_swap_instruction_enhanced for full implementation
        Err(anyhow!("Use get_swap_instruction_enhanced for Jupiter integration"))
    }

    async fn get_swap_instruction_enhanced(
        &self,
        swap_info: &CommonSwapInfo,
        _pool_info: Arc<PoolInfo>,
    ) -> Result<Instruction, ArbError> {
        // Get Jupiter quote first
        let quote = self.get_quote(
            &swap_info.source_token_mint.to_string(),
            &swap_info.destination_token_mint.to_string(),
            swap_info.input_amount,
            100, // Default 1% slippage in basis points
        ).await
        .map_err(|e| ArbError::NetworkError(format!("Jupiter quote failed: {}", e)))?;

        // Get swap transaction from Jupiter API
        let swap_response = self.get_swap_transaction(
            &quote,
            &swap_info.user_wallet_pubkey.to_string(),
            0, // Default priority fee
        ).await
        .map_err(|e| ArbError::InstructionError(format!("Jupiter swap failed: {}", e)))?;

        // Decode and return the instruction
        self.decode_swap_instruction(&swap_response.swap_transaction)
            .map_err(|e| ArbError::InstructionError(format!("Failed to decode Jupiter instruction: {}", e)))
    }

    async fn discover_pools(&self) -> Result<Vec<PoolInfo>> {
        // Jupiter is an aggregator, so it doesn't have its own pools
        // Instead, we return information about supported token pairs
        
        self.refresh_token_list().await?;
        
        let tokens = self.supported_tokens.read().await;
        info!("🔍 Jupiter supports {} tokens for routing", tokens.len());
        
        // Return empty vector as Jupiter doesn't have discoverable pools in the traditional sense
        Ok(Vec::new())
    }

    async fn health_check(&self) -> Result<DexHealthStatus, ArbError> {
        // Simple health check by getting a small quote for SOL->USDC
        const SOL_MINT: &str = "So11111111111111111111111111111111111111112";
        const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
        
        let start_time = Instant::now();
        match self.get_quote(SOL_MINT, USDC_MINT, 1_000_000, 100).await {
            Ok(_) => {
                let response_time = start_time.elapsed().as_millis() as u64;
                debug!("✅ Jupiter health check passed");
                Ok(DexHealthStatus {
                    is_healthy: true,
                    last_successful_request: Some(Instant::now()),
                    error_count: 0,
                    response_time_ms: Some(response_time),
                    pool_count: None,
                    status_message: "Jupiter API responding normally".to_string(),
                })
            }
            Err(e) => {
                warn!("❌ Jupiter health check failed: {}", e);
                Ok(DexHealthStatus {
                    is_healthy: false,
                    last_successful_request: None,
                    error_count: 1,
                    response_time_ms: None,
                    pool_count: None,
                    status_message: format!("Jupiter API error: {}", e),
                })
            }
        }
    }
}

impl JupiterClient {
    /// Get swap transaction from Jupiter API
    async fn get_swap_transaction(
        &self,
        quote: &JupiterQuoteResponse,
        user_public_key: &str,
        priority_fee_lamports: u64,
    ) -> Result<JupiterSwapResponse> {
        let swap_request = JupiterSwapRequest {
            user_public_key: user_public_key.to_string(),
            quote_response: quote.clone(),
            wrap_and_unwrap_sol: true,
            use_shared_accounts: false,
            fee_account: None,
            tracking_account: None,
            compute_unit_price_micro_lamports: Some(priority_fee_lamports),
            prioritization_fee_lamports: Some(priority_fee_lamports),
        };

        let response = self.client
            .post(&format!("{}/swap", JUPITER_API_BASE))
            .json(&swap_request)
            .send()
            .await?;

        if response.status().is_success() {
            let swap_response: JupiterSwapResponse = response.json().await?;
            Ok(swap_response)
        } else {
            let error_text = response.text().await?;
            Err(anyhow!("Jupiter swap API error: {}", error_text))
        }
    }

    /// Decode swap instruction from Jupiter transaction
    fn decode_swap_instruction(&self, transaction_data: &str) -> Result<Instruction> {
        use base64::{Engine as _, engine::general_purpose};
        use solana_sdk::transaction::Transaction;

        // Decode base64 transaction
        let transaction_bytes = general_purpose::STANDARD
            .decode(transaction_data)
            .map_err(|e| anyhow!("Failed to decode base64 transaction: {}", e))?;

        // Deserialize transaction
        let transaction: Transaction = bincode::deserialize(&transaction_bytes)
            .map_err(|e| anyhow!("Failed to deserialize transaction: {}", e))?;

        // For Jupiter, typically the first instruction is the swap instruction
        if let Some(instruction) = transaction.message.instructions.first() {
            Ok(Instruction {
                program_id: transaction.message.account_keys[instruction.program_id_index as usize],
                accounts: instruction.accounts.iter()
                    .map(|&idx| solana_sdk::instruction::AccountMeta {
                        pubkey: transaction.message.account_keys[idx as usize],
                        is_signer: transaction.message.is_signer(idx as usize),
                        is_writable: transaction.message.is_writable(idx as usize),
                    })
                    .collect(),
                data: instruction.data.clone(),
            })
        } else {
            Err(anyhow!("No instructions found in Jupiter transaction"))
        }
    }
}

#[async_trait]
impl PoolDiscoverable for JupiterClient {
    async fn discover_pools(&self) -> Result<Vec<PoolInfo>> {
        // Jupiter is an aggregator, so it doesn't have its own pools
        // Instead, we return information about supported token pairs
        
        self.refresh_token_list().await?;
        
        let tokens = self.supported_tokens.read().await;
        info!("🔍 Jupiter supports {} tokens for routing", tokens.len());
        
        // Return empty vector as Jupiter doesn't have discoverable pools in the traditional sense
        Ok(Vec::new())
    }

    async fn fetch_pool_data(&self, _pool_address: Pubkey) -> Result<PoolInfo> {
        // Jupiter doesn't have individual pools to fetch
        Err(anyhow!("Jupiter is an aggregator and doesn't have individual pools"))
    }

    fn dex_name(&self) -> &str {
        "Jupiter"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jupiter_client_creation() {
        let client = JupiterClient::new();
        assert_eq!(client.get_name(), "Jupiter");
    }

    #[test]
    fn test_rate_limiter() {
        let limiter = RateLimiter::new(10); // 10 requests per second
        assert_eq!(limiter.min_interval, Duration::from_millis(100));
    }

    #[tokio::test]
    async fn test_token_support_check() {
        let client = JupiterClient::new();
        
        // Test with well-known tokens (these should be supported)
        let sol_mint = "So11111111111111111111111111111111111111112";
        let usdc_mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
        
        // Note: This test requires internet connection
        // In a real test environment, you might want to mock the HTTP client
        let _sol_supported = client.is_token_supported(sol_mint).await;
        let _usdc_supported = client.is_token_supported(usdc_mint).await;
    }
}
