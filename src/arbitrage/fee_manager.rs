//! FeeManager: Centralized logic for all fee, slippage, and dynamic gas calculations across pools/DEXs.

use crate::utils::{DexType, PoolInfo, TokenAmount};
use once_cell::sync::Lazy;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct FeeBreakdown {
    pub expected_fee_usd: f64, // Changed from expected_fee
    pub expected_slippage: f64,
    pub gas_cost: u64,
    pub sudden_fee_increase: bool,
    pub explanation: String,
}

pub struct FeeManager;

impl FeeManager {
    pub fn is_fee_abnormal(
        current_fee_numerator: u64,
        current_fee_denominator: u64,
        historical_fee_numerator: u64,
        historical_fee_denominator: u64,
        threshold_multiplier: f64,
    ) -> bool {
        if current_fee_denominator == 0 || historical_fee_denominator == 0 {
            return false; // Avoid division by zero
        }
        let current_fee = current_fee_numerator as f64 / current_fee_denominator as f64;
        let historical_fee = historical_fee_numerator as f64 / historical_fee_denominator as f64;
        if historical_fee == 0.0 && current_fee > 0.0 {
            // Any current fee is abnormal if historical was zero
            return true;
        }
        if historical_fee == 0.0 {
            return false;
        } // No change from zero

        current_fee > historical_fee * threshold_multiplier
    }

    pub fn estimate_pool_swap_with_model(
        pool: &PoolInfo,
        input_amount: &TokenAmount,
        is_a_to_b: bool,
        last_known_fee_numerator: Option<u64>,
        last_known_fee_denominator: Option<u64>,
        last_update_timestamp: Option<u64>, // This seems to be pool's own last_update_timestamp
        slippage_model: &dyn SlippageModel,
    ) -> FeeBreakdown {
        let fee_fraction = pool.fee_numerator as f64 / pool.fee_denominator as f64;
        let expected_fee_native_token = input_amount.to_float() * fee_fraction;
        
        let input_token_for_fee_calc_symbol = if is_a_to_b {
            &pool.token_a.symbol
        } else {
            &pool.token_b.symbol
        };

        let expected_fee_usd = Self::convert_fee_to_reference_token(
            expected_fee_native_token,
            input_token_for_fee_calc_symbol,
            "USD", // Assuming USD is the reference currency
        )
        .unwrap_or_else(|| {
            log::warn!(
                "Could not convert fee for {} to USD, using native amount {} as fallback.",
                input_token_for_fee_calc_symbol,
                expected_fee_native_token
            );
            expected_fee_native_token // Fallback to native token amount if conversion fails
        });

        let slippage = slippage_model.estimate_slippage(pool, input_amount, is_a_to_b);
        let gas_cost = get_gas_cost_for_dex(pool.dex_type.clone()); // Clone DexType
        let sudden_fee_increase = if let (Some(last_num), Some(last_den)) =
            (last_known_fee_numerator, last_known_fee_denominator)
        {
            // Check against current pool fees, not last_known vs some other historical
            Self::is_fee_abnormal(
                pool.fee_numerator,
                pool.fee_denominator,
                last_num,
                last_den, // Corrected typo from lastDen to last_den
                1.5,
            ) // 1.5x threshold
        } else {
            false
        };
        let pool_stale = if let Some(ts) = last_update_timestamp {
            // This should use pool.last_update_timestamp
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            now.saturating_sub(ts) > 60 // Example: stale if older than 60 seconds
        } else {
            // If last_update_timestamp is from the pool itself, and it's 0, it might be considered stale
            // or freshly initialized with no updates.
            pool.last_update_timestamp == 0 // Consider it stale if never updated
        };

        let mut explanation = format!(
            "Pool: {}, Fee: {:.4}% ({}/{}), Slippage Est: {:.4}%, Gas: {}",
            pool.name,
            fee_fraction * 100.0,
            pool.fee_numerator,
            pool.fee_denominator,
            slippage * 100.0,
            gas_cost,
        );
        if sudden_fee_increase {
            explanation.push_str(" [FEE SPIKE DETECTED!]");
        }
        if pool_stale {
            explanation.push_str(" [POOL STALE!]");
        }
        FeeBreakdown {
            expected_fee_usd, // Changed from expected_fee
            expected_slippage: slippage,
            gas_cost,
            sudden_fee_increase,
            explanation,
        }
    }

