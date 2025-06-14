//! Meteora-specific mathematical calculations for Dynamic AMM and DLMM pools.

use rust_decimal::Decimal;
use anyhow::{anyhow, Result};
use std::str::FromStr;

/// Calculate output amount for Meteora Dynamic AMM pools
/// 
/// Dynamic AMM uses a constant product formula with Meteora-specific optimizations
pub fn calculate_dynamic_amm_output(
    input_amount: u64,
    input_reserve: u64,
    output_reserve: u64,
    fee_bps: u16,
) -> Result<u64> {
    if input_reserve == 0 || output_reserve == 0 {
        return Err(anyhow!("Pool reserves cannot be zero"));
    }

    if input_amount == 0 {
        return Ok(0);
    }

    // Convert to Decimal for precision
    let input_amount_decimal = Decimal::from(input_amount);
    let input_reserve_decimal = Decimal::from(input_reserve);
    let output_reserve_decimal = Decimal::from(output_reserve);
    let fee_rate = Decimal::from(fee_bps) / Decimal::from(10000u32);

    // Calculate fee
    let fee_amount = input_amount_decimal * fee_rate;
    let input_amount_after_fee = input_amount_decimal - fee_amount;

    // Constant product formula: (x + Δx) * (y - Δy) = x * y
    // Solving for Δy: Δy = (y * Δx) / (x + Δx)
    let numerator = output_reserve_decimal * input_amount_after_fee;
    let denominator = input_reserve_decimal + input_amount_after_fee;
    
    let output_amount_decimal = numerator / denominator;

    // Convert back to u64
    let output_amount = output_amount_decimal
        .to_u64()
        .ok_or_else(|| anyhow!("Output amount overflow"))?;

    Ok(output_amount)
}

/// Calculate output amount for Meteora DLMM (Dynamic Liquidity Market Maker) pools
/// 
/// DLMM uses bin-based pricing with concentrated liquidity
pub fn calculate_dlmm_output(
    input_amount: u64,
    active_bin_id: u32,
    bin_step: u16,
    fee_bps: u16,
) -> Result<u64> {
    if input_amount == 0 {
        return Ok(0);
    }

    // Convert to Decimal for precision
    let input_amount_decimal = Decimal::from(input_amount);
    let fee_rate = Decimal::from(fee_bps) / Decimal::from(10000u32);

    // Calculate fee
    let fee_amount = input_amount_decimal * fee_rate;
    let input_amount_after_fee = input_amount_decimal - fee_amount;

    // DLMM bin price calculation
    // Price = (1 + bin_step / 10000) ^ (active_bin_id - 2^23)
    let bin_step_decimal = Decimal::from(bin_step) / Decimal::from(10000u32);
    let one_plus_step = Decimal::ONE + bin_step_decimal;
    
    // Simplified bin price calculation (real implementation would use proper exponentiation)
    let bin_offset = active_bin_id as i64 - (1i64 << 23);
    let price_multiplier = if bin_offset >= 0 {
        // Approximate exponential for positive offset
        Decimal::ONE + (bin_step_decimal * Decimal::from(bin_offset.abs()))
    } else {
        // Approximate exponential for negative offset
        Decimal::ONE / (Decimal::ONE + (bin_step_decimal * Decimal::from(bin_offset.abs())))
    };

    // Calculate output based on bin price
    let output_amount_decimal = input_amount_after_fee * price_multiplier;

    // Convert back to u64
    let output_amount = output_amount_decimal
        .to_u64()
        .ok_or_else(|| anyhow!("Output amount overflow"))?;

    Ok(output_amount)
}

/// Calculate input amount required for a desired output (Dynamic AMM)
pub fn calculate_dynamic_amm_input_for_output(
    output_amount: u64,
    input_reserve: u64,
    output_reserve: u64,
    fee_bps: u16,
) -> Result<u64> {
    if input_reserve == 0 || output_reserve == 0 {
        return Err(anyhow!("Pool reserves cannot be zero"));
    }

    if output_amount == 0 {
        return Ok(0);
    }

    if output_amount >= output_reserve {
        return Err(anyhow!("Output amount exceeds available reserves"));
    }

    // Convert to Decimal for precision
    let output_amount_decimal = Decimal::from(output_amount);
    let input_reserve_decimal = Decimal::from(input_reserve);
    let output_reserve_decimal = Decimal::from(output_reserve);
    let fee_rate = Decimal::from(fee_bps) / Decimal::from(10000u32);

    // Reverse constant product formula
    // Given: (x + Δx_after_fee) * (y - Δy) = x * y
    // And: Δx_after_fee = Δx * (1 - fee_rate)
    // Solve for Δx: Δx = (x * Δy) / ((y - Δy) * (1 - fee_rate))
    
    let numerator = input_reserve_decimal * output_amount_decimal;
    let denominator = (output_reserve_decimal - output_amount_decimal) * (Decimal::ONE - fee_rate);
    
    let input_amount_decimal = numerator / denominator;

    // Convert back to u64
    let input_amount = input_amount_decimal
        .to_u64()
        .ok_or_else(|| anyhow!("Input amount overflow"))?;

    Ok(input_amount)
}

