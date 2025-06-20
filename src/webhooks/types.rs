// src/webhooks/types.rs
//! Types and structures for webhook handling

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;

/// QuickNode webhook payload structure (matches your QuickNode Function output)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QuickNodeWebhookPayload {
    pub data: Vec<QuickNodeBlockData>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QuickNodeBlockData {
    #[serde(rename = "arbitrageTransactions")]
    pub arbitrage_transactions: Vec<QuickNodeArbitrageTransaction>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QuickNodeArbitrageTransaction {
    pub signature: String,
    pub slot: Option<u64>,
    pub block_time: Option<i64>,
    #[serde(rename = "dexSwaps")]
    pub dex_swaps: Vec<QuickNodeDexSwap>,
    #[serde(rename = "tokenTransfers")]
    pub token_transfers: Vec<QuickNodeTokenTransfer>,
    #[serde(rename = "liquidityChanges")]
    pub liquidity_changes: Vec<QuickNodeLiquidityChange>,
    #[serde(rename = "arbitrageOpportunities")]
    pub opportunities: Vec<QuickNodeOpportunityType>,
    #[serde(rename = "addressFlags")]
    pub address_flags: QuickNodeAddressFlags,
    #[serde(rename = "priceImpact")]
    pub price_impact: f64,
    #[serde(rename = "isLargeTrade")]
    pub is_large_trade: bool,
    #[serde(rename = "estimatedValueUSD")]
    pub estimated_value_usd: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QuickNodeDexSwap {
    pub dex: String,
    #[serde(rename = "programId")]
    pub program_id: String,
    #[serde(rename = "type")]
    pub swap_type: String,
    #[serde(rename = "tokenIn")]
    pub token_in: String,
    #[serde(rename = "tokenOut")]
    pub token_out: String,
    #[serde(rename = "amountIn")]
    pub amount_in: f64,
    #[serde(rename = "amountOut")]
    pub amount_out: f64,
    pub slippage: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QuickNodeTokenTransfer {
    pub source: String,
    pub destination: String,
    pub amount: f64,
    pub mint: String,
    #[serde(rename = "isSignificant")]
    pub is_significant: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QuickNodeLiquidityChange {
    pub mint: String,
    pub owner: String,
    pub change: f64,
    #[serde(rename = "changeUSD")]
    pub change_usd: Option<f64>,
    #[serde(rename = "type")]
    pub change_type: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QuickNodeOpportunityType {
    #[serde(rename = "type")]
    pub opp_type: String,
    #[serde(default)]
    pub dexes: Vec<String>,
    #[serde(rename = "estimatedProfit")]
    pub estimated_profit: Option<f64>,
    #[serde(default)]
    pub confidence: String,
    #[serde(rename = "priceImpact")]
    pub price_impact: Option<f64>,
    #[serde(rename = "tradeValue")]
    pub trade_value: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QuickNodeAddressFlags {
    #[serde(rename = "isWatched")]
    pub is_watched: bool,
    #[serde(rename = "isMevBot")]
    pub is_mev_bot: bool,
    #[serde(rename = "isWhale")]
    pub is_whale: bool,
    #[serde(rename = "hasArbitrageToken")]
    pub has_arbitrage_token: bool,
}

/// Pool update event for downstream consumers (QuickNode-based)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolUpdateEvent {
    pub pool_address: Pubkey,
    pub program_id: Pubkey,
    pub signature: String,
    pub timestamp: u64,
    pub slot: u64,
    pub update_type: PoolUpdateType,
    pub token_transfers: Vec<PoolTokenTransfer>,
    pub account_changes: Vec<PoolAccountChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PoolUpdateType {
    Swap,
    AddLiquidity,
    RemoveLiquidity,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolTokenTransfer {
    pub from_user_account: String,
    pub to_user_account: String,
    pub amount: f64,
    pub mint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolAccountChange {
    pub account: String,
    pub native_balance_change: i64,
}
