// Main module: re-exports and high-level orchestration (ArbitrageAnalyzer, etc.)

pub mod fee;
pub mod math;

// Re-export all public API types, traits, and functions
pub use fee::*;
pub use math::*;

use crate::config::settings::Config;
use crate::local_metrics::Metrics;
use crate::solana::rpc::SolanaRpcClient;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct ArbitrageAnalyzer {
    pub slippage_model: Box<dyn math::SlippageModel>,
    pub fee_manager: fee::FeeManager,
}

impl ArbitrageAnalyzer {
    pub fn new(_config: &Config, _metrics: Arc<Mutex<Metrics>>) -> Self {
        Self {
            slippage_model: Box::new(math::EnhancedSlippageModel::new()),
            fee_manager: fee::FeeManager::default(),
        }
    }

    pub fn new_with_rpc(
        _config: &Config,
        _metrics: Arc<Mutex<Metrics>>,
        rpc_client: Arc<SolanaRpcClient>,
    ) -> Self {
        Self {
            slippage_model: Box::new(math::EnhancedSlippageModel::new()),
            fee_manager: fee::FeeManager::new(rpc_client),
        }
    }

    pub async fn calculate_fee_breakdown(
        &self,
        pools: &[&crate::utils::PoolInfo],
        input_amount: &crate::utils::TokenAmount,
        sol_price_usd: f64,
    ) -> Result<crate::arbitrage::analysis::FeeBreakdown> {
        self.fee_manager
            .calculate_multihop_fees(pools, input_amount, sol_price_usd)
            .await
    }

    // Backwards compatibility - deprecated
    #[deprecated(note = "Use calculate_fee_breakdown async version instead")]
    pub fn calculate_fee_breakdown_sync(
        &self,
        _pools: &[&crate::utils::PoolInfo],
        _input_amount: &crate::utils::TokenAmount,
        _sol_price_usd: f64,
    ) -> crate::arbitrage::analysis::FeeBreakdown {
        // Return a basic breakdown for backwards compatibility
        crate::arbitrage::analysis::FeeBreakdown {
            protocol_fee: 0.0,
            gas_fee: 5000.0, // Base Solana fee
            priority_fee: 1000.0,
            jito_tip: 0.0,
            slippage_cost: 0.0,
            total_cost: 6000.0,
            explanation: "Legacy sync calculation".to_string(),
            risk_score: 10.0,
            compute_units: 200_000,
            fee_per_signature: 5000.0,
            network_congestion: crate::arbitrage::analysis::NetworkCongestionLevel::Medium,
        }
    }
    // ...existing methods from analysis.rs for ArbitrageAnalyzer...
}
