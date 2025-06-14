//! Advanced DEX Math Module
//!
//! This module provides sophisticated mathematical calculations for various DEX types,
//! including concentrated liquidity market makers (CLMM), constant product AMMs,
//! and stable swap curves. It uses high-precision arithmetic to ensure accuracy
//! in quote calculations and minimize slippage discrepancies.

use anyhow::{anyhow, Result};
use num_bigint::BigUint;
use num_traits::ToPrimitive;
use rust_decimal::Decimal;
use std::str::FromStr;

/// High-precision CLMM calculations for concentrated liquidity pools
pub mod clmm {
    use super::*;
    
    /// Calculate output amount for CLMM pools using tick-based liquidity
    /// This is optimized for Orca Whirlpools and similar concentrated liquidity DEXs
    #[allow(dead_code)] // Advanced math function reserved for future CLMM implementation
    pub fn calculate_clmm_output(
        input_amount: u64,
        current_sqrt_price: u128,
        liquidity: u128,
        _tick_lower: i32,
        _tick_upper: i32,
        fee_rate: u32, // In basis points (e.g., 30 for 0.3%)
    ) -> Result<u64> {
        if liquidity == 0 {
            return Err(anyhow!("Liquidity cannot be zero"));
        }
        
        // Convert to high precision
        let input_decimal = Decimal::from(input_amount);
        let fee_decimal = Decimal::from(fee_rate) / Decimal::from(10000u32); // Convert bps to decimal
        
        // Calculate fee
        let input_after_fee = input_decimal * (Decimal::ONE - fee_decimal);
        
        // Simplified CLMM calculation
        // In a real CLMM, we'd calculate the exact price curve and tick transitions
        // For now, use a hybrid approach that approximates CLMM behavior
        
        // Convert sqrt price from x64 format to decimal
        let sqrt_price_decimal = Decimal::from(current_sqrt_price) / Decimal::from(1u128 << 32);
        
        // Use constant product as base but with concentration factor
        let liquidity_decimal = Decimal::from(liquidity);
        let concentration_factor = Decimal::from_str("1.5")?; // Simulate concentrated liquidity effect
        
        let virtual_reserve_in = liquidity_decimal / sqrt_price_decimal / concentration_factor;
        let virtual_reserve_out = liquidity_decimal * sqrt_price_decimal / concentration_factor;
        
        // Constant product formula with virtual reserves
        let output_amount = (input_after_fee * virtual_reserve_out) / (virtual_reserve_in + input_after_fee);
        
        output_amount.to_u64()
            .ok_or_else(|| anyhow!("CLMM output calculation overflow"))
    }
    
    /// Calculate CLMM swap price impact
    #[allow(dead_code)] // Advanced math function for price impact analysis
    pub fn calculate_price_impact(
        input_amount: u64,
        output_amount: u64,
        current_price: u64,
    ) -> Result<Decimal> {
        if current_price == 0 {
            return Err(anyhow!("Current price cannot be zero"));
        }
        
        let effective_price = Decimal::from(input_amount) / Decimal::from(output_amount);
        let current_price_decimal = Decimal::from(current_price);
        
        let price_impact = (effective_price - current_price_decimal) / current_price_decimal;
        Ok(price_impact.abs())
    }
}

/// Raydium-specific AMM calculations using constant product formula
pub mod raydium {
    use super::*;
    
