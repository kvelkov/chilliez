//! Updated FeeManager: Centralized logic for all fee, slippage, and dynamic gas calculations across pools/DEXs.
//! Improvements include modular explanation building, refined risk scoring, and clean, extensible code structure.

use crate::utils::{DexType, PoolInfo, TokenAmount};
use std::time::{SystemTime, UNIX_EPOCH};

/// Trait for slippage estimation models.
pub trait SlippageModel: Send + Sync {
    fn estimate_slippage(
        &self,
        pool: &PoolInfo,
        input_amount: &TokenAmount,
        is_a_to_b: bool,
    ) -> f64;
}

/// Basic constant-product model for slippage estimation.
pub struct XYKSlippageModel;
impl SlippageModel for XYKSlippageModel {
    fn estimate_slippage(
        &self,
        pool: &PoolInfo,
        input_amount: &TokenAmount,
        is_a_to_b: bool,
    ) -> f64 {
        let input_reserve_float = if is_a_to_b {
            pool.token_a.reserve as f64
        } else {
            pool.token_b.reserve as f64
        };
        let input_amount_float = input_amount.to_float();
        if (input_reserve_float + input_amount_float).abs() < std::f64::EPSILON {
            return 1.0;
        }
        // Slippage = input_amount / (reserve + input_amount)
        input_amount_float / (input_reserve_float + input_amount_float)
    }
}

impl Default for XYKSlippageModel {
    fn default() -> Self {
        XYKSlippageModel
    }
}

/// Returns the gas cost for a given DEX (values are hard-coded for now but could be made dynamic).
pub fn get_gas_cost_for_dex(dex: DexType) -> u64 {
    match dex {
        DexType::Raydium | DexType::Orca => 500_000,
        DexType::Whirlpool | DexType::Lifinity => 700_000,
        // DexType::Phoenix => 700_000, // Disabled
        DexType::Meteora => 600_000,
        DexType::Unknown(_) => 500_000,
    }
}

/// Structure representing a comprehensive fee breakdown.
#[derive(Debug, Clone)]
pub struct FeeBreakdown {
    pub expected_fee_usd: f64, // Expected fee in USD
    pub expected_slippage: f64,
    pub gas_cost: u64,
    pub sudden_fee_increase: bool,
    pub explanation: String,
}

/// Helper function that builds a human-readable explanation string for fee details.
fn build_explanation(
    pool: &PoolInfo,
    fee_fraction: f64,
    slippage: f64,
    gas_cost: u64,
    fee_spike: bool,
    pool_stale: bool,
) -> String {
    let mut explanation = format!(
        "Pool: {}, Fee: {:.4}% ({}/{}), Slippage Est: {:.4}%, Gas: {}",
        pool.name,
        fee_fraction * 100.0,
        pool.fee_numerator.unwrap_or(0),
        pool.fee_denominator.unwrap_or(10000),
        slippage * 100.0,
        gas_cost,
    );
    if fee_spike {
        explanation.push_str(" [FEE SPIKE DETECTED!]");
    }
    if pool_stale {
        explanation.push_str(" [POOL STALE!]");
    }
    explanation
}

/// FeeManager contains static methods to estimate fees, slippage, and gas costs.
pub struct FeeManager;

impl FeeManager {
    /// Returns true if the current fee (numerator/denominator) is abnormally high compared to historical values.
    pub fn is_fee_abnormal(
        current_fee_numerator: u64,
        current_fee_denominator: u64,
        historical_fee_numerator: u64,
        historical_fee_denominator: u64,
        threshold_multiplier: f64,
    ) -> bool {
        if current_fee_denominator == 0 || historical_fee_denominator == 0 {
            return false; // Avoid division by zero.
        }
        let current_fee = current_fee_numerator as f64 / current_fee_denominator as f64;
        let historical_fee = historical_fee_numerator as f64 / historical_fee_denominator as f64;
        if historical_fee == 0.0 && current_fee > 0.0 {
            return true;
        }
        if historical_fee == 0.0 {
            return false;
        }
        current_fee > historical_fee * threshold_multiplier
    }

