//! Lifinity-specific mathematical calculations with proactive market making.

use rust_decimal::Decimal;
use anyhow::{anyhow, Result};

/// Calculate output amount for Lifinity pools with proactive market making
/// 
/// Lifinity uses oracle prices and concentration parameters for rebalancing
pub fn calculate_lifinity_output(
    input_amount: u64,
    input_reserve: u64,
    output_reserve: u64,
    fee_bps: u16,
    oracle_price: Option<f64>,
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

    // Base constant product calculation
    let base_numerator = output_reserve_decimal * input_amount_after_fee;
    let base_denominator = input_reserve_decimal + input_amount_after_fee;
    let base_output = base_numerator / base_denominator;

    // Apply Lifinity's proactive market making adjustment
    let adjusted_output = if let Some(oracle_price) = oracle_price {
        apply_proactive_adjustment(
            base_output,
            input_amount_after_fee,
            input_reserve_decimal,
            output_reserve_decimal,
            oracle_price,
        )?
    } else {
        // Fallback to base calculation without oracle
        base_output
    };

    // Convert back to u64
    let output_amount = adjusted_output
        .to_u64()
        .ok_or_else(|| anyhow!("Output amount overflow"))?;

    Ok(output_amount)
}

/// Apply Lifinity's proactive market making adjustment
fn apply_proactive_adjustment(
    base_output: Decimal,
    input_amount: Decimal,
    input_reserve: Decimal,
    output_reserve: Decimal,
    oracle_price: f64,
) -> Result<Decimal> {
    // Current pool price
    let pool_price = output_reserve / input_reserve;
    let oracle_price_decimal = Decimal::try_from(oracle_price)
        .map_err(|_| anyhow!("Invalid oracle price"))?;

    // Calculate price deviation
    let price_deviation = (pool_price - oracle_price_decimal) / oracle_price_decimal;
    let abs_deviation = price_deviation.abs();

    // Concentration factor (higher concentration = more aggressive rebalancing)
    let concentration_factor = Decimal::from_str("0.1")?; // 10% concentration

    // Adjustment factor based on deviation and concentration
    let adjustment_factor = if abs_deviation > concentration_factor {
        // Apply rebalancing adjustment
        let rebalancing_strength = Decimal::from_str("0.05")?; // 5% adjustment strength
        
        if price_deviation > Decimal::ZERO {
            // Pool price > oracle price, encourage selling (increase output)
            Decimal::ONE + rebalancing_strength
        } else {
            // Pool price < oracle price, discourage selling (decrease output)
            Decimal::ONE - rebalancing_strength
        }
    } else {
        // Within concentration range, no adjustment
        Decimal::ONE
    };

    Ok(base_output * adjustment_factor)
}

/// Calculate Lifinity's dynamic fee based on market conditions
pub fn calculate_dynamic_fee(
    base_fee_bps: u16,
    volatility_factor: Option<f64>,
    liquidity_factor: Option<f64>,
) -> Result<u16> {
    let mut dynamic_fee = Decimal::from(base_fee_bps);

    // Adjust for volatility
    if let Some(volatility) = volatility_factor {
        let volatility_decimal = Decimal::try_from(volatility)
            .map_err(|_| anyhow!("Invalid volatility factor"))?;
        
        // Higher volatility = higher fees (up to 2x)
        let volatility_multiplier = Decimal::ONE + (volatility_decimal * Decimal::from_str("0.5")?);
        dynamic_fee *= volatility_multiplier;
    }

    // Adjust for liquidity
    if let Some(liquidity) = liquidity_factor {
        let liquidity_decimal = Decimal::try_from(liquidity)
            .map_err(|_| anyhow!("Invalid liquidity factor"))?;
        
        // Lower liquidity = higher fees
        if liquidity_decimal > Decimal::ZERO {
            let liquidity_adjustment = Decimal::ONE / liquidity_decimal.sqrt()
                .ok_or_else(|| anyhow!("Invalid liquidity calculation"))?;
            dynamic_fee *= liquidity_adjustment;
        }
    }

    // Cap the fee at reasonable limits (max 1%)
    let max_fee = Decimal::from(100u32); // 1%
    if dynamic_fee > max_fee {
        dynamic_fee = max_fee;
    }

    let final_fee = dynamic_fee
        .to_u16()
        .ok_or_else(|| anyhow!("Fee calculation overflow"))?;

    Ok(final_fee)
}