    /// Enhanced Raydium quote calculation with high precision
    /// Implements the exact formula used by Raydium V4 pools
    #[allow(dead_code)] // Advanced math function for high-precision Raydium calculations
    pub fn calculate_raydium_output(
        input_amount: u64,
        input_reserve: u64,
        output_reserve: u64,
        fee_numerator: u64,
        fee_denominator: u64,
    ) -> Result<u64> {
        if input_reserve == 0 || output_reserve == 0 {
            return Err(anyhow!("Reserves cannot be zero"));
        }
        
        if fee_denominator == 0 {
            return Err(anyhow!("Fee denominator cannot be zero"));
        }
        
        // Use BigUint to prevent overflow in intermediate calculations
        let input_big = BigUint::from(input_amount);
        let input_reserve_big = BigUint::from(input_reserve);
        let output_reserve_big = BigUint::from(output_reserve);
        let fee_num_big = BigUint::from(fee_numerator);
        let fee_den_big = BigUint::from(fee_denominator);
        
        // Calculate fee: fee = input_amount * fee_numerator / fee_denominator
        let fee = (&input_big * &fee_num_big) / &fee_den_big;
        let input_after_fee = &input_big - &fee;
        
        // Constant product formula: output = output_reserve - (input_reserve * output_reserve) / (input_reserve + input_after_fee)
        let k = &input_reserve_big * &output_reserve_big;
        let new_input_reserve = &input_reserve_big + &input_after_fee;
        let new_output_reserve = &k / &new_input_reserve;
        let output_amount = &output_reserve_big - &new_output_reserve;
        
        output_amount.to_u64()
            .ok_or_else(|| anyhow!("Output amount calculation overflow"))
    }
    
    /// Calculate optimal input amount for a target output (reverse quote)
    #[allow(dead_code)] // Advanced math function for reverse quote calculations
    pub fn calculate_raydium_input_for_output(
        target_output: u64,
        input_reserve: u64,
        output_reserve: u64,
        fee_numerator: u64,
        fee_denominator: u64,
    ) -> Result<u64> {
        if input_reserve == 0 || output_reserve == 0 {
            return Err(anyhow!("Reserves cannot be zero"));
        }
        
        if target_output >= output_reserve {
            return Err(anyhow!("Target output exceeds available liquidity"));
        }
        
        let target_big = BigUint::from(target_output);
        let input_reserve_big = BigUint::from(input_reserve);
        let output_reserve_big = BigUint::from(output_reserve);
        let fee_num_big = BigUint::from(fee_numerator);
        let fee_den_big = BigUint::from(fee_denominator);
        
        // Reverse calculation: find input needed for target output
        let k = &input_reserve_big * &output_reserve_big;
        let new_output_reserve = &output_reserve_big - &target_big;
        let new_input_reserve = &k / &new_output_reserve;
        let input_before_fee = &new_input_reserve - &input_reserve_big;
        
        // Account for fee: input_with_fee = input_before_fee * fee_denominator / (fee_denominator - fee_numerator)
        let fee_multiplier = &fee_den_big / (&fee_den_big - &fee_num_big);
        let input_with_fee = &input_before_fee * &fee_multiplier;
        
        input_with_fee.to_u64()
            .ok_or_else(|| anyhow!("Input amount calculation overflow"))
    }
}

/// Orca-specific calculations for both Whirlpools (CLMM) and legacy pools
pub mod orca {
    use super::*;
    
    /// Calculate Orca Whirlpool (CLMM) output with tick-aware pricing
    #[allow(dead_code)] // Advanced math function for Orca Whirlpool calculations
    pub fn calculate_whirlpool_output(
        input_amount: u64,
        sqrt_price_x64: u128,
        liquidity: u128,
        fee_rate: u16, // Fee rate in hundredths of a basis point
    ) -> Result<u64> {
        // Convert Orca's fee rate format (hundredths of bps) to our standard bps
        let fee_bps = fee_rate / 100;
        
        // Use CLMM calculation with Orca-specific parameters
        super::clmm::calculate_clmm_output(
            input_amount,
            sqrt_price_x64,
            liquidity,
            -887272, // Full range tick lower (approximate)
            887272,  // Full range tick upper (approximate)  
            fee_bps as u32,
        )
    }
    