    /// Estimates the fee breakdown for a single pool swap (one hop).  
    /// This method calculates:
    ///   - Expected fee in USD using a placeholder conversion,
    ///   - Estimated slippage via the supplied slippage model,
    ///   - Gas cost based on DexType,
    ///   - Detects sudden fee increases and pool staleness.
    pub fn estimate_pool_swap_with_model(
        pool: &PoolInfo,
        input_amount: &TokenAmount,
        is_a_to_b: bool,
        last_known_fee_numerator: Option<u64>,
        last_known_fee_denominator: Option<u64>,
        last_update: Option<u64>,
        slippage_model: &dyn SlippageModel,
    ) -> FeeBreakdown {
        let fee_fraction = pool.fee_numerator.unwrap_or(0) as f64 / pool.fee_denominator.unwrap_or(10000).max(1) as f64;
        let expected_fee_native = input_amount.to_float() * fee_fraction;

        let input_token_symbol = if is_a_to_b {
            &pool.token_a.symbol
        } else {
            &pool.token_b.symbol
        };

        let expected_fee_usd = Self::convert_fee_to_reference_token(expected_fee_native, input_token_symbol, "USD")
            .unwrap_or_else(|| {
                log::warn!(
                    "Could not convert fee for {} to USD, using native amount {} as fallback.",
                    input_token_symbol,
                    expected_fee_native
                );
                expected_fee_native
            });

        let slippage = slippage_model.estimate_slippage(pool, input_amount, is_a_to_b);
        let gas_cost = get_gas_cost_for_dex(pool.dex_type.clone());
        let fee_spike = if let (Some(last_num), Some(last_den)) = (last_known_fee_numerator, last_known_fee_denominator) {
            Self::is_fee_abnormal(
                pool.fee_numerator.unwrap_or(0), 
                pool.fee_denominator.unwrap_or(10000), 
                last_num, 
                last_den, 
                1.5
            )
        } else {
            false
        };

        let pool_stale = if let Some(ts) = last_update {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            now.saturating_sub(ts) > 60 // Example: consider stale if older than 60 seconds.
        } else {
            pool.last_update_timestamp == 0
        };

        let explanation = build_explanation(pool, fee_fraction, slippage, gas_cost, fee_spike, pool_stale);

        FeeBreakdown {
            expected_fee_usd,
            expected_slippage: slippage,
            gas_cost,
            sudden_fee_increase: fee_spike,
            explanation,
        }
    }

    /// Estimates fees and slippage for a multi-hop swap.  
    /// Aggregates the USD fees across all hops and combines the slippage estimates multiplicatively.
    pub fn estimate_multi_hop_with_model(
        pools: &[&PoolInfo],
        amounts: &[TokenAmount],
        directions: &[bool],
        last_fee_data: &[(Option<u64>, Option<u64>, Option<u64>)],
        slippage_model: &dyn SlippageModel,
    ) -> FeeBreakdown {
        if pools.len() != amounts.len() || pools.len() != directions.len() || pools.len() != last_fee_data.len() {
            log::error!("estimate_multi_hop_with_model: Mismatched array lengths.");
            return FeeBreakdown {
                expected_fee_usd: 0.0,
                expected_slippage: 1.0,
                gas_cost: 0,
                sudden_fee_increase: true,
                explanation: "Error: Input array length mismatch".to_string(),
            };
        }

        let mut total_fee_usd = 0.0;
        let mut total_slippage_product = 1.0;
        let mut total_gas = 0;
        let mut fee_spike_detected = false;
        let mut explanations = Vec::new();

        for i in 0..pools.len() {
            let pool = pools[i];
            let amount = &amounts[i];
            let direction = directions[i];
            let (hist_fee_num, hist_fee_den, hist_ts) = last_fee_data[i];

            let breakdown = Self::estimate_pool_swap_with_model(
                pool,
                amount,
                direction,
                hist_fee_num,
                hist_fee_den,
                hist_ts.or(Some(pool.last_update_timestamp)),
                slippage_model,
            );

            total_fee_usd += breakdown.expected_fee_usd;
            total_slippage_product *= 1.0 - breakdown.expected_slippage;
            total_gas += breakdown.gas_cost;
            if breakdown.sudden_fee_increase {
                fee_spike_detected = true;
            }
            explanations.push(format!("Hop{} ({}): {}", i + 1, pool.name, breakdown.explanation));
        }

        let overall_slippage = 1.0 - total_slippage_product;
        FeeBreakdown {
            expected_fee_usd: total_fee_usd,
            expected_slippage: overall_slippage,
            gas_cost: total_gas,
            sudden_fee_increase: fee_spike_detected,
            explanation: explanations.join(" | "),
        }
    }

