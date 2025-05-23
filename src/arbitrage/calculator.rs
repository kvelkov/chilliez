use crate::arbitrage::fee_manager::{get_gas_cost_for_dex, FeeManager, XYKSlippageModel};
use crate::utils::{PoolInfo, TokenAmount}; // Removed DexType, calculate_output_amount
use dashmap::DashMap;
use once_cell::sync::Lazy;
use solana_sdk::pubkey::Pubkey;

#[derive(Debug, Clone)]
pub struct OpportunityCalculationResult {
    pub input_amount: f64,
    pub output_amount: f64,
    pub profit: f64,
    pub profit_percentage: f64,
    pub price_impact: f64,
}

static CALCULATION_CACHE: Lazy<DashMap<(Pubkey, Pubkey, u64, bool), OpportunityCalculationResult>> =
    Lazy::new(|| DashMap::new());

static OPTIMAL_INPUT_CACHE: Lazy<DashMap<(Pubkey, Pubkey, bool, u64), TokenAmount>> =
    Lazy::new(|| DashMap::new());

static MULTI_HOP_CACHE: Lazy<DashMap<String, (f64, f64, f64)>> = Lazy::new(|| DashMap::new());

#[allow(dead_code)]
pub fn calculate_opportunity(_pair: &(Pubkey, Pubkey)) -> OpportunityCalculationResult {
    OpportunityCalculationResult {
        input_amount: 1000.0,
        output_amount: 1010.0,
        profit: 10.0,
        profit_percentage: 0.01,
        price_impact: 0.005,
    }
}

pub fn calculate_optimal_input(
    pool_a: &PoolInfo,
    pool_b: &PoolInfo,
    is_a_to_b: bool,
    max_input_amount: TokenAmount,
) -> TokenAmount {
    let cache_key = (
        pool_a.address,
        pool_b.address,
        is_a_to_b,
        max_input_amount.amount,
    );
    if let Some(cached_result) = OPTIMAL_INPUT_CACHE.get(&cache_key) {
        return cached_result.clone();
    }

    let (a_in_reserve, a_in_decimals, b_out_reserve, _) = if is_a_to_b {
        (
            pool_a.token_a.reserve,
            pool_a.token_a.decimals,
            pool_a.token_b.reserve,
            pool_a.token_b.decimals,
        )
    } else {
        (
            pool_a.token_b.reserve,
            pool_a.token_b.decimals,
            pool_a.token_a.reserve,
            pool_a.token_a.decimals,
        )
    };

    let (b_in_reserve, _b_in_decimals, a_out_reserve, _a_out_decimals) = if is_a_to_b {
        (
            pool_b.token_b.reserve,
            pool_b.token_b.decimals,
            pool_b.token_a.reserve,
            pool_b.token_a.decimals,
        )
    } else {
        (
            pool_b.token_a.reserve,
            pool_b.token_a.decimals,
            pool_b.token_b.reserve,
            pool_b.token_b.decimals,
        )
    };
    
    let price_a = if a_in_reserve > 0 { b_out_reserve as f64 / a_in_reserve as f64 } else { 0.0 };
    let price_b = if b_in_reserve > 0 { a_out_reserve as f64 / b_in_reserve as f64 } else { 0.0 };

    let optimal_percentage = if price_b * price_a > 1.01 {
        0.75
    } else if price_b * price_a > 1.005 {
        0.5
    } else {
        0.25
    };

    let amount = (max_input_amount.amount as f64 * optimal_percentage) as u64;
    let result = TokenAmount::new(amount, a_in_decimals);

    OPTIMAL_INPUT_CACHE.insert(cache_key, result.clone());
    result
}