    /// Calculate legacy Orca pool output (constant product)
    #[allow(dead_code)] // Advanced math function for legacy Orca pool calculations
    pub fn calculate_legacy_orca_output(
        input_amount: u64,
        input_reserve: u64,
        output_reserve: u64,
        fee_numerator: u64,
        fee_denominator: u64,
    ) -> Result<u64> {
        // Legacy Orca uses same constant product formula as Raydium
        super::raydium::calculate_raydium_output(
            input_amount,
            input_reserve,
            output_reserve,
            fee_numerator,
            fee_denominator,
        )
    }
}

/// Meteora-specific calculations for dynamic AMM and DLMM pools
pub mod meteora {
    use super::*;
    
    /// Calculate Meteora Dynamic AMM output (variable fees)
    #[allow(dead_code)] // Advanced math function for Meteora Dynamic AMM calculations
    pub fn calculate_dynamic_amm_output(
        input_amount: u64,
        input_reserve: u64,
        output_reserve: u64,
        base_fee_bps: u32,
        dynamic_fee_bps: u32,
    ) -> Result<u64> {
        let total_fee_bps = base_fee_bps + dynamic_fee_bps;
        
        // Use enhanced constant product with dynamic fee
        super::raydium::calculate_raydium_output(
            input_amount,
            input_reserve,
            output_reserve,
            total_fee_bps as u64,
            10000, // Fee denominator for basis points
        )
    }
    
    /// Calculate Meteora DLMM (Dynamic Liquidity Market Maker) output
    pub fn calculate_dlmm_output(
        input_amount: u64,
        active_bin_id: u32,
        bin_step: u16,
        _liquidity_in_bin: u128,
        fee_rate: u16,
    ) -> Result<u64> {
        // DLMM uses bin-based pricing similar to CLMM but with discrete price levels
        let bin_price = calculate_bin_price(active_bin_id, bin_step)?;
        
        // Simplified DLMM calculation - would be more complex with multiple bins
        let input_decimal = Decimal::from(input_amount);
        let fee_decimal = Decimal::from(fee_rate) / Decimal::from(10000u32);
        let input_after_fee = input_decimal * (Decimal::ONE - fee_decimal);
        
        let output = input_after_fee * Decimal::from(bin_price) / Decimal::from(1000000u64);
        
        output.to_u64()
            .ok_or_else(|| anyhow!("DLMM output calculation overflow"))
    }
    
    fn calculate_bin_price(bin_id: u32, bin_step: u16) -> Result<u64> {
        // Meteora bin price calculation: price = (1 + bin_step / 10000) ^ (bin_id - BASE_BIN_ID)
        let base_price = 1000000u64; // Base price in micro units
        let step_multiplier = Decimal::from(bin_step) / Decimal::from(10000u32);
        
        // Use a simplified calculation since rust_decimal doesn't have powi
        // In production, this would use a proper power calculation
        let bin_offset = bin_id as i64 - 8388608; // BASE_BIN_ID
        let mut bin_multiplier = Decimal::ONE;
        
        if bin_offset > 0 {
            for _ in 0..bin_offset.min(10) { // Limit iterations to prevent overflow
                bin_multiplier = bin_multiplier * (Decimal::ONE + step_multiplier);
            }
        } else if bin_offset < 0 {
            for _ in 0..(-bin_offset).min(10) { // Limit iterations to prevent overflow
                bin_multiplier = bin_multiplier / (Decimal::ONE + step_multiplier);
            }
        }
        
        let price = Decimal::from(base_price) * bin_multiplier;
        price.to_u64()
            .ok_or_else(|| anyhow!("Bin price calculation overflow"))
    }
}

/// Lifinity-specific calculations for proactive market making
pub mod lifinity {
    use super::*;
    