    /// Converts a fee amount from a source token to a reference token (e.g., USD).  
    /// This is currently a placeholder for a real integration with a price provider.
    pub fn convert_fee_to_reference_token(
        amount: f64,
        _from_symbol: &str,
        _to_symbol: &str,
    ) -> Option<f64> {
        // TODO: Replace with an actual price feed integration.
        Some(amount)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::{DexType, PoolInfo, PoolToken, TokenAmount};
    use solana_sdk::pubkey::Pubkey;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn create_test_pool() -> PoolInfo {
        PoolInfo {
            address: Pubkey::new_unique(),
            name: "Test SOL-USDC".to_string(),
            token_a: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "SOL".to_string(),
                decimals: 9,
                reserve: 1000 * 10u64.pow(9),
            },
            token_b: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 100_000 * 10u64.pow(6),
            },
            token_a_vault: Pubkey::new_unique(),
            token_b_vault: Pubkey::new_unique(),
            fee_numerator: Some(25),
            fee_denominator: Some(10000),
            fee_rate_bips: None,
            last_update_timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            dex_type: DexType::Raydium,
            liquidity: None,
            sqrt_price: None,
            tick_current_index: None,
            tick_spacing: None,
        }
    }

    #[test]
    fn test_is_fee_abnormal() {
        assert!(!FeeManager::is_fee_abnormal(25, 10000, 25, 10000, 1.5));
        assert!(!FeeManager::is_fee_abnormal(30, 10000, 25, 10000, 1.5));
        assert!(FeeManager::is_fee_abnormal(40, 10000, 25, 10000, 1.5));
        assert!(FeeManager::is_fee_abnormal(25, 10000, 0, 10000, 1.5));
        assert!(!FeeManager::is_fee_abnormal(0, 10000, 0, 10000, 1.5));
        assert!(!FeeManager::is_fee_abnormal(25, 10000, 25, 0, 1.5));
    }

    #[test]
    fn test_fee_spike_detection() {
        let pool = create_test_pool();
        let in_amt = TokenAmount::new(1 * 10u64.pow(9), 9); // 1 SOL

        let mut pool_spiked_fee = pool.clone();
        pool_spiked_fee.fee_numerator = Some(50);

        let breakdown = FeeManager::estimate_pool_swap_with_model(
            &pool_spiked_fee,
            &in_amt,
            true,
            pool.fee_numerator,
            pool.fee_denominator,
            Some(pool_spiked_fee.last_update_timestamp),
            &XYKSlippageModel::default(),
        );
        assert!(breakdown.sudden_fee_increase);
        assert!(breakdown.explanation.contains("[FEE SPIKE DETECTED!]"));

        let mut pool_normal_fee_increase = pool.clone();
        pool_normal_fee_increase.fee_numerator = Some(30);
        let breakdown_normal = FeeManager::estimate_pool_swap_with_model(
            &pool_normal_fee_increase,
            &in_amt,
            true,
            pool.fee_numerator,
            pool.fee_denominator,
            Some(pool_normal_fee_increase.last_update_timestamp),
            &XYKSlippageModel::default(),
        );
        assert!(!breakdown_normal.sudden_fee_increase);
        assert!(!breakdown_normal.explanation.contains("[FEE SPIKE DETECTED!]"));
    }
}
