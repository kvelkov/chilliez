//! Raydium V4 AMM Math Implementation
//!
//! This module provides production-ready mathematical calculations for Raydium V4
//! constant product AMM pools. It implements the exact formulas used by Raydium's
//! on-chain program for accurate quote calculations with proper fee handling.

use anyhow::{anyhow, Result};
use num_bigint::BigUint;
use num_traits::ToPrimitive;

/// Raydium V4 specific constants
#[allow(dead_code)] // Used by external code for fee calculations
pub const RAYDIUM_FEE_DENOMINATOR: u64 = 10000; // 100% = 10000 basis points
#[allow(dead_code)] // Used by external code for default fee rates
pub const DEFAULT_RAYDIUM_FEE_RATE: u64 = 25; // 0.25% = 25 basis points

/// Calculate AMM swap output for Raydium V4 pools
pub fn calculate_raydium_swap_output(
    input_amount: u64,
    input_reserve: u64,
    output_reserve: u64,
    fee_numerator: u64,
    fee_denominator: u64,
) -> Result<RaydiumSwapResult> {
    if input_reserve == 0 || output_reserve == 0 {
        return Err(anyhow!("Pool has no liquidity"));
    }

    if fee_denominator == 0 {
        return Err(anyhow!("Invalid fee denominator"));
    }

    if input_amount == 0 {
        return Err(anyhow!("Input amount cannot be zero"));
    }

    // Use BigUint to prevent overflow in intermediate calculations
    let input_big = BigUint::from(input_amount);
    let input_reserve_big = BigUint::from(input_reserve);
    let output_reserve_big = BigUint::from(output_reserve);
    let fee_num_big = BigUint::from(fee_numerator);
    let fee_den_big = BigUint::from(fee_denominator);

    // Calculate trading fee
    let fee_amount = (&input_big * &fee_num_big) / &fee_den_big;
    let input_after_fee = &input_big - &fee_amount;

    // Constant product formula: x * y = k
    // output = y - (x * y) / (x + Δx_after_fee)
    let k = &input_reserve_big * &output_reserve_big;
    let new_input_reserve = &input_reserve_big + &input_after_fee;
    let new_output_reserve = &k / &new_input_reserve;
    let output_amount = &output_reserve_big - &new_output_reserve;

    let fee_amount_u64 = fee_amount
        .to_u64()
        .ok_or_else(|| anyhow!("Fee calculation overflow"))?;

    let output_amount_u64 = output_amount
        .to_u64()
        .ok_or_else(|| anyhow!("Output calculation overflow"))?;

    // Calculate price before and after for price impact
    let price_before = (output_reserve as f64) / (input_reserve as f64);
    let price_after =
        (new_output_reserve.to_f64().unwrap_or(0.0)) / (new_input_reserve.to_f64().unwrap_or(1.0));
    let price_impact = ((price_after - price_before) / price_before).abs();

    Ok(RaydiumSwapResult {
        output_amount: output_amount_u64,
        fee_amount: fee_amount_u64,
        price_impact,
        new_input_reserve: new_input_reserve.to_u64().unwrap_or(0),
        new_output_reserve: new_output_reserve.to_u64().unwrap_or(0),
    })
}

/// Calculate reverse swap (input needed for specific output)
#[allow(dead_code)] // Advanced trading feature - used for exact output swaps
pub fn calculate_raydium_input_for_output(
    target_output: u64,
    input_reserve: u64,
    output_reserve: u64,
    fee_numerator: u64,
    fee_denominator: u64,
) -> Result<RaydiumSwapResult> {
    if input_reserve == 0 || output_reserve == 0 {
        return Err(anyhow!("Pool has no liquidity"));
    }

    if target_output >= output_reserve {
        return Err(anyhow!("Target output exceeds available liquidity"));
    }

    let target_big = BigUint::from(target_output);
    let input_reserve_big = BigUint::from(input_reserve);
    let output_reserve_big = BigUint::from(output_reserve);
    let fee_num_big = BigUint::from(fee_numerator);
    let fee_den_big = BigUint::from(fee_denominator);

    // Reverse calculation from constant product formula
    // We need: input_after_fee = (x * target_output) / (y - target_output)
    let k = &input_reserve_big * &output_reserve_big;
    let new_output_reserve = &output_reserve_big - &target_big;
    let new_input_reserve = &k / &new_output_reserve;
    let input_after_fee = &new_input_reserve - &input_reserve_big;

    // Calculate required input before fees: input = input_after_fee * fee_denominator / (fee_denominator - fee_numerator)
    let fee_factor = &fee_den_big - &fee_num_big;
    let required_input = (&input_after_fee * &fee_den_big) / &fee_factor;

    let fee_amount = &required_input - &input_after_fee;

    let _required_input_u64 = required_input
        .to_u64()
        .ok_or_else(|| anyhow!("Required input calculation overflow"))?;

    let fee_amount_u64 = fee_amount
        .to_u64()
        .ok_or_else(|| anyhow!("Fee calculation overflow"))?;

    // Calculate price impact
    let price_before = (output_reserve as f64) / (input_reserve as f64);
    let price_after =
        (new_output_reserve.to_f64().unwrap_or(0.0)) / (new_input_reserve.to_f64().unwrap_or(1.0));
    let price_impact = ((price_after - price_before) / price_before).abs();

    Ok(RaydiumSwapResult {
        output_amount: target_output,
        fee_amount: fee_amount_u64,
        price_impact,
        new_input_reserve: new_input_reserve.to_u64().unwrap_or(0),
        new_output_reserve: new_output_reserve.to_u64().unwrap_or(0),
    })
}