    pub fn estimate_pool_swap_integrated(
        pool: &PoolInfo,
        input_amount: &TokenAmount,
        is_a_to_b: bool,
        // last_update_timestamp from the pool can be passed directly
        // Or rely on pool.last_update_timestamp inside estimate_pool_swap_with_model
    ) -> FeeBreakdown {
        let (last_known_num, last_known_den) = FEE_HISTORY_TRACKER
            .get_last_fee(pool) // This now correctly calls get_last_fee_by_pubkey internally
            .unwrap_or((pool.fee_numerator, pool.fee_denominator)); // Use current if no history

        Self::estimate_pool_swap_with_model(
            pool,
            input_amount,
            is_a_to_b,
            Some(last_known_num),
            Some(last_known_den),
            Some(pool.last_update_timestamp), // Pass pool's own timestamp
            DEFAULT_SLIPPAGE_MODEL.as_ref(), // DEFAULT_SLIPPAGE_MODEL is now used
        )
    }

    pub fn estimate_multi_hop_with_model(
        pools: &[&PoolInfo],
        amounts: &[TokenAmount],
        directions: &[bool],
        last_fee_data: &[(Option<u64>, Option<u64>, Option<u64>)],
        slippage_model: &dyn SlippageModel,
    ) -> FeeBreakdown {
        let mut total_fee_usd = 0.0; // Changed from total_fee_tokens
        let mut total_slippage_fraction_product = 1.0; // Slippage multiplies: (1-s1)*(1-s2)...
        let mut total_gas_lamports = 0;
        let mut any_spike = false;
        let mut explanation_sections = Vec::new();

        if pools.len() != amounts.len()
            || pools.len() != directions.len()
            || pools.len() != last_fee_data.len()
        {
            // Log error or return a default error FeeBreakdown
            log::error!("estimate_multi_hop_with_model: Mismatched array lengths.");
            return FeeBreakdown {
                expected_fee_usd: 0.0, // Changed
                expected_slippage: 1.0,
                gas_cost: 0,
                sudden_fee_increase: true,
                explanation: "Error: Input array length mismatch".to_string(),
            };
        }

        for i in 0..pools.len() {
            let pool = pools[i];
            let amount = &amounts[i]; // This should be input to *this* hop
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
            // Fee conversion to a common currency (e.g., USD) would be needed for summation
            // This is now handled by estimate_pool_swap_with_model returning expected_fee_usd
            total_fee_usd += breakdown.expected_fee_usd; // Summing USD fees
            total_slippage_fraction_product *= 1.0 - breakdown.expected_slippage; // Removed unnecessary parentheses
            total_gas_lamports += breakdown.gas_cost;
            if breakdown.sudden_fee_increase {
                any_spike = true;
            }
            explanation_sections.push(format!(
                "Hop{}({}): {}",
                i + 1,
                pool.name,
                breakdown.explanation
            ));
        }

        let overall_slippage = 1.0 - total_slippage_fraction_product;

        FeeBreakdown {
            expected_fee_usd: total_fee_usd, // Changed
            expected_slippage: overall_slippage,
            gas_cost: total_gas_lamports,
            sudden_fee_increase: any_spike,
            explanation: explanation_sections.join(" | "),
        }
    }

