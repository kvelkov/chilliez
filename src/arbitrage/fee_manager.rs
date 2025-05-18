//! FeeManager: Centralized logic for all fee, slippage, and dynamic gas calculations across pools/DEXs.

use crate::arbitrage::calculator::OpportunityCalculationResult;
use crate::utils::{DexType, PoolInfo, TokenAmount};
use once_cell::sync::Lazy;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Represents a detailed breakdown of all fees/slippage/gas for a swap or arbitrage path.
#[derive(Debug, Clone)]
pub struct FeeBreakdown {
    pub expected_fee: f64,         // Summed token fee for the swap path
    pub expected_slippage: f64,    // Expected slippage (fraction, e.g., 0.002 = 0.2%)
    pub gas_cost: u64,             // Estimated compute/gas units or lamports
    pub sudden_fee_increase: bool, // True if fee spike/surge detected
    pub explanation: String,       // Human-readable summary for logs/alerts
}

/// Represents the result of fee estimation for an arbitrage opportunity
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FeeEstimationResult {
    pub swap_fee: f64,
    pub transaction_fee: u64,
    pub priority_fee: u64,
    pub total_cost: f64,
}

/// Estimate fees for an arbitrage opportunity
#[allow(dead_code)]
pub fn estimate_fees(
    _pair: &(Pubkey, Pubkey),
    calc_result: &OpportunityCalculationResult,
) -> FeeEstimationResult {
    // In a production system, this would:
    // 1. Look up the pool details from a global pool map or context
    // 2. Calculate the swap fee based on the DEX fee structure
    // 3. Estimate transaction fees based on current network conditions
    // 4. Add priority fee based on configuration and network congestion
    // 5. Calculate total cost considering all fees

    // Basic swap fee calculation - typically a percentage of the input amount
    let swap_fee = calc_result.input_amount * 0.003; // Assuming 0.3% fee

    // Transaction fee in lamports (0.000005 SOL = 5000 lamports)
    let transaction_fee: u64 = 5000;

    // Priority fee in compute units (200 is a typical minimum)
    let priority_fee: u64 = 200;

    // Total cost in terms of the input token
    let total_cost = swap_fee + (transaction_fee as f64 / 1_000_000.0); // Rough conversion to SOL

    FeeEstimationResult {
        swap_fee,
        transaction_fee,
        priority_fee,
        total_cost,
    }
}

/// Centralized manager for all dynamic fee/slippage/gas estimation.
/// Use this from arbitrage engine/detector before executing or quoting any trade.
pub struct FeeManager;

impl FeeManager {
    // --- Removed legacy APIs: estimate_pool_swap and estimate_multi_hop ---
    // Use estimate_pool_swap_with_model, estimate_pool_swap_integrated, and estimate_multi_hop_with_model instead.

    /// Determines if a fee is abnormally high compared to historical data
    pub fn is_fee_abnormal(
        current_fee_numerator: u64,
        current_fee_denominator: u64,
        historical_fee_numerator: u64,
        historical_fee_denominator: u64,
        threshold_multiplier: f64,
    ) -> bool {
        let current_fee = current_fee_numerator as f64 / current_fee_denominator as f64;
        let historical_fee = historical_fee_numerator as f64 / historical_fee_denominator as f64;

        current_fee > historical_fee * threshold_multiplier
    }

