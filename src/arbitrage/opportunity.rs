// src/arbitrage/opportunity.rs
//! Defines the core struct for representing multi-hop, cross-DEX arbitrage opportunities.

use crate::arbitrage::fee_manager::{FeeBreakdown, FeeManager}; // FeeManager might not be directly used here but good for context
use crate::utils::{DexType, PoolInfo, TokenAmount};
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

/// Represents a single hop in a multi-hop arbitrage route.
#[derive(Debug, Clone)]
pub struct ArbHop {
    pub dex: DexType,
    pub pool: Pubkey, // Address of the pool for this hop
    pub input_token: String,  // Symbol of the input token for this hop
    pub output_token: String, // Symbol of the output token for this hop
    pub input_amount: f64,    // Amount of input_token (human-readable format)
    pub expected_output: f64, // Amount of output_token expected (human-readable format)
}

/// Represents a full arbitrage opportunity, possibly spanning multiple DEXes and hops.
/// This struct now consolidates fields from the previous ArbitrageOpportunity.
#[derive(Debug, Clone)]
pub struct MultiHopArbOpportunity {
    pub id: String, // Unique identifier for the opportunity
    /// Ordered list of hops (DEX+pool+token transitions)
    pub hops: Vec<ArbHop>,
    /// Total profit in terms of the final output token (human-readable amount, after all hops, ideally net of estimated fees/slippage)
    pub total_profit: f64,
    /// Profit as a percentage of the initial input amount (human-readable)
    pub profit_pct: f64, // Example: 1.5 for 1.5%

    /// The initial input token symbol for the entire arbitrage cycle
    pub input_token: String,
    /// The final output token symbol for the entire arbitrage cycle (should match input_token for cyclic arbitrage)
    pub output_token: String,
    /// The amount of the initial input_token (human-readable format)
    pub input_amount: f64,
    /// The expected amount of the final output_token after all hops (human-readable format)
    pub expected_output: f64,

    /// The sequence of DEX types involved in the arbitrage path
    pub dex_path: Vec<DexType>,
    /// The sequence of pool public keys traversed
    pub pool_path: Vec<Pubkey>,

    // Optional: risk/score/metadata fields
    pub risk_score: Option<f64>,
    pub notes: Option<String>,

    // Fields for USD value estimation
    pub estimated_profit_usd: Option<f64>, // Estimated profit converted to USD
    pub input_amount_usd: Option<f64>,     // Initial input amount converted to USD
    pub output_amount_usd: Option<f64>,    // Final output amount converted to USD
    pub intermediate_tokens: Vec<String>, // Symbols of intermediate tokens in the path

    // Fields from the former simple ArbitrageOpportunity struct, mainly for 2-hop context
    // For multi-hop (more than 2 hops), these might represent the first and last significant pools or be less relevant.
    // The `hops` vector is the primary source of truth for the path.
    pub source_pool: Arc<PoolInfo>, // First pool in the chain
    pub target_pool: Arc<PoolInfo>, // Last pool in the chain (for a simple 2-hop cycle, this would be the second pool)

    // Mint addresses for the primary tokens involved in the cycle
    pub input_token_mint: Pubkey,    // Mint of the initial input_token
    pub output_token_mint: Pubkey,   // Mint of the final output_token
    // Mint of the token after the first hop, particularly relevant for 2-hop or first step of multi-hop.
    // For longer chains, `intermediate_tokens` symbols list might be more useful.
    pub intermediate_token_mint: Option<Pubkey>,
}

impl MultiHopArbOpportunity {
    /// Returns true if the opportunity's profit_pct meets or exceeds the minimum threshold.
    /// Note: min_profit_pct here is expected as a percentage, e.g., 0.5 for 0.5%.
    pub fn is_profitable(&self, min_profit_pct_threshold: f64) -> bool {
        self.profit_pct >= min_profit_pct_threshold
    }

    /// Logs details of each hop in the arbitrage opportunity.
    pub fn log_hop_details(&self) {
        for (i, hop) in self.hops.iter().enumerate() {
            log::info!(
                "[OPP ID: {}][HOP {}] DEX: {:?}, Pool: {}, Input: {} {:.6}, Output: {} {:.6}",
                self.id,
                i + 1,
                hop.dex,
                hop.pool,
                hop.input_token,
                hop.input_amount,
                hop.output_token,
                hop.expected_output
            );
        }
    }

    /// Logs a summary of the multi-hop arbitrage opportunity.
    pub fn log_summary(&self) {
        let path_str = self.dex_path.iter().map(|d| format!("{:?}", d)).collect::<Vec<String>>().join(" -> ");
        log::info!(
            "[ARB OPPORTUNITY ID: {}] Path: {} | Input: {:.6} {} ({}) -> Output: {:.6} {} ({}) | Profit: {:.6} {} ({:.4}%) | Est. USD Profit: {:?} | Pools: {:?} | Notes: {}",
            self.id,
            path_str,
            self.input_amount,
            self.input_token,
            self.input_token_mint,
            self.expected_output,
            self.output_token,
            self.output_token_mint,
            self.total_profit,
            self.output_token, // Profit is in terms of the output token
            self.profit_pct,
            self.estimated_profit_usd.map_or_else(|| "N/A".to_string(), |p| format!("{:.2}", p)),
            self.pool_path,
            self.notes.as_deref().unwrap_or("N/A")
        );
        if !self.hops.is_empty() {
            self.log_hop_details();
        }
    }
}

#[allow(dead_code)]
/// Example analysis function (remains for context, not directly called by detector)
pub fn analyze_arbitrage_opportunity(
    pools: &[&PoolInfo],
    amounts: &[TokenAmount],
    directions: &[bool],
    last_fee_data: &[(Option<u64>, Option<u64>, Option<u64>)],
) -> FeeBreakdown {
    let slippage_model = crate::arbitrage::fee_manager::XYKSlippageModel;
    FeeManager::estimate_multi_hop_with_model(
        pools,
        amounts,
        directions,
        last_fee_data,
        &slippage_model,
    )
}