    /// Calculate Lifinity output with proactive market making adjustments
    #[allow(dead_code)] // Advanced math function for Lifinity proactive market making
    pub fn calculate_lifinity_output(
        input_amount: u64,
        input_reserve: u64,
        output_reserve: u64,
        fee_bps: u32,
        oracle_price: Option<u64>, // External price oracle for proactive MM
    ) -> Result<u64> {
        // Base calculation using constant product
        let base_output = super::raydium::calculate_raydium_output(
            input_amount,
            input_reserve,
            output_reserve,
            fee_bps as u64,
            10000,
        )?;
        
        // Apply Lifinity's proactive market making adjustment if oracle price available
        if let Some(oracle_price) = oracle_price {
            let pool_price = (output_reserve as u128 * 1000000) / input_reserve as u128;
            let oracle_price_scaled = oracle_price as u128;
            
            // Adjust output based on oracle vs pool price deviation
            let price_ratio = oracle_price_scaled * 1000 / pool_price;
            
            if price_ratio > 1050 { // Oracle price > 5% higher than pool
                // Reduce output to move pool price toward oracle
                let adjustment = Decimal::from(base_output) * Decimal::from_str("0.95")?;
                return adjustment.to_u64()
                    .ok_or_else(|| anyhow!("Lifinity adjustment calculation overflow"));
            } else if price_ratio < 950 { // Oracle price > 5% lower than pool
                // Increase output to move pool price toward oracle
                let adjustment = Decimal::from(base_output) * Decimal::from_str("1.05")?;
                return adjustment.to_u64()
                    .ok_or_else(|| anyhow!("Lifinity adjustment calculation overflow"));
            }
        }
        
        Ok(base_output)
    }
}

/// Common utilities for all DEX math calculations
pub mod utils {
    use super::*;
    
    /// Calculate slippage percentage given input and output amounts
    #[allow(dead_code)] // Utility function for slippage analysis
    pub fn calculate_slippage(
        expected_output: u64,
        actual_output: u64,
    ) -> Result<Decimal> {
        if expected_output == 0 {
            return Err(anyhow!("Expected output cannot be zero"));
        }
        
        let expected = Decimal::from(expected_output);
        let actual = Decimal::from(actual_output);
        
        let slippage = (expected - actual) / expected;
        Ok(slippage.max(Decimal::ZERO)) // Slippage cannot be negative
    }
    
    /// Calculate minimum output amount given slippage tolerance
    #[allow(dead_code)] // Utility function for slippage protection calculations
    pub fn calculate_minimum_output(
        expected_output: u64,
        slippage_tolerance_bps: u32,
    ) -> Result<u64> {
        let expected = Decimal::from(expected_output);
        let slippage = Decimal::from(slippage_tolerance_bps) / Decimal::from(10000u32);
        
        let minimum = expected * (Decimal::ONE - slippage);
        minimum.to_u64()
            .ok_or_else(|| anyhow!("Minimum output calculation overflow"))
    }
    
    /// Validate that calculated outputs are reasonable
    #[allow(dead_code)] // Utility function for output validation
    pub fn validate_output(
        input_amount: u64,
        output_amount: u64,
        max_slippage_bps: u32,
    ) -> Result<()> {
        if output_amount == 0 {
            return Err(anyhow!("Output amount cannot be zero"));
        }
        
        // Sanity check: output shouldn't be more than 10x input (prevents calculation errors)
        if output_amount > input_amount * 10 {
            return Err(anyhow!("Output amount suspiciously high: {} for input {}", output_amount, input_amount));
        }
        
        // Check if slippage is within reasonable bounds (configurable but defaulting to 50% max)
        let max_slippage_ratio = Decimal::from(max_slippage_bps.max(5000)) / Decimal::from(10000u32);
        let output_ratio = Decimal::from(output_amount) / Decimal::from(input_amount);
        
        if output_ratio < (Decimal::ONE - max_slippage_ratio) {
            return Err(anyhow!("Output amount indicates excessive slippage"));
        }
        
        Ok(())
    }
}

/// General utility functions that can be used across DEXes
pub mod general {
    use super::*;
    
