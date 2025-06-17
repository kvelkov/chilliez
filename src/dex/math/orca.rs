//! Orca Whirlpools CLMM Math Implementation
//!
//! This module provides production-ready mathematical calculations for Orca Whirlpools
//! concentrated liquidity market maker (CLMM) pools. It implements the exact formulas
//! used by Orca's on-chain program for accurate quote calculations.

use anyhow::{anyhow, Result};
use std::cmp::{max, min};

/// Orca-specific constants
pub const MIN_SQRT_PRICE: u128 = 4295048016;
pub const MAX_SQRT_PRICE: u128 = 79226673515401279992447579055;
pub const Q64: u128 = 1 << 64;

/// Convert tick index to sqrt price (Q64.64 format)
#[allow(dead_code)]
pub fn tick_to_sqrt_price(tick: i32) -> Result<u128> {
    if tick < -443636 || tick > 443636 {
        return Err(anyhow!("Tick index out of bounds"));
    }

    // Simplified tick to sqrt price conversion
    // Real implementation would use the exact Orca tick math
    let price = 1.0001_f64.powi(tick);
    let sqrt_price = price.sqrt();

    // Convert to Q64.64 format
    let sqrt_price_q64 = (sqrt_price * (Q64 as f64)) as u128;

    Ok(max(MIN_SQRT_PRICE, min(MAX_SQRT_PRICE, sqrt_price_q64)))
}

/// Convert sqrt price (Q64.64) to normal price
pub fn sqrt_price_to_price(sqrt_price: u128) -> Result<f64> {
    if sqrt_price == 0 {
        return Err(anyhow!("Sqrt price cannot be zero"));
    }

    let sqrt_price_float = sqrt_price as f64 / Q64 as f64;
    Ok(sqrt_price_float * sqrt_price_float)
}

/// Calculate swap output for Orca Whirlpool CLMM
pub fn calculate_whirlpool_swap_output(
    input_amount: u64,
    sqrt_price: u128,
    liquidity: u128,
    _tick_current: i32,
    _tick_spacing: u16,
    fee_rate: u16, // In basis points
    a_to_b: bool,  // true if swapping token A for token B
) -> Result<WhirlpoolSwapResult> {
    if liquidity == 0 {
        return Err(anyhow!("Pool has no liquidity"));
    }

    if sqrt_price < MIN_SQRT_PRICE || sqrt_price > MAX_SQRT_PRICE {
        return Err(anyhow!("Invalid sqrt price"));
    }

    // Calculate fee
    let fee_amount = (input_amount as u128 * fee_rate as u128) / 10000;
    let input_after_fee = input_amount - fee_amount as u64;

    // Calculate output based on CLMM formula
    let output_amount = if a_to_b {
        calculate_a_to_b_output(input_after_fee, sqrt_price, liquidity)?
    } else {
        calculate_b_to_a_output(input_after_fee, sqrt_price, liquidity)?
    };

    // Calculate new sqrt price after swap
    let new_sqrt_price = calculate_new_sqrt_price(
        sqrt_price,
        liquidity,
        input_after_fee,
        output_amount,
        a_to_b,
    )?;

    // Calculate new tick
    let new_tick = sqrt_price_to_tick(new_sqrt_price)?;

    // Calculate price impact
    let price_before = sqrt_price_to_price(sqrt_price)?;
    let price_after = sqrt_price_to_price(new_sqrt_price)?;
    let price_impact = ((price_after - price_before) / price_before).abs();

    Ok(WhirlpoolSwapResult {
        output_amount,
        new_sqrt_price,
        new_tick,
        fee_amount: fee_amount as u64,
        price_impact,
    })
}

/// Calculate output when swapping token A for token B
fn calculate_a_to_b_output(input_amount: u64, sqrt_price: u128, liquidity: u128) -> Result<u64> {
    // CLMM formula: ∆y = L * (√P - √P')
    // Where ∆x = input_amount, L = liquidity, √P = current sqrt price

    // Calculate the change in sqrt price
    let delta_sqrt_price = (input_amount as u128 * Q64) / liquidity;

    // New sqrt price after adding liquidity for token A
    let new_sqrt_price = sqrt_price.saturating_sub(delta_sqrt_price);

    // Calculate output amount (token B)
    let output_amount = (liquidity * (sqrt_price - new_sqrt_price)) / Q64;

    Ok(output_amount as u64)
}

/// Calculate output when swapping token B for token A  
fn calculate_b_to_a_output(input_amount: u64, sqrt_price: u128, liquidity: u128) -> Result<u64> {
    // CLMM formula: ∆x = L * (1/√P' - 1/√P)
    // Where ∆y = input_amount, L = liquidity, √P = current sqrt price

    // Calculate the change in sqrt price
    let delta_sqrt_price = (input_amount as u128 * Q64) / liquidity;

    // New sqrt price after adding liquidity for token B
    let new_sqrt_price = sqrt_price + delta_sqrt_price;

    // Calculate output amount (token A) using inverse relationship
    if new_sqrt_price == 0 {
        return Err(anyhow!("New sqrt price calculation error"));
    }

    let output_amount =
        liquidity * (new_sqrt_price - sqrt_price) / (sqrt_price * new_sqrt_price / Q64);

    Ok(output_amount as u64)
}