pub fn calculate_max_profit(
    pool_a: &PoolInfo,
    pool_b: &PoolInfo,
    is_a_to_b: bool,
    input_amount: TokenAmount,
) -> f64 {
    let timestamp = pool_a.last_update_timestamp.max(pool_b.last_update_timestamp);
    let cache_key = (pool_a.address, pool_b.address, timestamp, is_a_to_b);

    if let Some(cached_result) = CALCULATION_CACHE.get(&cache_key) {
        return cached_result.profit_percentage;
    }

    let get_fee = |pool: &PoolInfo| pool.fee_numerator as f64 / pool.fee_denominator as f64;

    let (a_in_reserve, a_fee, b_out_reserve, _) = if is_a_to_b {
        (pool_a.token_a.reserve, get_fee(pool_a), pool_a.token_b.reserve, pool_a.token_b.decimals)
    } else {
        (pool_a.token_b.reserve, get_fee(pool_a), pool_a.token_a.reserve, pool_a.token_a.decimals)
    };

    let (b_in_reserve, b_fee, a_out_reserve, _) = if is_a_to_b {
        (pool_b.token_b.reserve, get_fee(pool_b), pool_b.token_a.reserve, pool_b.token_a.decimals)
    } else {
        (pool_b.token_a.reserve, get_fee(pool_b), pool_b.token_b.reserve, pool_b.token_b.decimals)
    };

    let input_with_fee = input_amount.amount as f64 * (1.0 - a_fee);
    let numerator = if a_in_reserve as f64 + input_with_fee != 0.0 {
        input_with_fee * b_out_reserve as f64 / (a_in_reserve as f64 + input_with_fee)
    } else { 0.0 };
    let b_amount = numerator;

    let b_input_with_fee = b_amount * (1.0 - b_fee);
    let numerator2 = if b_in_reserve as f64 + b_input_with_fee != 0.0 {
        b_input_with_fee * a_out_reserve as f64 / (b_in_reserve as f64 + b_input_with_fee)
    } else { 0.0 };
    let a_final_amount = numerator2;

    let profit = a_final_amount - input_amount.amount as f64;
    let profit_percentage = if input_amount.amount > 0 { profit / input_amount.amount as f64 } else { 0.0 };

    let price_impact = estimate_price_impact(pool_a, if is_a_to_b { 0 } else { 1 }, input_amount.clone());

    let result = OpportunityCalculationResult {
        input_amount: input_amount.amount as f64,
        output_amount: a_final_amount,
        profit,
        profit_percentage,
        price_impact,
    };
    CALCULATION_CACHE.insert(cache_key, result);
    profit_percentage.max(0.0)
}

pub fn calculate_transaction_cost(_transaction_size: usize, _priority_fee: u64) -> f64 {
    const BASE_FEE_LAMPORTS: f64 = 5000.0;
    const LAMPORTS_PER_SIGNATURE: f64 = 5000.0;
    const SIGNATURES_PER_TX: usize = 2;
    // const COMPUTE_UNITS_PER_TX: u64 = 200_000; // Priority fee calculation needs actual CUs
    const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

    let signature_cost = SIGNATURES_PER_TX as f64 * LAMPORTS_PER_SIGNATURE;
    // Priority fee needs to be calculated based on actual compute units if applied per CU.
    // If priority_fee is a flat addition in lamports, then it's simpler.
    // For now, assuming priority_fee is a flat addition and not per CU.
    let priority_cost_lamports = _priority_fee as f64; // If priority_fee is already in lamports

    let total_lamports = BASE_FEE_LAMPORTS + signature_cost + priority_cost_lamports;
    total_lamports / LAMPORTS_PER_SOL
}

pub fn is_profitable(
    profit_in_token: f64,
    token_price_in_sol: f64,
    transaction_cost_in_sol: f64,
    min_profit_threshold_sol: f64, // Assuming this threshold is in SOL
) -> bool {
    let profit_in_sol = profit_in_token * token_price_in_sol;
    let net_profit_sol = profit_in_sol - transaction_cost_in_sol;
    net_profit_sol > min_profit_threshold_sol
}

pub fn estimate_price_impact(
    pool: &PoolInfo,
    input_token_index: usize, // 0 for token_a, 1 for token_b
    input_amount: TokenAmount,
) -> f64 {
    let input_reserve_float = if input_token_index == 0 {
        pool.token_a.reserve as f64
    } else {
        pool.token_b.reserve as f64
    };
    let input_amount_float = input_amount.to_float(); // Use .to_float()

    if input_reserve_float == 0.0 && input_amount_float == 0.0 { return 0.0; } // No impact if no reserves and no input
    if input_reserve_float + input_amount_float == 0.0 { return 1.0; } // Max impact if sum is zero (e.g. negative reserves - though unlikely)


    input_amount_float / (input_reserve_float + input_amount_float)
}


