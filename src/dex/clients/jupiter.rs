#![allow(dead_code)] // Jupiter integration in development, some structs not fully utilized

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use log::{debug, info, warn};
use once_cell::sync::Lazy;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use std::sync::atomic::AtomicUsize;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

use crate::{
    arbitrage::jupiter::{CacheConfig, CacheKey, CacheMetrics, JupiterQuoteCache},
    dex::{CommonSwapInfo, DexClient, DexHealthStatus, PoolDiscoverable, Quote, SwapInfo},
    error::ArbError,
    utils::PoolInfo,
};

// Import new Jupiter API structures
use super::jupiter_api::{
    CircuitBreakerConfig, CircuitBreakerState, QuoteRequest, QuoteResponse, RateLimitInfo,
    SwapRequest, SwapResponse,
};

static INFLIGHT_REQUESTS: Lazy<AtomicUsize> = Lazy::new(|| AtomicUsize::new(0));

/// Jupiter API v6 endpoints
const JUPITER_API_BASE: &str = "https://quote-api.jup.ag/v6";
const JUPITER_QUOTE_ENDPOINT: &str = "quote";
const JUPITER_SWAP_ENDPOINT: &str = "swap";
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
    pub input_mint: String,
    #[serde(rename = "inAmount")]
    pub in_amount: String,
    #[serde(rename = "outputMint")]
    pub output_mint: String,
    #[serde(rename = "outAmount")]
    pub out_amount: String,
    #[serde(rename = "otherAmountThreshold")]
    pub other_amount_threshold: String,
    #[serde(rename = "swapMode")]
    pub swap_mode: String,
    #[serde(rename = "slippageBps")]
    pub slippage_bps: u16,
    #[serde(rename = "platformFee")]
    pub platform_fee: Option<JupiterPlatformFee>,
    #[serde(rename = "priceImpactPct")]
    pub price_impact_pct: String,
    #[serde(rename = "routePlan")]
    pub route_plan: Vec<JupiterRoutePlan>,
    #[serde(rename = "contextSlot")]
    pub context_slot: Option<u64>,
    #[serde(rename = "timeTaken")]
    pub time_taken: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JupiterPlatformFee {
    amount: String,
    #[serde(rename = "feeBps")]
    fee_bps: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JupiterRoutePlan {
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

#[derive(Debug)]
struct CircuitBreaker {
    failure_count: u32,
    last_failure: Option<Instant>,
    state: CircuitState,
}

#[derive(Debug, PartialEq, Eq)]
enum CircuitState {
    Closed,
    Open(Instant), // time when opened
    HalfOpen,
}

impl CircuitBreaker {
    pub fn new() -> Self {
        CircuitBreaker {
            failure_count: 0,
            last_failure: None,
            state: CircuitState::Closed,
        }
    }
}

/// Jupiter aggregator client for price comparisons and routing
pub struct JupiterClient {
    client: Client,
    rate_limiter: Arc<tokio::sync::Mutex<RateLimiter>>,
    supported_tokens: Arc<tokio::sync::RwLock<HashMap<String, JupiterToken>>>,
    last_token_refresh: Arc<tokio::sync::RwLock<Instant>>,
    circuit_breaker: Arc<Mutex<CircuitBreaker>>,
    // Enhanced fallback-specific fields
    fallback_rate_limit: Arc<Mutex<RateLimitInfo>>,
    fallback_circuit_breaker: Arc<Mutex<CircuitBreakerState>>,
    fallback_config: CircuitBreakerConfig,
    // Cache integration
    quote_cache: Option<JupiterQuoteCache>,
    cache_config: CacheConfig,
}

impl JupiterClient {
    /// Create a new Jupiter client with enhanced fallback capabilities
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_millis(JUPITER_REQUEST_TIMEOUT_MS))
            .user_agent("SolanaArbBot/1.0")
            .build()
            .expect("Failed to create HTTP client");

        let cache_config = CacheConfig::default();
        let quote_cache = if cache_config.enabled {
            Some(JupiterQuoteCache::new(cache_config.clone()))
        } else {
            None
        };

        Self {
            client,
            rate_limiter: Arc::new(tokio::sync::Mutex::new(RateLimiter::new(
                JUPITER_REQUESTS_PER_SECOND,
            ))),
            supported_tokens: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            last_token_refresh: Arc::new(tokio::sync::RwLock::new(
                Instant::now() - Duration::from_secs(3600),
            )),
            circuit_breaker: Arc::new(Mutex::new(CircuitBreaker::new())),
            // Enhanced fallback fields
            fallback_rate_limit: Arc::new(Mutex::new(RateLimitInfo::default())),
            fallback_circuit_breaker: Arc::new(Mutex::new(CircuitBreakerState::default())),
            fallback_config: CircuitBreakerConfig::default(),
            // Cache integration
            quote_cache,
            cache_config,
        }
    }

    /// Create a new Jupiter client with custom cache configuration
    pub fn new_with_cache_config(cache_config: CacheConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_millis(JUPITER_REQUEST_TIMEOUT_MS))
            .user_agent("SolanaArbBot/1.0")
            .build()
            .expect("Failed to create HTTP client");

        let quote_cache = if cache_config.enabled {
            Some(JupiterQuoteCache::new(cache_config.clone()))
        } else {
            None
        };

        Self {
            client,
            rate_limiter: Arc::new(tokio::sync::Mutex::new(RateLimiter::new(
                JUPITER_REQUESTS_PER_SECOND,
            ))),
            supported_tokens: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            last_token_refresh: Arc::new(tokio::sync::RwLock::new(
                Instant::now() - Duration::from_secs(3600),
            )),
            circuit_breaker: Arc::new(Mutex::new(CircuitBreaker::new())),
            // Enhanced fallback fields
            fallback_rate_limit: Arc::new(Mutex::new(RateLimitInfo::default())),
            fallback_circuit_breaker: Arc::new(Mutex::new(CircuitBreakerState::default())),
            fallback_config: CircuitBreakerConfig::default(),
            // Cache integration
            quote_cache,
            cache_config,
        }
    }

    /// Create a new Jupiter client from application config
    pub fn from_config(config: &crate::config::settings::Config) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_millis(config.jupiter_api_timeout_ms))
            .user_agent("SolanaArbBot/1.0")
            .build()
            .expect("Failed to create HTTP client");

        let cache_config = CacheConfig {
            enabled: config.jupiter_cache_enabled,
            ttl_seconds: config.jupiter_cache_ttl_seconds,
            max_entries: config.jupiter_cache_max_entries,
            amount_bucket_size: config.jupiter_cache_amount_bucket_size,
            volatility_threshold_pct: config.jupiter_cache_volatility_threshold_pct,
            target_hit_rate: 0.7,
        };

        let quote_cache = if cache_config.enabled {
            Some(JupiterQuoteCache::new(cache_config.clone()))
        } else {
            None
        };

        info!(
            "🪐 Jupiter client initialized with cache {}",
            if cache_config.enabled {
                "enabled"
            } else {
                "disabled"
            }
        );

        Self {
            client,
            rate_limiter: Arc::new(tokio::sync::Mutex::new(RateLimiter::new(
                JUPITER_REQUESTS_PER_SECOND,
            ))),
            supported_tokens: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            last_token_refresh: Arc::new(tokio::sync::RwLock::new(
                Instant::now() - Duration::from_secs(3600),
            )),
            circuit_breaker: Arc::new(Mutex::new(CircuitBreaker::new())),
            // Enhanced fallback fields
            fallback_rate_limit: Arc::new(Mutex::new(RateLimitInfo::default())),
            fallback_circuit_breaker: Arc::new(Mutex::new(CircuitBreakerState::default())),
            fallback_config: CircuitBreakerConfig::default(),
            // Cache integration
            quote_cache,
            cache_config,
        }
    }

    /// Get quote for fallback price aggregation - main fallback method
    pub async fn get_quote_with_fallback(
        &self,
        input_mint: &str,
        output_mint: &str,
        amount: u64,
        slippage_bps: u16,
    ) -> Result<QuoteResponse> {
        info!(
            "🔄 Jupiter fallback: Getting quote for {} -> {} (amount: {})",
            input_mint, output_mint, amount
        );

        // Check cache first if enabled
        if let Some(ref cache) = self.quote_cache {
            let cache_key = CacheKey::from_params(
                input_mint,
                output_mint,
                amount,
                slippage_bps,
                self.cache_config.amount_bucket_size,
            );

            if let Some(cached_quote) = cache.get_quote(&cache_key).await {
                debug!("✅ Jupiter cache hit for key: {}", cache_key.to_string());
                return Ok(cached_quote);
            }

            debug!("❌ Jupiter cache miss for key: {}", cache_key.to_string());
        }

        // Check circuit breaker
        if !self.is_circuit_breaker_closed().await {
            return Err(anyhow!("Jupiter API circuit breaker is open"));
        }

        // Check rate limiting
        if !self.check_rate_limit().await {
            return Err(anyhow!("Jupiter API rate limit exceeded"));
        }

        let request = QuoteRequest {
            input_mint: input_mint.to_string(),
            output_mint: output_mint.to_string(),
            amount,
            slippage_bps,
            only_direct_routes: Some(false), // Allow multi-hop for better prices
            exclude_dexes: None,             // Don't exclude any DEXs for fallback
            max_accounts: Some(64),          // Reasonable limit
        };

        let quote = self.execute_quote_request(request).await?;

        // Cache the response if caching is enabled
        if let Some(ref cache) = self.quote_cache {
            let cache_key = CacheKey::from_params(
                input_mint,
                output_mint,
                amount,
                slippage_bps,
                self.cache_config.amount_bucket_size,
            );
            cache.store_quote(cache_key, quote.clone()).await;
            debug!("📦 Jupiter quote cached for future use");
        }

        debug!(
            "✅ Jupiter fallback quote: {} {} -> {} {} (impact: {}%)",
            quote.in_amount, input_mint, quote.out_amount, output_mint, quote.price_impact_pct
        );

        Ok(quote)
    }

    /// Create swap transaction for Jupiter-based opportunity
    pub async fn create_swap_transaction(
        &self,
        quote: &QuoteResponse,
        user_public_key: &str,
        priority_fee_lamports: Option<u64>,
    ) -> Result<SwapResponse> {
        info!(
            "🔄 Jupiter fallback: Creating swap transaction for user {}",
            user_public_key
        );

        // Check circuit breaker
        if !self.is_circuit_breaker_closed().await {
            return Err(anyhow!("Jupiter API circuit breaker is open"));
        }

        // Check rate limiting
        if !self.check_rate_limit().await {
            return Err(anyhow!("Jupiter API rate limit exceeded"));
        }

        let request = SwapRequest {
            user_public_key: user_public_key.to_string(),
            quote_response: quote.clone(),
            user_token_accounts: None, // Let Jupiter auto-discover
            wrap_and_unwrap_sol: Some(true),
            use_shared_accounts: Some(true),
            fee_account: None,
            tracking_account: None,
            compute_unit_price_micro_lamports: None,
            prioritization_fee_lamports: priority_fee_lamports,
            as_legacy_transaction: Some(false),
            use_token_ledger: Some(false),
            destination_token_account: None,
        };

        let swap_response = self.execute_swap_request(request).await?;

        debug!("✅ Jupiter fallback swap transaction created");

        Ok(swap_response)
    }

    /// Execute quote request with comprehensive error handling
    async fn execute_quote_request(&self, request: QuoteRequest) -> Result<QuoteResponse> {
        let url = format!("{}/{}", JUPITER_API_BASE, JUPITER_QUOTE_ENDPOINT);

        debug!("[DEBUG] JupiterClient: Sent GET request to {} with params: {:?}", url, request);
        let response = self
            .client
            .get(&url)
            .query(&request)
            .send()
            .await
            .map_err(|e| {
                self.record_api_failure();
                anyhow!("Jupiter quote request failed: {}", e)
            })?;
        debug!("[DEBUG] JupiterClient: Response status: {}", response.status());

        if !response.status().is_success() {
            self.record_api_failure();
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();

            if status == 429 {
                self.handle_rate_limit_exceeded().await;
                return Err(anyhow!("Jupiter API rate limit exceeded: {}", error_text));
            }

            return Err(anyhow!(
                "Jupiter quote failed with status {}: {}",
                status,
                error_text
            ));
        }

        let quote: QuoteResponse = response.json().await.map_err(|e| {
            self.record_api_failure();
            anyhow!("Failed to parse Jupiter quote response: {}", e)
        })?;

        self.record_api_success().await;
        Ok(quote)
    }

    /// Execute swap request with comprehensive error handling
    async fn execute_swap_request(&self, request: SwapRequest) -> Result<SwapResponse> {
        let url = format!("{}/{}", JUPITER_API_BASE, JUPITER_SWAP_ENDPOINT);

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                self.record_api_failure();
                anyhow!("Jupiter swap request failed: {}", e)
            })?;

        if !response.status().is_success() {
            self.record_api_failure();
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();

            if status == 429 {
                self.handle_rate_limit_exceeded().await;
                return Err(anyhow!("Jupiter API rate limit exceeded: {}", error_text));
            }

            return Err(anyhow!(
                "Jupiter swap failed with status {}: {}",
                status,
                error_text
            ));
        }

        let swap_response: SwapResponse = response.json().await.map_err(|e| {
            self.record_api_failure();
            anyhow!("Failed to parse Jupiter swap response: {}", e)
        })?;

        self.record_api_success().await;
        Ok(swap_response)
    }

    /// Check if circuit breaker allows requests
    async fn is_circuit_breaker_closed(&self) -> bool {
        let circuit_breaker = self.fallback_circuit_breaker.lock().await;
        match *circuit_breaker {
            CircuitBreakerState::Closed => true,
            CircuitBreakerState::Open { opened_at, .. } => {
                // Check if recovery timeout has passed
                opened_at.elapsed().as_secs() >= self.fallback_config.recovery_timeout_seconds
            }
            CircuitBreakerState::HalfOpen => true, // Allow test requests
        }
    }

    /// Check rate limiting
    async fn check_rate_limit(&self) -> bool {
        let mut rate_limit = self.fallback_rate_limit.lock().await;

        // Reset window if needed
        if rate_limit.window_start.elapsed().as_secs() >= rate_limit.reset_time_seconds {
            rate_limit.remaining = rate_limit.limit;
            rate_limit.window_start = Instant::now();
        }

        if rate_limit.remaining > 0 {
            rate_limit.remaining -= 1;
            true
        } else {
            false
        }
    }

    /// Handle rate limit exceeded
    async fn handle_rate_limit_exceeded(&self) {
        warn!("⚠️ Jupiter API rate limit exceeded, implementing backoff");

        let mut rate_limit = self.fallback_rate_limit.lock().await;
        rate_limit.remaining = 0;

        // Implement exponential backoff
        let backoff_duration = Duration::from_secs(2);
        drop(rate_limit);

        tokio::time::sleep(backoff_duration).await;
    }

    /// Record API success for circuit breaker
    async fn record_api_success(&self) {
        let mut circuit_breaker = self.fallback_circuit_breaker.lock().await;
        if let CircuitBreakerState::HalfOpen = *circuit_breaker {
            // Transition to closed after successful test
            *circuit_breaker = CircuitBreakerState::Closed;
            debug!("🟢 Jupiter circuit breaker closed after successful test");
        }
        // Already closed or success in normal operation: do nothing
    }

    /// Record API failure for circuit breaker
    fn record_api_failure(&self) {
        tokio::spawn({
            let circuit_breaker = self.fallback_circuit_breaker.clone();
            let config = self.fallback_config.clone();
            async move {
                let mut cb = circuit_breaker.lock().await;
                match &mut *cb {
                    CircuitBreakerState::Closed => {
                        // Open circuit after threshold failures
                        *cb = CircuitBreakerState::Open {
                            opened_at: Instant::now(),
                            failure_count: 1,
                        };
                        warn!("🔴 Jupiter circuit breaker opened due to API failure");
                    }
                    CircuitBreakerState::Open { failure_count, .. } => {
                        *failure_count += 1;
                    }
                    CircuitBreakerState::HalfOpen => {
                        // Failed test, go back to open
                        *cb = CircuitBreakerState::Open {
                            opened_at: Instant::now(),
                            failure_count: config.failure_threshold,
                        };
                        warn!("🔴 Jupiter circuit breaker reopened after failed test");
                    }
                }
            }
        });
    }

    /// Helper for GET requests with retry and exponential backoff
    async fn get_with_retry<T: for<'de> Deserialize<'de>>(
        client: &Client,
        url: &str,
        query: &impl Serialize,
        retries: u32,
        backoff: Duration,
    ) -> Result<T> {
        let mut attempt = 0;

        loop {
            let response = client
                .get(url)
                .query(query)
                .send()
                .await
                .map_err(|e| anyhow!("Request failed: {}", e))?;

            if response.status().is_success() {
                let result = response
                    .json::<T>()
                    .await
                    .map_err(|e| anyhow!("Failed to parse response: {}", e))?;
                return Ok(result);
            } else {
                attempt += 1;
                if attempt > retries {
                    return Err(anyhow!("Request failed after {} attempts", retries));
                }
                let delay = backoff * attempt;
                tokio::time::sleep(delay).await;
            }
        }
    }

    /// Enable caching for this Jupiter client
    pub fn enable_cache(&mut self, cache_config: CacheConfig) {
        info!("🔧 Enabling Jupiter quote cache");
        self.cache_config = cache_config.clone();
        self.quote_cache = if cache_config.enabled {
            Some(JupiterQuoteCache::new(cache_config))
        } else {
            None
        };
    }

    /// Disable caching for this Jupiter client
    pub fn disable_cache(&mut self) {
        info!("🔧 Disabling Jupiter quote cache");
        self.quote_cache = None;
        self.cache_config.enabled = false;
    }

    /// Get cache statistics if caching is enabled
    pub async fn get_cache_stats(&self) -> Option<CacheMetrics> {
        if let Some(ref cache) = self.quote_cache {
            Some(cache.get_metrics().await)
        } else {
            None
        }
    }

    /// Clear the cache manually
    pub async fn clear_cache(&self) {
        if let Some(ref cache) = self.quote_cache {
            cache.clear_volatile_cache("manual clear").await;
            info!("🧹 Jupiter quote cache cleared");
        }
    }

    /// Check if caching is enabled
    pub fn is_cache_enabled(&self) -> bool {
        self.quote_cache.is_some() && self.cache_config.enabled
    }

    /// Force cache invalidation for high volatility periods
    pub async fn invalidate_cache_for_volatility(&self) {
        if let Some(ref cache) = self.quote_cache {
            if cache.should_invalidate_for_volatility().await {
                cache.clear_volatile_cache("high volatility").await;
                info!("⚡ Jupiter cache invalidated due to high volatility");
            }
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
        Err(anyhow!(
            "Jupiter is an aggregator - use get_quote method instead"
        ))
    }

    fn get_swap_instruction(&self, _swap_info: &SwapInfo) -> Result<Instruction> {
        // For now, return an error as this requires async Jupiter API calls
        // Use get_swap_instruction_enhanced for full implementation
        Err(anyhow!(
            "Use get_swap_instruction_enhanced for Jupiter integration"
        ))
    }

    async fn get_swap_instruction_enhanced(
        &self,
        swap_info: &CommonSwapInfo,
        _pool_info: Arc<PoolInfo>,
    ) -> Result<Instruction, ArbError> {
        // Get Jupiter quote first using fallback method
        let quote = self
            .get_quote_with_fallback(
                &swap_info.source_token_mint.to_string(),
                &swap_info.destination_token_mint.to_string(),
                swap_info.input_amount,
                100, // Default 1% slippage in basis points
            )
            .await
            .map_err(|e| ArbError::NetworkError(format!("Jupiter quote failed: {}", e)))?;

        // Get swap transaction from Jupiter API using new method
        let swap_response = self
            .create_swap_transaction(
                &quote,
                &swap_info.user_wallet_pubkey.to_string(),
                None, // Default priority fee
            )
            .await
            .map_err(|e| ArbError::InstructionError(format!("Jupiter swap failed: {}", e)))?;

        // Decode and return the instruction
        self.decode_swap_instruction(&swap_response.swap_transaction)
            .map_err(|e| {
                ArbError::InstructionError(format!("Failed to decode Jupiter instruction: {}", e))
            })
    }

    async fn discover_pools(&self) -> Result<Vec<PoolInfo>> {
        // Jupiter is an aggregator, so it doesn't have its own pools
        // Instead, we return information about supported token pairs

        // TODO: self.refresh_token_list().await?;

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
        match self
            .get_quote_with_fallback(SOL_MINT, USDC_MINT, 1_000_000, 100)
            .await
        {
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

        let response = self
            .client
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
        use base64::{engine::general_purpose, Engine as _};
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
                accounts: instruction
                    .accounts
                    .iter()
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

        // TODO: self.refresh_token_list().await?;

        let tokens = self.supported_tokens.read().await;
        info!("🔍 Jupiter supports {} tokens for routing", tokens.len());

        // Return empty vector as Jupiter doesn't have discoverable pools in the traditional sense
        Ok(Vec::new())
    }

    async fn fetch_pool_data(&self, _pool_address: Pubkey) -> Result<PoolInfo> {
        // Jupiter doesn't have individual pools to fetch
        Err(anyhow!(
            "Jupiter is an aggregator and doesn't have individual pools"
        ))
    }

    fn dex_name(&self) -> &str {
        "Jupiter"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
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
        let _client = JupiterClient::new();

        // Test with well-known tokens (these should be supported)
        let _sol_mint = "So11111111111111111111111111111111111111112";
        let _usdc_mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

        // Note: This test requires internet connection
        // In a real test environment, you might want to mock the HTTP client
        // TODO: Implement token_supported method
        // let _sol_supported = client.is_token_supported(sol_mint).await;
        // let _usdc_supported = client.is_token_supported(usdc_mint).await;
    }
}