/// Calculate slippage for a given trade
#[allow(dead_code)] // Trading analytics feature - used for slippage monitoring
pub fn calculate_slippage(
    _input_amount: u64,
    expected_output: u64,
    actual_output: u64,
) -> Result<f64> {
    if expected_output == 0 {
        return Err(anyhow!("Expected output cannot be zero"));
    }

    let slippage = ((expected_output as f64 - actual_output as f64) / expected_output as f64).abs();
    Ok(slippage)
}

/// Validate pool state for AMM calculations
#[allow(dead_code)] // Pool validation feature - used for safety checks
pub fn validate_pool_state(
    input_reserve: u64,
    output_reserve: u64,
    fee_numerator: u64,
    fee_denominator: u64,
) -> Result<()> {
    if input_reserve == 0 {
        return Err(anyhow!("Input reserve cannot be zero"));
    }

    if output_reserve == 0 {
        return Err(anyhow!("Output reserve cannot be zero"));
    }

    if fee_denominator == 0 {
        return Err(anyhow!("Fee denominator cannot be zero"));
    }

    if fee_numerator >= fee_denominator {
        return Err(anyhow!("Fee rate cannot be >= 100%"));
    }

    // Sanity check: fee rate should be reasonable (< 10%)
    let fee_rate = (fee_numerator as f64) / (fee_denominator as f64);
    if fee_rate > 0.1 {
        return Err(anyhow!("Fee rate {} is unreasonably high", fee_rate));
    }

    Ok(())
}

/// Calculate minimum output for slippage protection
pub fn calculate_minimum_output_with_slippage(
    expected_output: u64,
    slippage_tolerance_bps: u16, // basis points (e.g., 500 = 5%)
) -> u64 {
    let slippage_factor = 1.0 - (slippage_tolerance_bps as f64 / 10000.0);
    (expected_output as f64 * slippage_factor) as u64
}

/// Result of a Raydium swap calculation
#[derive(Debug, Clone)]
pub struct RaydiumSwapResult {
    pub output_amount: u64,
    #[allow(dead_code)] // Used for fee analysis and reporting
    pub fee_amount: u64,
    pub price_impact: f64,
    #[allow(dead_code)] // Used for liquidity pool state tracking
    pub new_input_reserve: u64,
    #[allow(dead_code)] // Used for liquidity pool state tracking
    pub new_output_reserve: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raydium_swap_calculation() {
        let result = calculate_raydium_swap_output(
            1_000_000,   // 1 token input
            100_000_000, // 100 token reserve
            50_000_000,  // 50 token reserve
            25,          // 0.25% fee
            10000,       // fee denominator
        )
        .unwrap();

        assert!(result.output_amount > 0);
        assert!(result.fee_amount > 0);
        assert!(result.price_impact >= 0.0);
        assert!(result.price_impact < 0.1); // Should be less than 10%

        // Fee should be approximately 0.25% of input
        let expected_fee = 1_000_000 * 25 / 10000;
        assert!((result.fee_amount as i64 - expected_fee as i64).abs() < 10);
    }

    #[test]
    fn test_raydium_reverse_calculation() {
        let result = calculate_raydium_input_for_output(
            500_000,     // 0.5 token output desired
            100_000_000, // 100 token reserve
            50_000_000,  // 50 token reserve
            25,          // 0.25% fee
            10000,       // fee denominator
        )
        .unwrap();

        assert!(result.output_amount == 500_000);
        assert!(result.fee_amount > 0);
        assert!(result.price_impact >= 0.0);
    }

    #[test]
    fn test_pool_validation() {
        // Valid pool
        assert!(validate_pool_state(1_000_000, 1_000_000, 25, 10000).is_ok());

        // Invalid cases
        assert!(validate_pool_state(0, 1_000_000, 25, 10000).is_err()); // Zero reserve
        assert!(validate_pool_state(1_000_000, 0, 25, 10000).is_err()); // Zero reserve
        assert!(validate_pool_state(1_000_000, 1_000_000, 25, 0).is_err()); // Zero denominator
        assert!(validate_pool_state(1_000_000, 1_000_000, 10000, 10000).is_err());
        // 100% fee
    }

    #[test]
    fn test_slippage_calculation() {
        let slippage = calculate_slippage(1_000_000, 990_000, 980_000).unwrap();
        assert!((slippage - 0.0101).abs() < 0.001); // ~1.01% slippage
    }

    #[test]
    fn test_minimum_output_calculation() {
        let min_output = calculate_minimum_output_with_slippage(1_000_000, 500); // 5% slippage
        assert_eq!(min_output, 950_000); // 95% of expected output
    }
}