    /// Estimate pool swap with pluggable slippage model and config-driven gas cost
    pub fn estimate_pool_swap_with_model(
        pool: &PoolInfo,
        input_amount: &TokenAmount,
        is_a_to_b: bool,
        last_known_fee_numerator: Option<u64>,
        last_known_fee_denominator: Option<u64>,
        last_update_timestamp: Option<u64>,
        slippage_model: &dyn SlippageModel,
    ) -> FeeBreakdown {
        let fee_fraction = pool.fee_numerator as f64 / pool.fee_denominator as f64;
        let expected_fee = input_amount.to_float() * fee_fraction;
        let slippage = slippage_model.estimate_slippage(pool, input_amount, is_a_to_b);
        let gas_cost = get_gas_cost_for_dex(pool.dex_type);
        let sudden_fee_increase = if let (Some(last_num), Some(last_den)) =
            (last_known_fee_numerator, last_known_fee_denominator)
        {
            let last_fraction = last_num as f64 / last_den as f64;
            fee_fraction > (last_fraction * 1.5)
        } else {
            false
        };
        let pool_stale = if let Some(ts) = last_update_timestamp {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            now > ts + 10
        } else {
            false
        };
        let mut explanation = format!(
            "Fee: {:.5} ({} / {}), slippage est: {:.5}, gas: {}, sudden_fee_increase: {}",
            fee_fraction,
            pool.fee_numerator,
            pool.fee_denominator,
            slippage,
            gas_cost,
            sudden_fee_increase
        );
        if pool_stale {
            explanation.push_str(" WARNING: POOL STALE!");
        }
        FeeBreakdown {
            expected_fee,
            expected_slippage: slippage,
            gas_cost,
            sudden_fee_increase,
            explanation,
        }
    }

    /// Estimate pool swap using the default slippage model and config-driven gas cost
    pub fn estimate_pool_swap_integrated(
        pool: &PoolInfo,
        input_amount: &TokenAmount,
        is_a_to_b: bool,
        last_update_timestamp: Option<u64>,
    ) -> FeeBreakdown {
        // Use fee history tracker for last known fee
        let last_fee = FEE_HISTORY_TRACKER.get_last_fee(pool);
        let (last_num, last_den) = last_fee.unwrap_or((pool.fee_numerator, pool.fee_denominator));
        Self::estimate_pool_swap_with_model(
            pool,
            input_amount,
            is_a_to_b,
            Some(last_num),
            Some(last_den),
            last_update_timestamp,
            DEFAULT_SLIPPAGE_MODEL.as_ref(),
        )
    }

    /// Estimate multi-hop arbitrage with pluggable slippage model and config-driven gas cost
    pub fn estimate_multi_hop_with_model(
        pools: &[&PoolInfo],
        amounts: &[TokenAmount],
        directions: &[bool],
        last_fee_data: &[(Option<u64>, Option<u64>, Option<u64>)],
        slippage_model: &dyn SlippageModel,
    ) -> FeeBreakdown {
        let mut total_fee = 0.0;
        let mut total_slippage = 0.0;
        let mut total_gas = 0;
        let mut any_spike = false;
        let mut explanation_sections = Vec::new();
        for ((pool, amount), (direction, fee_data)) in pools
            .iter()
            .zip(amounts)
            .zip(directions.iter().zip(last_fee_data))
        {
            let breakdown = Self::estimate_pool_swap_with_model(
                pool,
                amount,
                *direction,
                fee_data.0,
                fee_data.1,
                fee_data.2,
                slippage_model,
            );
            total_fee += breakdown.expected_fee;
            total_slippage += breakdown.expected_slippage;
            total_gas += breakdown.gas_cost;
            if breakdown.sudden_fee_increase {
                any_spike = true;
            }
            explanation_sections.push(breakdown.explanation);
        }
        FeeBreakdown {
            expected_fee: total_fee,
            expected_slippage: total_slippage,
            gas_cost: total_gas,
            sudden_fee_increase: any_spike,
            explanation: explanation_sections.join(" | "),
        }
    }

    /// Convert a fee or slippage value to a reference token (e.g., USDC) using price info
    pub fn convert_fee_to_reference_token(
        amount: f64,
        from_symbol: &str,
        to_symbol: &str,
    ) -> Option<f64> {
        convert_to_reference_token(amount, from_symbol, to_symbol)
    }

    /// Record a fee observation for a pool (for anomaly detection)
    pub fn record_fee_observation(pool: &PoolInfo, fee_numerator: u64, fee_denominator: u64) {
        FEE_HISTORY_TRACKER.record_fee(pool, fee_numerator, fee_denominator);
    }

    /// Get the last known fee for a pool (for anomaly detection)
    pub fn get_last_fee_for_pool(pool: &PoolInfo) -> Option<(u64, u64)> {
        FEE_HISTORY_TRACKER.get_last_fee(pool)
    }
}

