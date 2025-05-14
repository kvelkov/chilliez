//! FeeManager: Centralized logic for all fee, slippage, and dynamic gas calculations across pools/DEXs.

use crate::dex::pool::{DexType, PoolInfo, TokenAmount};
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

/// Centralized manager for all dynamic fee/slippage/gas estimation.
/// Use this from arbitrage engine/detector before executing or quoting any trade.
pub struct FeeManager;

impl FeeManager {
    /// Full breakdown for a given single pool swap (input amount, a/b direction).
    pub fn estimate_pool_swap(
        pool: &PoolInfo,
        input_amount: &TokenAmount,
        is_a_to_b: bool,
        last_known_fee_numerator: Option<u64>,
        last_known_fee_denominator: Option<u64>,
        last_update_timestamp: Option<u64>,
    ) -> FeeBreakdown {
        // --- Fee calculation per DEX ---
        let fee_fraction = pool.fee_numerator as f64 / pool.fee_denominator as f64;
        let expected_fee = input_amount.to_float() * fee_fraction;

        // Reserve-based slippage estimate (basic xy=k model for now)
        let input_reserve = if is_a_to_b {
            pool.token_a.reserve
        } else {
            pool.token_b.reserve
        } as f64;
        let slippage = if input_reserve > 0.0 {
            input_amount.to_float() / (input_reserve + 1.0) // crude estimate, refactor for actual curve
        } else {
            0.0
        };

        // Estimate gas (can be improved with empirical values per DEX/type)
        let gas_cost = match pool.dex_type {
            DexType::Raydium | DexType::Orca => 5000, // Use lamports: placeholder!
            DexType::Whirlpool => 8000,
            _ => 6000,
        };

        // Spike/surge/fraud detection
        let sudden_fee_increase = if let (Some(last_num), Some(last_den)) =
            (last_known_fee_numerator, last_known_fee_denominator)
        {
            let last_fraction = last_num as f64 / last_den as f64;
            fee_fraction > (last_fraction * 1.5) // arbitrary 50%+ jump triggers alert
        } else {
            false
        };

        // Pool staleness
        let pool_stale = if let Some(ts) = last_update_timestamp {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            now > ts + 10 // Considered stale if last update older than 10 seconds
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

    /// Computes the aggregate fees/slippage/gas for a multi-hop arbitrage sequence.
    /// Pass a Vec of pools traversed (e.g., cross-DEX arbitrage).
    pub fn estimate_multi_hop(
        pools: &[&PoolInfo],
        amounts: &[TokenAmount],
        directions: &[bool], // true if a->b for each hop
        last_fee_data: &[(Option<u64>, Option<u64>, Option<u64>)], // fee_numerator, fee_denominator, last_update_timestamp per pool
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
            let breakdown = Self::estimate_pool_swap(
                pool, amount, *direction, fee_data.0, fee_data.1, fee_data.2,
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

    /// Estimates the total transaction cost including fees, slippage, and gas
    pub fn estimate_total_cost(
        fee_breakdown: &FeeBreakdown,
        token_price_usd: f64,
        lamports_per_sol: u64,
        sol_price_usd: f64,
    ) -> f64 {
        let fee_cost_usd = fee_breakdown.expected_fee * token_price_usd;
        let slippage_cost_usd = fee_breakdown.expected_slippage * token_price_usd;
        let gas_cost_sol = fee_breakdown.gas_cost as f64 / lamports_per_sol as f64;
        let gas_cost_usd = gas_cost_sol * sol_price_usd;

        fee_cost_usd + slippage_cost_usd + gas_cost_usd
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dex::pool::{DexType, PoolInfo, PoolToken, TokenAmount};
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
        let breakdown = FeeManager::estimate_pool_swap(
            &pool, &in_amt, true, fee_data.0, fee_data.1, fee_data.2,
        );
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

    #[test]
    fn test_estimate_total_cost() {
        let breakdown = FeeBreakdown {
            expected_fee: 0.001,       // 0.1% fee on 1 token
            expected_slippage: 0.0005, // 0.05% slippage
            gas_cost: 5000,            // 5000 lamports
            sudden_fee_increase: false,
            explanation: "Test".to_string(),
        };

        let token_price_usd = 100.0; // $100 per token
        let lamports_per_sol = 1_000_000_000; // 1 billion lamports per SOL
        let sol_price_usd = 50.0; // $50 per SOL

        let total_cost = FeeManager::estimate_total_cost(
            &breakdown,
            token_price_usd,
            lamports_per_sol,
            sol_price_usd,
        );

        // Expected:
        // Fee: 0.001 * $100 = $0.10
        // Slippage: 0.0005 * $100 = $0.05
        // Gas: (5000 / 1_000_000_000) * $50 = $0.00025
        // Total: $0.10 + $0.05 + $0.00025 = $0.15025
        assert!((total_cost - 0.15025).abs() < 0.0001);
    }
}