/// Calculate optimal rebalancing amount for Lifinity pools
pub fn calculate_rebalancing_amount(
    current_reserve_a: u64,
    current_reserve_b: u64,
    target_price: f64,
    max_rebalance_pct: f64,
) -> Result<(u64, u64)> {
    if current_reserve_a == 0 || current_reserve_b == 0 {
        return Err(anyhow!("Reserves cannot be zero"));
    }

    let reserve_a_decimal = Decimal::from(current_reserve_a);
    let reserve_b_decimal = Decimal::from(current_reserve_b);
    let target_price_decimal = Decimal::try_from(target_price)
        .map_err(|_| anyhow!("Invalid target price"))?;

    // Current price
    let current_price = reserve_b_decimal / reserve_a_decimal;
    
    // Calculate total value in terms of token B
    let total_value_b = reserve_b_decimal + (reserve_a_decimal * target_price_decimal);
    
    // Optimal reserves at target price
    let optimal_reserve_a = (total_value_b / (Decimal::TWO * target_price_decimal))
        .to_u64()
        .ok_or_else(|| anyhow!("Reserve A calculation overflow"))?;
    
    let optimal_reserve_b = (total_value_b / Decimal::TWO)
        .to_u64()
        .ok_or_else(|| anyhow!("Reserve B calculation overflow"))?;

    // Calculate rebalancing amounts
    let rebalance_a = if optimal_reserve_a > current_reserve_a {
        optimal_reserve_a - current_reserve_a
    } else {
        current_reserve_a - optimal_reserve_a
    };

    let rebalance_b = if optimal_reserve_b > current_reserve_b {
        optimal_reserve_b - current_reserve_b
    } else {
        current_reserve_b - optimal_reserve_b
    };

    // Apply maximum rebalancing percentage limit
    let max_rebalance_pct_decimal = Decimal::try_from(max_rebalance_pct)
        .map_err(|_| anyhow!("Invalid max rebalance percentage"))?;

    let max_rebalance_a = (reserve_a_decimal * max_rebalance_pct_decimal)
        .to_u64()
        .ok_or_else(|| anyhow!("Max rebalance A calculation overflow"))?;

    let max_rebalance_b = (reserve_b_decimal * max_rebalance_pct_decimal)
        .to_u64()
        .ok_or_else(|| anyhow!("Max rebalance B calculation overflow"))?;

    let final_rebalance_a = rebalance_a.min(max_rebalance_a);
    let final_rebalance_b = rebalance_b.min(max_rebalance_b);

    Ok((final_rebalance_a, final_rebalance_b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lifinity_output_without_oracle() {
        let input_amount = 1_000_000;
        let input_reserve = 100_000_000;
        let output_reserve = 200_000_000;
        let fee_bps = 25;

        let result = calculate_lifinity_output(
            input_amount, input_reserve, output_reserve, fee_bps, None
        );
        assert!(result.is_ok());
        
        let output = result.unwrap();
        assert!(output > 0);
        assert!(output < input_amount * 2); // Should be reasonable
    }

    #[test]
    fn test_lifinity_output_with_oracle() {
        let input_amount = 1_000_000;
        let input_reserve = 100_000_000;
        let output_reserve = 200_000_000;
        let fee_bps = 25;
        let oracle_price = 2.1; // Slightly above pool price of 2.0

        let result = calculate_lifinity_output(
            input_amount, input_reserve, output_reserve, fee_bps, Some(oracle_price)
        );
        assert!(result.is_ok());
        
        let output = result.unwrap();
        assert!(output > 0);
    }

    #[test]
    fn test_dynamic_fee_calculation() {
        let base_fee = 25; // 0.25%
        let volatility = 0.5; // 50% volatility
        let liquidity = 1.0; // Normal liquidity

        let result = calculate_dynamic_fee(base_fee, Some(volatility), Some(liquidity));
        assert!(result.is_ok());
        
        let dynamic_fee = result.unwrap();
        assert!(dynamic_fee >= base_fee); // Should be higher due to volatility
        assert!(dynamic_fee <= 100); // Should not exceed 1%
    }

    #[test]
    fn test_rebalancing_calculation() {
        let reserve_a = 100_000_000; // 100 tokens
        let reserve_b = 150_000_000; // 150 tokens (current price = 1.5)
        let target_price = 2.0; // Target price
        let max_rebalance_pct = 0.1; // 10% max rebalancing

        let result = calculate_rebalancing_amount(
            reserve_a, reserve_b, target_price, max_rebalance_pct
        );
        assert!(result.is_ok());
        
        let (rebalance_a, rebalance_b) = result.unwrap();
        
        // Should suggest some rebalancing
        assert!(rebalance_a > 0 || rebalance_b > 0);
        
        // Should respect max rebalancing limits
        assert!(rebalance_a <= reserve_a / 10); // Max 10% of reserve A
        assert!(rebalance_b <= reserve_b / 10); // Max 10% of reserve B
    }

    #[test]
    fn test_proactive_adjustment() {
        let base_output = Decimal::from(1000000);
        let input_amount = Decimal::from(500000);
        let input_reserve = Decimal::from(100000000);
        let output_reserve = Decimal::from(200000000);
        let oracle_price = 2.1; // Pool price is 2.0, oracle is 2.1

        let result = apply_proactive_adjustment(
            base_output, input_amount, input_reserve, output_reserve, oracle_price
        );
        assert!(result.is_ok());
        
        let adjusted_output = result.unwrap();
        // Should be different from base output due to oracle adjustment
        assert_ne!(adjusted_output, base_output);
    }

    #[test]
    fn test_zero_input_amount() {
        let result = calculate_lifinity_output(0, 1000, 2000, 25, None);
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn test_zero_reserves() {
        let result = calculate_lifinity_output(1000, 0, 2000, 25, None);
        assert!(result.is_err());
        
        let result = calculate_lifinity_output(1000, 1000, 0, 25, None);
        assert!(result.is_err());
    }
}
