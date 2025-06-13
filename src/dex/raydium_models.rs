// src/dex/raydium_models.rs
//! Data models for Raydium API responses

use serde::{Deserialize, Serialize};

/// Root structure for Raydium liquidity JSON response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidityFile {
    /// Official pools list
    pub official: Vec<AmmPool>,
    /// Unofficial pools list (optional)
    #[serde(default, rename = "unOfficial")]
    pub un_official: Vec<AmmPool>,
}

/// Raydium AMM pool information from API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmmPool {
    /// Pool ID (address)
    pub id: String,
    /// Base token mint
    #[serde(rename = "baseMint")]
    pub base_mint: String,
    /// Quote token mint
    #[serde(rename = "quoteMint")]
    pub quote_mint: String,
    /// LP mint
    #[serde(rename = "lpMint")]
    pub lp_mint: String,
    /// Base token decimals
    #[serde(rename = "baseDecimals")]
    pub base_decimals: Option<u8>,
    /// Quote token decimals
    #[serde(rename = "quoteDecimals")]
    pub quote_decimals: Option<u8>,
    /// LP token decimals
    #[serde(rename = "lpDecimals")]
    pub lp_decimals: Option<u8>,
    /// Version
    pub version: u8,
    /// Program ID
    #[serde(rename = "programId")]
    pub program_id: String,
    /// Authority
    pub authority: String,
    /// Open orders
    #[serde(rename = "openOrders")]
    pub open_orders: String,
    /// Target orders
    #[serde(rename = "targetOrders")]
    pub target_orders: String,
    /// Base vault
    #[serde(rename = "baseVault")]
    pub base_vault: String,
    /// Quote vault
    #[serde(rename = "quoteVault")]
    pub quote_vault: String,
    /// Withdraw queue
    #[serde(rename = "withdrawQueue")]
    pub withdraw_queue: String,
    /// LP vault
    #[serde(rename = "lpVault")]
    pub lp_vault: String,
    /// Market version
    #[serde(rename = "marketVersion")]
    pub market_version: u8,
    /// Market program ID
    #[serde(rename = "marketProgramId")]
    pub market_program_id: String,
    /// Market ID
    #[serde(rename = "marketId")]
    pub market_id: String,
    /// Market authority
    #[serde(rename = "marketAuthority")]
    pub market_authority: String,
    /// Market base vault
    #[serde(rename = "marketBaseVault")]
    pub market_base_vault: String,
    /// Market quote vault
    #[serde(rename = "marketQuoteVault")]
    pub market_quote_vault: String,
    /// Market bids
    #[serde(rename = "marketBids")]
    pub market_bids: String,
    /// Market asks
    #[serde(rename = "marketAsks")]
    pub market_asks: String,
    /// Market event queue
    #[serde(rename = "marketEventQueue")]
    pub market_event_queue: String,
    /// Base symbol
    #[serde(default)]
    pub base_symbol: Option<String>,
    /// Quote symbol
    #[serde(default)]
    pub quote_symbol: Option<String>,
}
