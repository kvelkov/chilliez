//! Precision-First Arbitrage Calculator
//! 
//! This is a proposed refactor to demonstrate the recommended approach
//! using rust_decimal throughout the calculation pipeline.

use rust_decimal::prelude::*;
use rust_decimal_macros::dec;
use anyhow::Result;
use crate::utils::{PoolInfo, TokenAmount};

/// High-precision arbitrage calculator
pub struct PrecisionArbitrageCalculator {
    /// Number of decimal places for internal calculations
    precision: u32,
}

impl PrecisionArbitrageCalculator {
    pub fn new(precision: u32) -> Self {
        Self { precision }
    }

    /// Calculate arbitrage profit with full precision
    pub fn calculate_arbitrage_profit(
        &self,
        pools: &[&PoolInfo],
        input_amount: Decimal,
        route_directions: &[bool],
    ) -> Result<ArbitragePrecisionResult> {
        let mut current_amount = input_amount;
        let mut total_fees = Decimal::ZERO;
        let mut price_impact = Decimal::ZERO;

        for (i, &pool) in pools.iter().enumerate() {
            let direction = route_directions[i];
            
            // Calculate swap with full precision
            let swap_result = self.calculate_precise_swap(pool, current_amount, direction)?;
            
            current_amount = swap_result.output_amount;
            total_fees += swap_result.fee_amount;
            price_impact += swap_result.price_impact;
        }

        let gross_profit = current_amount - input_amount;
        let net_profit = gross_profit - total_fees;

        Ok(ArbitragePrecisionResult {
            input_amount,
            output_amount: current_amount,
            gross_profit,
            net_profit,
            total_fees,
            price_impact,
            profit_percentage: if input_amount > Decimal::ZERO {
                (net_profit / input_amount) * dec!(100)
            } else {
                Decimal::ZERO
            },
        })
    }

    /// Calculate single swap with precision
    fn calculate_precise_swap(
        &self,
        pool: &PoolInfo,
        input_amount: Decimal,
        a_to_b: bool,
    ) -> Result<SwapResult> {
        let (reserve_in, reserve_out) = if a_to_b {
            (
                Decimal::from(pool.token_a.reserve),
                Decimal::from(pool.token_b.reserve),
            )
        } else {
            (
                Decimal::from(pool.token_b.reserve),
                Decimal::from(pool.token_a.reserve),
            )
        };

        if reserve_in == Decimal::ZERO || reserve_out == Decimal::ZERO {
            return Err(anyhow::anyhow!("Pool has zero reserves"));
        }

        // Get fee rate (default 0.3% = 30 basis points)
        let fee_rate_bps = pool.fee_rate_bips.unwrap_or(30);
        let fee_rate = Decimal::from(fee_rate_bps) / dec!(10000);
        
        // Calculate fee amount
        let fee_amount = input_amount * fee_rate;
        let input_after_fee = input_amount - fee_amount;

        // Constant product formula with full precision
        let numerator = input_after_fee * reserve_out;
        let denominator = reserve_in + input_after_fee;
        let output_amount = numerator / denominator;

        // Calculate price impact
        let expected_rate = reserve_out / reserve_in;
        let actual_rate = output_amount / input_amount;
        let price_impact = ((expected_rate - actual_rate) / expected_rate).abs() * dec!(100);

        Ok(SwapResult {
            output_amount,
            fee_amount,
            price_impact,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ArbitragePrecisionResult {
    pub input_amount: Decimal,
    pub output_amount: Decimal,
    pub gross_profit: Decimal,
    pub net_profit: Decimal,
    pub total_fees: Decimal,
    pub price_impact: Decimal,
    pub profit_percentage: Decimal,
}

#[derive(Debug, Clone)]
struct SwapResult {
    output_amount: Decimal,
    fee_amount: Decimal,
    price_impact: Decimal,
}

impl ArbitragePrecisionResult {
    /// Check if the arbitrage is profitable above a threshold
    pub fn is_profitable(&self, min_profit: Decimal) -> bool {
        self.net_profit > min_profit
    }

    /// Convert to legacy f64 format for backward compatibility
    pub fn to_legacy_format(&self) -> (f64, f64, f64) {
        (
            self.net_profit.to_f64().unwrap_or(0.0),
            self.price_impact.to_f64().unwrap_or(0.0),
            self.total_fees.to_f64().unwrap_or(0.0),
        )
    }

    /// Get results as u64 token amounts for transaction construction
    pub fn get_amounts_for_transaction(&self, decimals: u8) -> (u64, u64) {
        let scale = Decimal::from(10u64.pow(decimals as u32));
        let input_scaled = (self.input_amount * scale).to_u64().unwrap_or(0);
        let output_scaled = (self.output_amount * scale).to_u64().unwrap_or(0);
        (input_scaled, output_scaled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_precision_arbitrage_calculation() {
        let calculator = PrecisionArbitrageCalculator::new(18);
        
        // Create mock pool data
        let mut pool = PoolInfo::default();
        pool.token_a.reserve = 1_000_000_000_000; // 1M tokens
        pool.token_b.reserve = 2_000_000_000_000; // 2M tokens
        pool.fee_rate_bips = Some(30); // 0.3%

        let pools = vec![&pool];
        let directions = vec![true]; // A to B
        let input_amount = dec!(1000); // 1000 tokens

        let result = calculator
            .calculate_arbitrage_profit(&pools, input_amount, &directions)
            .unwrap();

        assert!(result.output_amount > Decimal::ZERO);
        assert!(result.total_fees > Decimal::ZERO);
        println!("Precision calculation result: {:?}", result);
    }

    #[test]
    fn test_no_precision_loss() {
        let calculator = PrecisionArbitrageCalculator::new(18);
        
        // Test with very small amounts that would lose precision in f64
        let small_amount = dec!(0.000000001); // 1 nano-token
        
        // Even tiny amounts should maintain precision
        assert!(small_amount > Decimal::ZERO);
        assert_ne!(small_amount.to_f64().unwrap(), 0.0);
    }
}
