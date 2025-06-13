// src/dex/opportunity.rs

use crate::utils::DexType;
use solana_sdk::pubkey::Pubkey;
use serde::{Deserialize, Serialize};

/// Represents a single hop (swap) in an arbitrage path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HopInfo {
    pub pool_address: Pubkey,
    pub dex_type: DexType,
    pub input_mint: Pubkey,
    pub output_mint: Pubkey,
    pub input_amount: u64,
    pub output_amount: u64,
    // Potentially add estimated fee for this hop if available
}

/// Represents a multi-hop arbitrage opportunity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiHopArbOpportunity {
    /// A unique identifier for this opportunity (e.g., hash of hops and timestamp).
    pub id: String,
    /// The sequence of swaps (hops) that make up the arbitrage.
    pub hops: Vec<HopInfo>,
    /// The initial token mint for the arbitrage cycle.
    pub start_token_mint: Pubkey,
    /// The final token mint (should be same as start_token_mint for an arbitrage).
    pub end_token_mint: Pubkey,
    /// The initial amount of `start_token_mint` used for the arbitrage.
    pub initial_input_amount: u64,
    /// The final amount of `end_token_mint` received after all hops.
    pub final_output_amount: u64,
    /// Estimated profit in basis points (e.g., 100 bips = 1%).
    pub profit_bps: u32,
    /// Optional: Estimated profit in USD (requires token price data).
    pub estimated_profit_usd: Option<f64>,
    /// Optional: A score indicating the risk or reliability of this opportunity.
    pub risk_score: Option<f32>,
}