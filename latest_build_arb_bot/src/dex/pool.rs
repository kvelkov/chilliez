use anyhow::Result;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::fmt;

// Import unified types and trait from utils
use crate::utils::{PoolInfo, PoolToken, DexType, PoolParser, TokenAmount};

pub type PoolParseFn = fn(address: Pubkey, data: &[u8]) -> Result<PoolInfo>;

pub static POOL_PARSER_REGISTRY: Lazy<HashMap<Pubkey, PoolParseFn>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert(
        crate::dex::orca::OrcaPoolParser::get_program_id(),
        crate::dex::orca::OrcaPoolParser::parse_pool_data as PoolParseFn,
    );
    m.insert(
        crate::dex::raydium::RaydiumPoolParser::get_program_id(),
        crate::dex::raydium::RaydiumPoolParser::parse_pool_data as PoolParseFn,
    );
    m.insert(
        crate::dex::whirlpool::WhirlpoolPoolParser::get_program_id(),
        crate::dex::whirlpool::WhirlpoolPoolParser::parse_pool_data as PoolParseFn,
    );
    m.insert(
        crate::dex::lifinity::LifinityPoolParser::get_program_id(),
        crate::dex::lifinity::LifinityPoolParser::parse_pool_data as PoolParseFn,
    );
    m
});

pub fn get_pool_parser_fn_for_program(program_id: &Pubkey) -> Option<PoolParseFn> {
    POOL_PARSER_REGISTRY.get(program_id).copied()
}

/// Returns spot price for a PoolInfo (A/B token price).
///
/// This function calculates the simple spot price ratio between token A and token B
/// in a liquidity pool. It's kept public for several important use cases:
///
/// - Testing: Verifying pool state and price calculations in unit/integration tests
/// - Analytics: Used in opportunity analysis and market monitoring modules
/// - UI/Reporting: Will be used in dashboard and reporting features
/// - Debugging: Helpful for diagnostics when troubleshooting arbitrage paths
///
/// Note: For actual swap execution, use calculate_output_amount() which accounts
/// for fees and slippage.
#[allow(dead_code)]
pub fn calculate_price(pool: &PoolInfo) -> f64 {
    let token_a_amount = pool.token_a.reserve as f64 / 10f64.powi(pool.token_a.decimals as i32);
    let token_b_amount = pool.token_b.reserve as f64 / 10f64.powi(pool.token_b.decimals as i32);

    token_a_amount / token_b_amount
}

// Calculate how much token_b you get for a given amount of token_a
pub fn calculate_output_amount(
    pool: &PoolInfo,
    input_amount: TokenAmount,
    is_a_to_b: bool,
) -> TokenAmount {
    let (input_reserve, _input_decimals, output_reserve, output_decimals) = if is_a_to_b {
        (
            pool.token_a.reserve,
            pool.token_a.decimals,
            pool.token_b.reserve,
            pool.token_b.decimals,
        )
    } else {
        (
            pool.token_b.reserve,
            pool.token_b.decimals,
            pool.token_a.reserve,
            pool.token_a.decimals,
        )
    };

    let adjusted_input = input_amount.amount;

    // Apply fees
    let fee = adjusted_input * pool.fee_numerator / pool.fee_denominator;
    let input_with_fee = adjusted_input - fee;

    // Calculate output using constant product formula: x * y = k
    let numerator = input_with_fee * output_reserve;
    let denominator = input_reserve + input_with_fee;
    let output_amount = numerator / denominator;

    TokenAmount::new(output_amount, output_decimals)
}