    pub fn convert_fee_to_reference_token(
        amount: f64,
        _from_symbol: &str, // Underscore if price provider not yet integrated
        _to_symbol: &str,   // Underscore if price provider not yet integrated
    ) -> Option<f64> {
        // TODO: Integrate with actual price provider
        // For now, if converting to self, return amount. Otherwise, None.
        // if from_symbol == to_symbol { Some(amount) } else { None }
        Some(amount * 1.0) // Placeholder: assume 1.0 price for conversion for now
    }

    pub fn record_fee_observation(pool: &PoolInfo, fee_numerator: u64, fee_denominator: u64) {
        FEE_HISTORY_TRACKER.record_fee(pool.address, fee_numerator, fee_denominator); // FEE_HISTORY_TRACKER is now used
    }

    pub fn get_last_fee_for_pool(pool: &PoolInfo) -> Option<(u64, u64)> {
        FEE_HISTORY_TRACKER.get_last_fee_by_pubkey(&pool.address) // FEE_HISTORY_TRACKER is now used
    }
}

pub trait SlippageModel: Send + Sync {
    fn estimate_slippage(
        &self,
        pool: &PoolInfo,
        input_amount: &TokenAmount,
        is_a_to_b: bool,
    ) -> f64;
}

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

        if input_reserve_float + input_amount_float == 0.0 {
            return 1.0;
        } // Max slippage if no reserve or input

        // Slippage = (input_amount) / (input_reserve + input_amount)
        // This is one way to estimate slippage in a constant product pool.
        // More accurate models might consider the output amount.
        input_amount_float / (input_reserve_float + input_amount_float)
    }
}

pub fn get_gas_cost_for_dex(dex: DexType) -> u64 {
    match dex {
        DexType::Raydium | DexType::Orca => 500_000, // Increased, typical swaps can be complex
        DexType::Whirlpool | DexType::Lifinity | DexType::Phoenix => 700_000, // CLMMs/Orderbooks can be more
        DexType::Meteora => 600_000,
        DexType::Unknown(_) => 500_000, // Default
    }
}

// Simplified FeeHistoryTracker
// This struct is now used by record_fee_observation, get_last_fee_for_pool, and estimate_pool_swap_integrated.
pub struct FeeHistoryTracker {
    // Store last known fee for each pool address
    // In a real system, this would be more sophisticated (e.g., moving average, multiple samples)
    // and likely persisted (e.g., Redis or DB).
    last_fees: dashmap::DashMap<Pubkey, (u64, u64, u64)>, // num, den, timestamp
}
impl FeeHistoryTracker {
    // This method is now used by record_fee_observation.
    pub fn record_fee(&self, pool_address: Pubkey, fee_numerator: u64, fee_denominator: u64) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.last_fees
            .insert(pool_address, (fee_numerator, fee_denominator, timestamp));
    }
    // This method is now used by get_last_fee_for_pool.
    pub fn get_last_fee_by_pubkey(&self, pool_address: &Pubkey) -> Option<(u64, u64)> {
        self.last_fees
            .get(pool_address)
            .map(|entry| (entry.value().0, entry.value().1))
    }
    // This method is now used by estimate_pool_swap_integrated (via get_last_fee).
    pub fn get_last_fee(&self, pool: &PoolInfo) -> Option<(u64, u64)> {
        self.get_last_fee_by_pubkey(&pool.address)
    }
}
impl Default for FeeHistoryTracker {
    fn default() -> Self {
        FeeHistoryTracker {
            last_fees: dashmap::DashMap::new(),
        }
    }
}

// This static is now used by estimate_pool_swap_integrated.
pub static DEFAULT_SLIPPAGE_MODEL: Lazy<Arc<XYKSlippageModel>> =
    Lazy::new(|| Arc::new(XYKSlippageModel));