/// Trait for pluggable slippage models (xy=k, curve, custom)
pub trait SlippageModel: Send + Sync {
    fn estimate_slippage(
        &self,
        pool: &PoolInfo,
        input_amount: &TokenAmount,
        is_a_to_b: bool,
    ) -> f64;
}

/// Default xy=k slippage model
pub struct XYKSlippageModel;
impl SlippageModel for XYKSlippageModel {
    fn estimate_slippage(
        &self,
        pool: &PoolInfo,
        input_amount: &TokenAmount,
        is_a_to_b: bool,
    ) -> f64 {
        let input_reserve = if is_a_to_b {
            pool.token_a.reserve
        } else {
            pool.token_b.reserve
        } as f64;
        if input_reserve > 0.0 {
            input_amount.to_float() / (input_reserve + 1.0)
        } else {
            0.0
        }
    }
}

/// Config-driven gas cost lookup (can be replaced with empirical/config values)
pub fn get_gas_cost_for_dex(dex: DexType) -> u64 {
    match dex {
        DexType::Raydium | DexType::Orca => 5000, // TODO: load from config
        DexType::Whirlpool => 8000,
        _ => 6000,
    }
}

/// Convert a fee/slippage amount to a reference token (e.g., USDC) using price info
/// (Stub: requires price oracle integration)
pub fn convert_to_reference_token(
    _amount: f64,
    _from_symbol: &str,
    _to_symbol: &str,
) -> Option<f64> {
    // TODO: Integrate with price oracle or aggregator
    None
}

/// Stub for historical fee tracking (to be filled in by engine)
pub struct FeeHistoryTracker;
impl FeeHistoryTracker {
    pub fn record_fee(&self, _pool: &PoolInfo, _fee_numerator: u64, _fee_denominator: u64) {
        // TODO: Store fee history for pools
    }
    pub fn get_last_fee(&self, _pool: &PoolInfo) -> Option<(u64, u64)> {
        // TODO: Retrieve last known fee for pool
        None
    }
}

// Provide a static default slippage model and fee history tracker for engine-wide use
static DEFAULT_SLIPPAGE_MODEL: Lazy<Arc<XYKSlippageModel>> =
    Lazy::new(|| Arc::new(XYKSlippageModel));
static FEE_HISTORY_TRACKER: Lazy<Arc<FeeHistoryTracker>> =
    Lazy::new(|| Arc::new(FeeHistoryTracker));

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::{DexType, PoolInfo, PoolToken, TokenAmount};
    use solana_sdk::pubkey::Pubkey;

    #[test]
    fn test_estimate_pool_swap() {
        let pool = PoolInfo {
            address: Pubkey::new_unique(),
            name: "Raydium SOL-USDC".to_string(),
            token_a: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "SOL".to_string(),
                decimals: 9,
                reserve: 1_000_000_000_000,
            },
            token_b: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 2_000_000_000_000,
            },
            fee_numerator: 25,
            fee_denominator: 10000,
            last_update_timestamp: 1234567890,
            dex_type: DexType::Raydium,
        };
        let in_amt = TokenAmount::new(10_000_000, 9); // 0.01 SOL in base units
        let fee_data = (Some(25), Some(10000), Some(1234567890));
        // Use the advanced API for the test
        let breakdown = FeeManager::estimate_pool_swap_integrated(&pool, &in_amt, true, fee_data.2);
        assert!(breakdown.expected_fee > 0.0);
        assert!(breakdown.expected_slippage >= 0.0);
        assert!(!breakdown.sudden_fee_increase);
    }

    #[test]
    fn test_is_fee_abnormal() {
        // Normal case - same fee
        assert!(!FeeManager::is_fee_abnormal(25, 10000, 25, 10000, 1.5));

        // Normal case - slightly higher fee
        assert!(!FeeManager::is_fee_abnormal(30, 10000, 25, 10000, 1.5));

        // Abnormal case - fee increased by more than 50%
        assert!(FeeManager::is_fee_abnormal(40, 10000, 25, 10000, 1.5));
    }
}
