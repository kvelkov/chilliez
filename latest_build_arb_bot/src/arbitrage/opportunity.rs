//! Defines the core struct for representing multi-hop, cross-DEX arbitrage opportunities.

use crate::arbitrage::fee_manager::{FeeBreakdown, FeeManager};
use crate::utils::{DexType, PoolInfo, TokenAmount};
use solana_sdk::pubkey::Pubkey;

/// Represents a single hop in a multi-hop arbitrage route.
#[derive(Debug, Clone)]
pub struct ArbHop {
    pub dex: DexType,
    pub pool: Pubkey,
    pub input_token: String,
    pub output_token: String,
    pub input_amount: f64,
    pub expected_output: f64,
}

/// Represents a full arbitrage opportunity, possibly spanning multiple DEXes and hops.
#[derive(Debug, Clone)]
pub struct MultiHopArbOpportunity {
    /// Ordered list of hops (DEX+pool+token transitions)
    pub hops: Vec<ArbHop>,
    /// Total profit in output token (after all hops, fees, slippage)
    pub total_profit: f64,
    /// Profit as a percentage of input
    pub profit_pct: f64,
    /// The initial input token
    pub input_token: String,
    /// The final output token (should match input for cyclic arb)
    pub output_token: String,
    /// The amount initially input
    pub input_amount: f64,
    /// The expected output after all hops
    pub expected_output: f64,
    /// The full DEX path (for analytics/logging)
    pub dex_path: Vec<DexType>,
    /// The pools traversed (for analytics/logging)
    pub pool_path: Vec<Pubkey>,
    /// Optional: risk/score/metadata fields
    pub risk_score: Option<f64>,
    pub notes: Option<String>,
}

impl MultiHopArbOpportunity {
    /// Returns true if the opportunity is profitable above a given threshold
    pub fn is_profitable(&self, min_profit_pct: f64) -> bool {
        self.profit_pct >= min_profit_pct
    }

    /// Logs all hops in the arbitrage opportunity for analytics/logging.
    pub fn log_hop(&self) {
        for (i, hop) in self.hops.iter().enumerate() {
            log::info!(
                "[HOP {}] DEX: {:?}, Pool: {}, InputToken: {}, OutputToken: {}, InputAmount: {:.6}, ExpectedOutput: {:.6}",
                i,
                hop.dex,
                hop.pool,
                hop.input_token,
                hop.output_token,
                hop.input_amount,
                hop.expected_output
            );
        }
    }

    /// Logs summary of the multi-hop arbitrage opportunity for analytics/logging.
    pub fn log_summary(&self) {
        log::info!(
            "[ARB OPPORTUNITY] InputToken: {}, OutputToken: {}, InputAmount: {:.6}, ExpectedOutput: {:.6}, DexPath: {:?}, PoolPath: {:?}, RiskScore: {:?}, Notes: {}",
            self.input_token,
            self.output_token,
            self.input_amount,
            self.expected_output,
            self.dex_path,
            self.pool_path,
            self.risk_score,
            self.notes.as_deref().unwrap_or("")
        );
    }
}

#[allow(dead_code)]
/// Example: Compute fee/slippage/gas for a multi-hop arbitrage opportunity
pub fn analyze_arbitrage_opportunity(
    pools: &[&PoolInfo],
    amounts: &[TokenAmount],
    directions: &[bool],
    last_fee_data: &[(Option<u64>, Option<u64>, Option<u64>)],
) -> FeeBreakdown {
    // Use the default XYK slippage model
    let slippage_model = crate::arbitrage::fee_manager::XYKSlippageModel;
    FeeManager::estimate_multi_hop_with_model(
        pools,
        amounts,
        directions,
        last_fee_data,
        &slippage_model,
    )
}
