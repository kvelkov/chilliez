// Main module: re-exports and high-level orchestration (ArbitrageAnalyzer, etc.)

pub mod fee;
pub mod math;

// Re-export all public API types, traits, and functions
pub use fee::*;
pub use math::*;

use crate::config::settings::Config;
use crate::metrics::Metrics;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct ArbitrageAnalyzer {
    pub advanced_math: math::AdvancedArbitrageMath,
    pub fee_manager: fee::FeeManager,
    pub threshold_updater: math::DynamicThresholdUpdater,
    #[allow(dead_code)]
    pub slippage_model: Box<dyn math::SlippageModel>,
}

impl ArbitrageAnalyzer {
    pub fn new(config: &Config, metrics: Arc<Mutex<Metrics>>) -> Self {
        Self {
            advanced_math: math::AdvancedArbitrageMath::new(12),
            fee_manager: fee::FeeManager::default(),
            threshold_updater: math::DynamicThresholdUpdater::new(config, metrics),
            slippage_model: Box::new(math::XYKSlippageModel::default()),
        }
    }
    pub fn calculate_fee_breakdown(
        &self,
        pools: &[&crate::utils::PoolInfo],
        input_amount: &crate::utils::TokenAmount,
        sol_price_usd: f64,
    ) -> crate::arbitrage::analysis::FeeBreakdown {
        self.fee_manager.calculate_multihop_fees(pools, input_amount, sol_price_usd)
    }
    // ...existing methods from analysis.rs for ArbitrageAnalyzer...
}