// This function was in utils.rs as well. Consolidating or ensuring one is canonical.
// Assuming this is the primary one for multi-hop calculations context.
// The error was for line 379, inside calculate_multihop_profit_and_slippage.
// The error "cannot move out of type `[&PoolInfo]`, a non-copy slice"
// at line 379: let _gas_cost = get_gas_cost_for_dex(pools[0].dex_type);
// Is strange. `pools` is `&[&PoolInfo]`. `pools[0]` is `&PoolInfo`. `pools[0].dex_type` is `DexType`.
// This should not try to move `pools`. The issue is with `dex_type`.
pub fn calculate_multihop_profit_and_slippage(
    pools: &[&PoolInfo],
    input_amount: f64,
    directions: &[bool],
    last_fee_data: &[(Option<u64>, Option<u64>, Option<u64>)],
) -> (f64, f64, f64) {
    let cache_key = {
        let mut key = String::new();
        for (i, pool) in pools.iter().enumerate() {
            key.push_str(&pool.address.to_string());
            key.push_str(if directions.get(i).copied().unwrap_or(true) { "t" } else { "f" });
        }
        key.push_str(&format!("_{}", input_amount)); // Consider if input_amount precision matters for cache
        key
    };

    if let Some(cached_result) = MULTI_HOP_CACHE.get(&cache_key) {
        return *cached_result;
    }

    let mut amounts_for_fee_est = Vec::new();
    let mut current_simulated_amount = input_amount;

    // Simulate hop outputs to get input amounts for fee estimation per hop
    for (idx, pool_ref) in pools.iter().enumerate() {
        let pool = *pool_ref; // Dereference to PoolInfo
        let input_decimals = if directions[idx] { pool.token_a.decimals } else { pool.token_b.decimals };
        let current_input_token_amount = TokenAmount::new(
            (current_simulated_amount * 10f64.powi(input_decimals as i32)) as u64,
            input_decimals
        );
        amounts_for_fee_est.push(current_input_token_amount.clone());

        // Simulate output of this hop to be input for next (simplified, real calc would use exact output)
        // This is a very rough estimation for subsequent hop input amounts for fee/slippage estimation
        let fee_fraction = pool.fee_numerator as f64 / pool.fee_denominator as f64;
        current_simulated_amount = current_simulated_amount * (1.0 - fee_fraction) * 0.99; // Assume 1% slippage + fee for next hop input rough est.
    }
    // Ensure amounts_for_fee_est has the correct length
     while amounts_for_fee_est.len() < pools.len() { // Pad if simulation was too short
        if let Some(last_pool) = pools.last() {
             amounts_for_fee_est.push(TokenAmount::new(0, last_pool.token_a.decimals)); // Dummy amount
        } else {
            break; // No pools to get decimals from
        }
    }


    let slippage_model = XYKSlippageModel;
    let fee_breakdown = FeeManager::estimate_multi_hop_with_model(
        pools,
        &amounts_for_fee_est, // Use simulated input amounts for each hop
        directions,
        last_fee_data,
        &slippage_model,
    );

    // The _gas_cost error was here. `get_gas_cost_for_dex` takes DexType by value.
    // `pools[0].dex_type` requires cloning because DexType is not Copy.
    if pools.is_empty() { // Guard against empty pools slice
        MULTI_HOP_CACHE.insert(cache_key, (0.0, 1.0, 0.0)); // Profit, Slippage, Fee
        return (0.0, 1.0, 0.0);
    }
    let _gas_cost = get_gas_cost_for_dex(pools[0].dex_type.clone()); // Fixed: clone DexType

    let _fee_in_usdc = FeeManager::convert_fee_to_reference_token(
        fee_breakdown.expected_fee,
        &pools[0].token_a.symbol,
        "USDC",
    );

    // This calculation of total_profit is a placeholder.
    // A real calculation would simulate the swaps step-by-step with actual output amounts.
    // The `fee_breakdown.expected_fee` is also a sum of fees in potentially different tokens.
    // For accurate profit, one must track the token amounts through each hop precisely.
    let placeholder_final_amount = input_amount * (1.0 - fee_breakdown.expected_slippage) - fee_breakdown.expected_fee; // Highly simplified
    let total_profit = placeholder_final_amount - input_amount;

    let result = (
        total_profit,
        fee_breakdown.expected_slippage,
        fee_breakdown.expected_fee,
    );

    MULTI_HOP_CACHE.insert(cache_key, result);
    result
}

pub fn calculate_rebate(_pools: &[&PoolInfo], _amounts: &[TokenAmount]) -> f64 {
    0.0
}

pub fn clear_caches_if_needed() {
    const MAX_CACHE_SIZE: usize = 10000;
    if CALCULATION_CACHE.len() > MAX_CACHE_SIZE { CALCULATION_CACHE.clear(); }
    if OPTIMAL_INPUT_CACHE.len() > MAX_CACHE_SIZE { OPTIMAL_INPUT_CACHE.clear(); }
    if MULTI_HOP_CACHE.len() > MAX_CACHE_SIZE { MULTI_HOP_CACHE.clear(); }
}