/// Calculate price impact for a trade
pub fn calculate_price_impact(
    input_amount: u64,
    input_reserve: u64,
    output_reserve: u64,
) -> Result<Decimal> {
    if input_reserve == 0 || output_reserve == 0 {
        return Err(anyhow!("Pool reserves cannot be zero"));
    }

    if input_amount == 0 {
        return Ok(Decimal::ZERO);
    }

    // Current price (output per input)
    let current_price = Decimal::from(output_reserve) / Decimal::from(input_reserve);
    
    // Price after trade
    let new_input_reserve = input_reserve + input_amount;
    let output_amount = calculate_dynamic_amm_output(input_amount, input_reserve, output_reserve, 0)?;
    let new_output_reserve = output_reserve - output_amount;
    
    let new_price = Decimal::from(new_output_reserve) / Decimal::from(new_input_reserve);
    
    // Price impact = (new_price - current_price) / current_price
    let price_impact = (new_price - current_price) / current_price;
    
    Ok(price_impact.abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dynamic_amm_output_calculation() {
        let input_amount = 1_000_000; // 1 token
        let input_reserve = 100_000_000; // 100 tokens
        let output_reserve = 200_000_000; // 200 tokens
        let fee_bps = 25; // 0.25%

        let result = calculate_dynamic_amm_output(input_amount, input_reserve, output_reserve, fee_bps);
        assert!(result.is_ok());
        
        let output = result.unwrap();
        assert!(output > 0);
        assert!(output < input_amount * 2); // Should be less than 2x due to fees and slippage
    }

    #[test]
    fn test_dynamic_amm_zero_input() {
        let result = calculate_dynamic_amm_output(0, 1000, 2000, 25);
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn test_dynamic_amm_zero_reserves() {
        let result = calculate_dynamic_amm_output(1000, 0, 2000, 25);
        assert!(result.is_err());
        
        let result = calculate_dynamic_amm_output(1000, 1000, 0, 25);
        assert!(result.is_err());
    }

    #[test]
    fn test_dlmm_output_calculation() {
        let input_amount = 1_000_000;
        let active_bin_id = 8388608; // 2^23 (neutral bin)
        let bin_step = 25; // 0.25%
        let fee_bps = 25;

        let result = calculate_dlmm_output(input_amount, active_bin_id, bin_step, fee_bps);
        assert!(result.is_ok());
        
        let output = result.unwrap();
        assert!(output > 0);
    }

    #[test]
    fn test_price_impact_calculation() {
        let input_amount = 1_000_000;
        let input_reserve = 100_000_000;
        let output_reserve = 200_000_000;

        let result = calculate_price_impact(input_amount, input_reserve, output_reserve);
        assert!(result.is_ok());
        
        let impact = result.unwrap();
        assert!(impact >= Decimal::ZERO);
        assert!(impact <= Decimal::ONE); // Impact should be between 0 and 100%
    }

    #[test]
    fn test_input_for_output_calculation() {
        let output_amount = 1_000_000;
        let input_reserve = 100_000_000;
        let output_reserve = 200_000_000;
        let fee_bps = 25;

        let result = calculate_dynamic_amm_input_for_output(
            output_amount, input_reserve, output_reserve, fee_bps
        );
        assert!(result.is_ok());
        
        let input = result.unwrap();
        assert!(input > 0);
        
        // Verify by calculating output with the computed input
        let computed_output = calculate_dynamic_amm_output(
            input, input_reserve, output_reserve, fee_bps
        ).unwrap();
        
        // Should be approximately equal (within 1% due to rounding)
        let diff = if computed_output > output_amount {
            computed_output - output_amount
        } else {
            output_amount - computed_output
        };
        assert!(diff <= output_amount / 100); // Within 1%
    }
}
