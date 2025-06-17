//! Jupiter API V6 Data Structures
//!
//! This module defines the request and response structures for Jupiter V6 API endpoints.
//! Used for fallback price aggregation when primary DEX pathfinding fails.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Request structure for Jupiter V6 /quote endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteRequest {
    /// Input token mint address
    #[serde(rename = "inputMint")]
    pub input_mint: String,

    /// Output token mint address
    #[serde(rename = "outputMint")]
    pub output_mint: String,

    /// Amount of input token (in smallest unit)
    pub amount: u64,

    /// Slippage tolerance in basis points (e.g., 100 = 1%)
    #[serde(rename = "slippageBps")]
    pub slippage_bps: u16,

    /// Only use direct routes (optional)
    #[serde(rename = "onlyDirectRoutes", skip_serializing_if = "Option::is_none")]
    pub only_direct_routes: Option<bool>,

    /// Exclude specific DEXs (optional)
    #[serde(rename = "excludeDexes", skip_serializing_if = "Option::is_none")]
    pub exclude_dexes: Option<Vec<String>>,

    /// Maximum number of accounts (optional)
    #[serde(rename = "maxAccounts", skip_serializing_if = "Option::is_none")]
    pub max_accounts: Option<u16>,
}

/// Response structure for Jupiter V6 /quote endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteResponse {
    /// Input token mint
    #[serde(rename = "inputMint")]
    pub input_mint: String,

    /// Input amount
    #[serde(rename = "inAmount")]
    pub in_amount: String,

    /// Output token mint
    #[serde(rename = "outputMint")]
    pub output_mint: String,

    /// Output amount (estimated)
    #[serde(rename = "outAmount")]
    pub out_amount: String,

    /// Other output amounts for different slippage levels
    #[serde(rename = "otherAmountThreshold")]
    pub other_amount_threshold: String,

    /// Route plan with swap information
    #[serde(rename = "routePlan")]
    pub route_plan: Vec<RoutePlan>,

    /// Context slot when quote was generated
    #[serde(rename = "contextSlot")]
    pub context_slot: u64,

    /// Time taken to generate quote (ms)
    #[serde(rename = "timeTaken")]
    pub time_taken: f64,

    /// Platform fees (optional)
    #[serde(rename = "platformFee", skip_serializing_if = "Option::is_none")]
    pub platform_fee: Option<PlatformFee>,

    /// Price impact percentage
    #[serde(rename = "priceImpactPct")]
    pub price_impact_pct: String,
}

/// Route plan step in Jupiter quote
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutePlan {
    /// Swap information
    #[serde(rename = "swapInfo")]
    pub swap_info: SwapInfo,

    /// Percentage of amount for this route
    pub percent: u8,
}

/// Swap information within route plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapInfo {
    /// AMM key/identifier
    #[serde(rename = "ammKey")]
    pub amm_key: String,

    /// Label/name of the DEX
    pub label: String,

    /// Input mint for this swap
    #[serde(rename = "inputMint")]
    pub input_mint: String,

    /// Output mint for this swap
    #[serde(rename = "outputMint")]
    pub output_mint: String,

    /// Input amount for this swap
    #[serde(rename = "inAmount")]
    pub in_amount: String,

    /// Output amount for this swap
    #[serde(rename = "outAmount")]
    pub out_amount: String,

    /// Fee amount
    #[serde(rename = "feeAmount")]
    pub fee_amount: String,

    /// Fee mint
    #[serde(rename = "feeMint")]
    pub fee_mint: String,
}

/// Platform fee information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformFee {
    /// Fee amount
    pub amount: String,

    /// Fee percentage in basis points
    #[serde(rename = "feeBps")]
    pub fee_bps: u16,
}