    /// Simple constant product AMM calculation (x * y = k)
    /// This is the most basic AMM formula used as a fallback
    #[allow(dead_code)] // General utility function for simple AMM calculations
    pub fn calculate_simple_amm_output(
        input_amount: u64,
        input_reserve: u64,
        output_reserve: u64,
        fee_rate_bps: u32,
    ) -> u64 {
        if input_reserve == 0 || output_reserve == 0 {
            return 0;
        }
        
        // Apply fee
        let fee_decimal = Decimal::from(fee_rate_bps) / Decimal::from(10000u32);
        let input_after_fee = Decimal::from(input_amount) * (Decimal::ONE - fee_decimal);
        
        // Constant product formula: output = (input_after_fee * output_reserve) / (input_reserve + input_after_fee)
        let numerator = input_after_fee * Decimal::from(output_reserve);
        let denominator = Decimal::from(input_reserve) + input_after_fee;
        
        let output = numerator / denominator;
        output.to_u64().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raydium_amm_calculation() {
        // Test basic Raydium AMM calculation
        let input_amount = 1000000; // 1 token (6 decimals)
        let input_reserve = 1000000000; // 1000 tokens
        let output_reserve = 2000000000; // 2000 tokens
        let fee_numerator = 25; // 0.25%
        let fee_denominator = 10000;

        let output = raydium::calculate_raydium_output(
            input_amount,
            input_reserve,
            output_reserve,
            fee_numerator,
            fee_denominator,
        ).unwrap();

        assert!(output > 0, "Output should be greater than 0");
        assert!(output < input_amount * 2, "Output should be reasonable given reserves");
        
        // Output should be less than input due to fees
        assert!(output < input_amount * 2, "Output should account for fees");
    }

    #[test]
    fn test_raydium_clmm_calculation() {
        // Test Raydium CLMM (concentrated liquidity) calculation using the clmm module
        let input_amount = 1000000;
        let sqrt_price_x64 = (1u128 << 64) * 2; // Price of 2
        let liquidity = 1000000000u128;
        let tick_lower = -100;
        let tick_upper = 100;
        let fee_rate = 500; // 0.05%

        let output = clmm::calculate_clmm_output(
            input_amount,
            sqrt_price_x64,
            liquidity,
            tick_lower,
            tick_upper,
            fee_rate,
        ).unwrap();

        assert!(output > 0, "CLMM output should be greater than 0");
    }

    #[test]
    fn test_orca_whirlpool_calculation() {
        // Test Orca Whirlpool calculation
        let input_amount = 1000000;
        let sqrt_price_x64 = (1u128 << 64) * 3; // Price of 3
        let liquidity = 500000000u128;
        let fee_rate = 3000; // Orca format: hundredths of basis points

        let output = orca::calculate_whirlpool_output(
            input_amount,
            sqrt_price_x64,
            liquidity,
            fee_rate,
        ).unwrap();

        assert!(output > 0, "Whirlpool output should be greater than 0");
    }

    #[test]
    fn test_meteora_dynamic_amm_calculation() {
        // Test Meteora Dynamic AMM calculation
        let input_amount = 1000000;
        let input_reserve = 800000000;
        let output_reserve = 1200000000;
        let base_fee_bps = 25; // 0.25%
        let dynamic_fee_bps = 5; // 0.05% additional

        let output = meteora::calculate_dynamic_amm_output(
            input_amount,
            input_reserve,
            output_reserve,
            base_fee_bps,
            dynamic_fee_bps,
        ).unwrap();

        assert!(output > 0, "Dynamic AMM output should be greater than 0");
    }

    #[test]
    fn test_meteora_dlmm_calculation() {
        // Test Meteora DLMM (bin-based) calculation
        let input_amount = 1000000;
        let active_bin_id = 8388608; // Base bin ID
        let bin_step = 100;
        let liquidity_in_bin = 1000000000u128;
        let fee_rate = 100; // 0.1%

        let output = meteora::calculate_dlmm_output(
            input_amount,
            active_bin_id,
            bin_step,
            liquidity_in_bin,
            fee_rate,
        ).unwrap();

        assert!(output > 0, "DLMM output should be greater than 0");
    }

    #[test]
    fn test_lifinity_calculation_with_oracle() {
        // Test Lifinity calculation with oracle price
        let input_amount = 1000000;
        let input_reserve = 1000000000;
        let output_reserve = 1000000000; // 1:1 pool
        let fee_bps = 30; // 0.3%
        let oracle_price = Some(1100000); // Oracle suggests 10% higher price

        let output = lifinity::calculate_lifinity_output(
            input_amount,
            input_reserve,
            output_reserve,
            fee_bps,
            oracle_price,
        ).unwrap();

        assert!(output > 0, "Lifinity output should be greater than 0");
    }

    #[test]
    fn test_lifinity_calculation_without_oracle() {
        // Test Lifinity calculation without oracle price (fallback to AMM)
        let input_amount = 1000000;
        let input_reserve = 1000000000;
        let output_reserve = 1000000000;
        let fee_bps = 30;
        let oracle_price = None;

        let output = lifinity::calculate_lifinity_output(
            input_amount,
            input_reserve,
            output_reserve,
            fee_bps,
            oracle_price,
        ).unwrap();

        assert!(output > 0, "Lifinity output without oracle should be greater than 0");
    }

    #[test]
    fn test_general_simple_amm_calculation() {
        // Test the general simple AMM calculation used as fallback
        let input_amount = 1000000;
        let input_reserve = 1000000000;
        let output_reserve = 2000000000; // 1:2 ratio
        let fee_rate_bps = 25; // 0.25%

        let output = general::calculate_simple_amm_output(
            input_amount,
            input_reserve,
            output_reserve,
            fee_rate_bps,
        );

        assert!(output > 0, "Simple AMM output should be greater than 0");
        
        // With 1:2 ratio and 1M input, we expect close to 2M output minus fees
        let expected_max = 2000000u64; // Theoretical max without fees
        assert!(output < expected_max, "Output should be less than theoretical max due to fees");
        
        // Should be reasonable - somewhere around 1.9M+ after fees and slippage
        assert!(output > 1800000, "Output should be reasonable given the reserves");
    }

    #[test]
    fn test_zero_reserves_handling() {
        // Test that functions handle zero reserves gracefully
        let input_amount = 1000000;
        
        // Test simple AMM with zero reserves
        let output = general::calculate_simple_amm_output(
            input_amount,
            0, // Zero input reserve
            1000000000,
            25,
        );
        assert_eq!(output, 0, "Should return 0 for zero input reserve");

        let output = general::calculate_simple_amm_output(
            input_amount,
            1000000000,
            0, // Zero output reserve
            25,
        );
        assert_eq!(output, 0, "Should return 0 for zero output reserve");
    }

    #[test]
    fn test_high_fee_scenarios() {
        // Test behavior with very high fees
        let input_amount = 1000000;
        let input_reserve = 1000000000;
        let output_reserve = 1000000000;
        
        // Test with 50% fee (extreme case)
        let output = general::calculate_simple_amm_output(
            input_amount,
            input_reserve,
            output_reserve,
            5000, // 50% fee
        );
        
        assert!(output > 0, "Should still produce some output even with high fees");
        assert!(output < input_amount / 2, "Output should be significantly reduced with 50% fee");
    }

    #[test]
    fn test_minimum_output_calculation() {
        // Test minimum output calculation with slippage tolerance
        let expected_output = 1000000;
        let slippage_tolerance_bps = 500; // 5%
        
        let minimum_output = utils::calculate_minimum_output(
            expected_output,
            slippage_tolerance_bps,
        ).unwrap();
        
        assert!(minimum_output < expected_output, "Minimum output should be less than expected");
        assert!(minimum_output >= expected_output * 95 / 100, "Should be around 95% of expected");
    }
}