pub static FEE_HISTORY_TRACKER: Lazy<Arc<FeeHistoryTracker>> = Lazy::new(|| {
    Arc::new(FeeHistoryTracker {
        last_fees: dashmap::DashMap::new(),
    })
});
#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::{DexType, PoolInfo, PoolToken, TokenAmount};
    use solana_sdk::pubkey::Pubkey;

    fn create_test_pool() -> PoolInfo {
        PoolInfo {
            address: Pubkey::new_unique(),
            name: "Test SOL-USDC".to_string(),
            token_a: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "SOL".to_string(),
                decimals: 9,
                reserve: 1000 * 10u64.pow(9),
            }, // 1000 SOL
            token_b: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 100000 * 10u64.pow(6),
            }, // 100,000 USDC
            fee_numerator: 25,
            fee_denominator: 10000, // 0.25%
            last_update_timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            dex_type: DexType::Raydium,
        }
    }

    #[test]
    fn test_estimate_pool_swap_integrated() {
        let pool = create_test_pool();
        let in_amt = TokenAmount::new(1 * 10u64.pow(9), 9); // 1 SOL

        FEE_HISTORY_TRACKER.record_fee(pool.address, 25, 10000); // Prime history

        let breakdown = FeeManager::estimate_pool_swap_integrated(&pool, &in_amt, true); // SOL -> USDC

        assert!(breakdown.expected_fee_usd > 0.0); // Fee for 1 SOL at 0.25%
        assert_eq!(breakdown.expected_fee_usd, 1.0 * (25.0 / 10000.0)); // 1 SOL * 0.0025
        assert!(breakdown.expected_slippage >= 0.0);
        assert!(!breakdown.sudden_fee_increase); // Fee matches history
        assert!(!breakdown.explanation.contains("[POOL STALE!]"));
        assert!(breakdown.explanation.contains(&pool.name));
        log::debug!("Test Breakdown: {}", breakdown.explanation);
    }

    #[test]
    fn test_is_fee_abnormal() {
        assert!(!FeeManager::is_fee_abnormal(25, 10000, 25, 10000, 1.5)); // Same
        assert!(!FeeManager::is_fee_abnormal(30, 10000, 25, 10000, 1.5)); // Slightly higher, within threshold
        assert!(FeeManager::is_fee_abnormal(40, 10000, 25, 10000, 1.5)); // Abnormal, 60% higher
        assert!(FeeManager::is_fee_abnormal(25, 10000, 0, 10000, 1.5)); // Abnormal if historical was 0
        assert!(!FeeManager::is_fee_abnormal(0, 10000, 0, 10000, 1.5)); // Not abnormal if both are 0
        assert!(!FeeManager::is_fee_abnormal(25, 10000, 25, 0, 1.5)); // Denominator 0, should be false
    }

    #[test]
    fn test_fee_spike_detection() {
        let pool = create_test_pool();
        let in_amt = TokenAmount::new(1 * 10u64.pow(9), 9); // 1 SOL

        // Old fee was 0.25%
        FEE_HISTORY_TRACKER.record_fee(pool.address, 25, 10000);

        // Current pool fee is 0.50% (doubled)
        let mut pool_spiked_fee = pool.clone();
        pool_spiked_fee.fee_numerator = 50;

        let breakdown = FeeManager::estimate_pool_swap_integrated(&pool_spiked_fee, &in_amt, true);
        assert!(breakdown.sudden_fee_increase);
        assert!(breakdown.explanation.contains("[FEE SPIKE DETECTED!]"));
        log::debug!("Spike Test Breakdown: {}", breakdown.explanation);

        // Current pool fee is 0.30% (small increase)
        let mut pool_normal_fee_increase = pool.clone();
        pool_normal_fee_increase.fee_numerator = 30;
        let breakdown_normal =
            FeeManager::estimate_pool_swap_integrated(&pool_normal_fee_increase, &in_amt, true);
        assert!(!breakdown_normal.sudden_fee_increase);
        assert!(!breakdown_normal
            .explanation
            .contains("[FEE SPIKE DETECTED!]"));
    }
}
impl Default for XYKSlippageModel {
    fn default() -> Self {
        XYKSlippageModel
    }
}