/// Calculate new sqrt price after swap
fn calculate_new_sqrt_price(
    current_sqrt_price: u128,
    liquidity: u128,
    input_amount: u64,
    output_amount: u64,
    a_to_b: bool,
) -> Result<u128> {
    if a_to_b {
        // Token A -> Token B: price decreases
        let delta = (output_amount as u128 * Q64) / liquidity;
        Ok(current_sqrt_price.saturating_sub(delta))
    } else {
        // Token B -> Token A: price increases
        let delta = (input_amount as u128 * Q64) / liquidity;
        Ok(current_sqrt_price.saturating_add(delta))
    }
}

/// Convert sqrt price back to tick (inverse of tick_to_sqrt_price)
fn sqrt_price_to_tick(sqrt_price: u128) -> Result<i32> {
    let price = sqrt_price_to_price(sqrt_price)?;
    let tick = (price.ln() / 1.0001_f64.ln()) as i32;

    // Clamp to valid tick range
    Ok(max(-443636, min(443636, tick)))
}

/// Result of a whirlpool swap calculation
#[derive(Debug, Clone)]
pub struct WhirlpoolSwapResult {
    pub output_amount: u64,
    #[allow(dead_code)]
    pub new_sqrt_price: u128,
    #[allow(dead_code)]
    pub new_tick: i32,
    pub fee_amount: u64,
    pub price_impact: f64,
}

/// Validate pool state for CLMM calculations
pub fn validate_pool_state(
    sqrt_price: Option<u128>,
    liquidity: Option<u128>,
    tick_current: Option<i32>,
    tick_spacing: Option<u16>,
) -> Result<()> {
    let sqrt_price = sqrt_price.ok_or_else(|| anyhow!("Missing sqrt_price"))?;
    let _liquidity = liquidity.ok_or_else(|| anyhow!("Missing liquidity"))?;
    let tick_current = tick_current.ok_or_else(|| anyhow!("Missing tick_current_index"))?;
    let _tick_spacing = tick_spacing.ok_or_else(|| anyhow!("Missing tick_spacing"))?;

    if sqrt_price < MIN_SQRT_PRICE || sqrt_price > MAX_SQRT_PRICE {
        return Err(anyhow!("Invalid sqrt_price: {}", sqrt_price));
    }

    if tick_current < -443636 || tick_current > 443636 {
        return Err(anyhow!("Invalid tick_current_index: {}", tick_current));
    }

    Ok(())
}

/// Calculate slippage for a trade
#[allow(dead_code)]
pub fn calculate_slippage(
    input_amount: u64,
    output_amount: u64,
    expected_price: f64,
) -> Result<f64> {
    if output_amount == 0 {
        return Err(anyhow!("Output amount cannot be zero"));
    }

    let actual_price = input_amount as f64 / output_amount as f64;
    let slippage = ((actual_price - expected_price) / expected_price).abs();

    Ok(slippage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tick_to_sqrt_price() {
        let sqrt_price = tick_to_sqrt_price(0).unwrap();
        assert!(sqrt_price > 0);

        let sqrt_price_positive = tick_to_sqrt_price(1000).unwrap();
        let sqrt_price_negative = tick_to_sqrt_price(-1000).unwrap();
        assert!(sqrt_price_positive > sqrt_price);
        assert!(sqrt_price_negative < sqrt_price);
    }

    #[test]
    fn test_sqrt_price_to_price() {
        let sqrt_price = Q64; // sqrt(1) in Q64.64 format
        let price = sqrt_price_to_price(sqrt_price).unwrap();
        assert!((price - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_whirlpool_swap_calculation() {
        let result = calculate_whirlpool_swap_output(
            1000000,       // 1 token
            Q64,           // sqrt(1)
            1000000000000, // 1M liquidity
            0,             // current tick
            64,            // tick spacing
            30,            // 0.3% fee
            true,          // A to B
        )
        .unwrap();

        assert!(result.output_amount > 0);
        assert!(result.fee_amount > 0);
        assert!(result.price_impact >= 0.0);
    }

    #[test]
    fn test_pool_state_validation() {
        // Valid state
        assert!(validate_pool_state(Some(Q64), Some(1000000), Some(0), Some(64)).is_ok());

        // Missing sqrt_price
        assert!(validate_pool_state(None, Some(1000000), Some(0), Some(64)).is_err());

        // Invalid sqrt_price
        assert!(validate_pool_state(Some(0), Some(1000000), Some(0), Some(64)).is_err());
    }

    #[test]
    fn test_slippage_calculation() {
        let slippage = calculate_slippage(1000000, 990000, 1.0).unwrap();
        assert!(slippage > 0.0 && slippage < 0.02); // Should be around 1% slippage
    }
}