/// Request structure for Jupiter V6 /swap endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapRequest {
    /// User's public key
    #[serde(rename = "userPublicKey")]
    pub user_public_key: String,

    /// Quote response from /quote endpoint
    #[serde(rename = "quoteResponse")]
    pub quote_response: QuoteResponse,

    /// User token accounts (optional)
    #[serde(rename = "userTokenAccounts", skip_serializing_if = "Option::is_none")]
    pub user_token_accounts: Option<Vec<UserTokenAccount>>,

    /// Wrap and unwrap SOL (default: true)
    #[serde(rename = "wrapAndUnwrapSol", skip_serializing_if = "Option::is_none")]
    pub wrap_and_unwrap_sol: Option<bool>,

    /// Use shared accounts (default: true)
    #[serde(rename = "useSharedAccounts", skip_serializing_if = "Option::is_none")]
    pub use_shared_accounts: Option<bool>,

    /// Fee account (optional)
    #[serde(rename = "feeAccount", skip_serializing_if = "Option::is_none")]
    pub fee_account: Option<String>,

    /// Tracking account (optional)
    #[serde(rename = "trackingAccount", skip_serializing_if = "Option::is_none")]
    pub tracking_account: Option<String>,

    /// Compute unit price (optional)
    #[serde(
        rename = "computeUnitPriceMicroLamports",
        skip_serializing_if = "Option::is_none"
    )]
    pub compute_unit_price_micro_lamports: Option<u64>,

    /// Priority fee (optional)
    #[serde(
        rename = "prioritizationFeeLamports",
        skip_serializing_if = "Option::is_none"
    )]
    pub prioritization_fee_lamports: Option<u64>,

    /// As legacy transaction (optional)
    #[serde(
        rename = "asLegacyTransaction",
        skip_serializing_if = "Option::is_none"
    )]
    pub as_legacy_transaction: Option<bool>,

    /// Use token ledger (optional)
    #[serde(rename = "useTokenLedger", skip_serializing_if = "Option::is_none")]
    pub use_token_ledger: Option<bool>,

    /// Destination token account (optional)
    #[serde(
        rename = "destinationTokenAccount",
        skip_serializing_if = "Option::is_none"
    )]
    pub destination_token_account: Option<String>,
}

/// User token account information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserTokenAccount {
    /// Token account address
    #[serde(rename = "tokenAccountAddress")]
    pub token_account_address: String,

    /// Token mint address
    #[serde(rename = "mint")]
    pub mint: String,

    /// Account balance
    pub balance: u64,
}

/// Response structure for Jupiter V6 /swap endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapResponse {
    /// Base64 encoded transaction
    #[serde(rename = "swapTransaction")]
    pub swap_transaction: String,

    /// Last valid block height
    #[serde(rename = "lastValidBlockHeight")]
    pub last_valid_block_height: u64,

    /// Priority fee estimate
    #[serde(rename = "prioritizationFeeLamports")]
    pub prioritization_fee_lamports: u64,

    /// Compute units consumed
    #[serde(rename = "computeUnitsConsumed")]
    pub compute_units_consumed: u64,
}

/// Jupiter API error response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JupiterErrorResponse {
    /// Error code
    pub error: String,

    /// Error message
    pub message: String,

    /// Additional error details (optional)
    pub details: Option<HashMap<String, serde_json::Value>>,
}

/// Rate limiting information
#[derive(Debug, Clone)]
pub struct RateLimitInfo {
    /// Requests remaining in current window
    pub remaining: u32,

    /// Total requests allowed per window
    pub limit: u32,

    /// Time until rate limit resets (seconds)
    pub reset_time_seconds: u64,

    /// Current window start time
    pub window_start: std::time::Instant,
}

impl Default for RateLimitInfo {
    fn default() -> Self {
        Self {
            remaining: 10, // Jupiter allows 10 req/sec
            limit: 10,
            reset_time_seconds: 1,
            window_start: std::time::Instant::now(),
        }
    }
}

/// Circuit breaker state for Jupiter API
#[derive(Debug, Clone)]
pub enum CircuitBreakerState {
    /// Circuit is closed (normal operation)
    Closed,

    /// Circuit is open (blocking requests due to failures)
    Open {
        /// When the circuit was opened
        opened_at: std::time::Instant,
        /// Number of consecutive failures
        failure_count: u32,
    },

    /// Circuit is half-open (testing if service is recovered)
    #[allow(dead_code)]
    HalfOpen,
}

impl Default for CircuitBreakerState {
    fn default() -> Self {
        Self::Closed
    }
}

/// Circuit breaker configuration
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening circuit
    pub failure_threshold: u32,

    /// Time to wait before testing service recovery
    pub recovery_timeout_seconds: u64,

    /// Success threshold to close circuit from half-open
    #[allow(dead_code)]
    pub success_threshold: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            recovery_timeout_seconds: 30,
            success_threshold: 2,
        }
    }
}
