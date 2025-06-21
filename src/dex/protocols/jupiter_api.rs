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

    /// Swap direction (e.g., "input" or "output")
    #[serde(rename = "swapDir")]
    pub swap_dir: String,

    /// Market price impact (in basis points)
    #[serde(rename = "marketPriceImpact")]
    pub market_price_impact: i64,

    /// Exchange rate (numerator/denominator)
    #[serde(rename = "exchangeRate")]
    pub exchange_rate: String,

    /// Amounts for the swap (input/output)
    #[serde(rename = "amounts")]
    pub amounts: Vec<String>,

    /// Additional info (e.g., fee tiers)
    #[serde(rename = "info")]
    pub info: Option<HashMap<String, String>>,
}

/// Platform fee structure in Jupiter response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformFee {
    /// Fee amount in native currency
    pub amount: String,

    /// Fee percentage
    #[serde(rename = "pct")]
    pub pct: String,
}
