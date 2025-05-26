// src/arbitrage/calculator.rs
use crate::arbitrage::fee_manager::{FeeManager, XYKSlippageModel};
use crate::utils::{PoolInfo, TokenAmount};
use dashmap::DashMap;
use once_cell::sync::Lazy;
use solana_sdk::pubkey::Pubkey;
use log::{debug, warn, info}; // Ensured info is imported

#[derive(Debug, Clone)]
pub struct OpportunityCalculationResult {
    pub input_amount: f64,    
    pub output_amount: f64,   
    pub profit: f64,          
    pub profit_percentage: f64, 
    pub price_impact: f64,    
}

static CALCULATION_CACHE: Lazy<DashMap<(Pubkey, Pubkey, u64, bool), OpportunityCalculationResult>> =
    Lazy::new(DashMap::new);

static OPTIMAL_INPUT_CACHE: Lazy<DashMap<(Pubkey, Pubkey, bool, u64), TokenAmount>> =
    Lazy::new(DashMap::new);

static MULTI_HOP_CACHE: Lazy<DashMap<String, (f64, f64, f64)>> = Lazy::new(DashMap::new);

#[allow(dead_code)] 
pub fn calculate_simple_opportunity_result(
    pool_a: &PoolInfo,
    pool_b: &PoolInfo,
    input_token_mint_for_pool_a: &Pubkey, 
    input_amount_tokens: f64 
) -> OpportunityCalculationResult {
    let (pool_a_input_reserve, pool_a_output_reserve, pool_a_input_decimals, pool_a_output_decimals, pool_a_fee_num, pool_a_fee_den, pool_a_intermediate_mint) =
        if pool_a.token_a.mint == *input_token_mint_for_pool_a {
            (pool_a.token_a.reserve, pool_a.token_b.reserve, pool_a.token_a.decimals, pool_a.token_b.decimals, pool_a.fee_numerator, pool_a.fee_denominator, pool_a.token_b.mint)
        } else if pool_a.token_b.mint == *input_token_mint_for_pool_a {
            (pool_a.token_b.reserve, pool_a.token_a.reserve, pool_a.token_b.decimals, pool_a.token_a.decimals, pool_a.fee_numerator, pool_a.fee_denominator, pool_a.token_a.mint)
        } else {
            warn!("Input token {} not found in pool_a {}", input_token_mint_for_pool_a, pool_a.name);
            return OpportunityCalculationResult { input_amount: input_amount_tokens, output_amount: 0.0, profit: -input_amount_tokens, profit_percentage: -1.0, price_impact: 1.0 };
        };

    let input_amount_atomic_pool_a = TokenAmount::from_float(input_amount_tokens, pool_a_input_decimals).amount;

    let fee_a_rate = pool_a_fee_num as f64 / pool_a_fee_den.max(1) as f64;
    let input_a_after_fee = input_amount_atomic_pool_a as f64 * (1.0 - fee_a_rate);
    let output_a_atomic_intermediate = if pool_a_input_reserve > 0 && (pool_a_input_reserve as f64 + input_a_after_fee) > 0.0 {
        (pool_a_output_reserve as f64 * input_a_after_fee) / (pool_a_input_reserve as f64 + input_a_after_fee)
    } else {
        0.0
    };
    let intermediate_token_amount = TokenAmount::new(output_a_atomic_intermediate.floor() as u64, pool_a_output_decimals);
    
    let (pool_b_input_reserve, pool_b_output_reserve, _pool_b_input_decimals, pool_b_output_decimals, pool_b_fee_num, pool_b_fee_den) =
        if pool_b.token_a.mint == pool_a_intermediate_mint && pool_b.token_b.mint == *input_token_mint_for_pool_a {
            (pool_b.token_a.reserve, pool_b.token_b.reserve, pool_b.token_a.decimals, pool_b.token_b.decimals, pool_b.fee_numerator, pool_b.fee_denominator)
        } else if pool_b.token_b.mint == pool_a_intermediate_mint && pool_b.token_a.mint == *input_token_mint_for_pool_a {
            (pool_b.token_b.reserve, pool_b.token_a.reserve, pool_b.token_b.decimals, pool_b.token_a.decimals, pool_b.fee_numerator, pool_b.fee_denominator)
        } else {
            warn!("Pool_b {} does not complete the cycle for intermediate {} to original input {}", pool_b.name, pool_a_intermediate_mint, input_token_mint_for_pool_a);
            return OpportunityCalculationResult { input_amount: input_amount_tokens, output_amount: 0.0, profit: -input_amount_tokens, profit_percentage: -1.0, price_impact: 1.0 };
        };

    let fee_b_rate = pool_b_fee_num as f64 / pool_b_fee_den.max(1) as f64;
    let input_b_after_fee = intermediate_token_amount.amount as f64 * (1.0 - fee_b_rate);
    let final_output_atomic = if pool_b_input_reserve > 0 && (pool_b_input_reserve as f64 + input_b_after_fee) > 0.0 {
        (pool_b_output_reserve as f64 * input_b_after_fee) / (pool_b_input_reserve as f64 + input_b_after_fee)
    } else {
        0.0
    };
    
    let final_output_tokens = TokenAmount::new(final_output_atomic.floor() as u64, pool_b_output_decimals).to_float();
    let profit_tokens = final_output_tokens - input_amount_tokens;
    let profit_percentage = if input_amount_tokens > 0.0 { profit_tokens / input_amount_tokens } else { 0.0 };

    let price_impact_a = if (pool_a_input_reserve as f64 + input_a_after_fee) > 1e-9 { input_a_after_fee / (pool_a_input_reserve as f64 + input_a_after_fee) } else { 1.0 };
    let price_impact_b = if (pool_b_input_reserve as f64 + input_b_after_fee) > 1e-9 { input_b_after_fee / (pool_b_input_reserve as f64 + input_b_after_fee) } else { 1.0 };
    
    OpportunityCalculationResult {
        input_amount: input_amount_tokens,
        output_amount: final_output_tokens,
        profit: profit_tokens,
        profit_percentage,
        price_impact: 1.0 - ((1.0 - price_impact_a) * (1.0 - price_impact_b)),
    }
}

pub fn calculate_optimal_input(
    pool_a: &PoolInfo,
    pool_b: &PoolInfo,
    is_a_to_b_first_hop: bool, 
    max_input_amount: TokenAmount,
) -> TokenAmount {
    let cache_key = (
        pool_a.address,
        pool_b.address,
        is_a_to_b_first_hop,
        max_input_amount.amount,
    );
    if let Some(cached_result) = OPTIMAL_INPUT_CACHE.get(&cache_key) {
        return cached_result.clone();
    }
    
    let input_token_decimals_pool_a = if is_a_to_b_first_hop { pool_a.token_a.decimals } else { pool_a.token_b.decimals };

    let test_fractions = [0.001, 0.005, 0.01, 0.02, 0.05, 0.1]; 
    let mut best_input_atomic = TokenAmount::new(0, input_token_decimals_pool_a);
    let mut max_profit_pct = -1.0; 

    for fraction in test_fractions.iter() {
        let current_test_input_atomic_amount = (max_input_amount.amount as f64 * fraction).floor() as u64;
        if current_test_input_atomic_amount == 0 { continue; }
        
        let current_test_input = TokenAmount::new(current_test_input_atomic_amount, input_token_decimals_pool_a);
        let result = calculate_max_profit_result(pool_a, pool_b, is_a_to_b_first_hop, current_test_input.clone());

        if result.profit_percentage > max_profit_pct {
            max_profit_pct = result.profit_percentage;
            best_input_atomic = current_test_input;
        }
    }
    
    let final_optimal_input = if max_profit_pct > 0.0 {
        best_input_atomic
    } else {
        TokenAmount::new((max_input_amount.amount as f64 * 0.001).floor() as u64, input_token_decimals_pool_a) 
    };

    OPTIMAL_INPUT_CACHE.insert(cache_key, final_optimal_input.clone());
    debug!("Calculated optimal input: {:?} for pools {} ({} -> B) and {} (B -> A)", final_optimal_input, pool_a.name, if is_a_to_b_first_hop {"A"} else {"B"}, pool_b.name);
    final_optimal_input
}

pub fn calculate_max_profit_result(
    pool_a: &PoolInfo,
    pool_b: &PoolInfo,
    is_a_to_b_first_hop: bool, 
    input_amount: TokenAmount, 
) -> OpportunityCalculationResult {
    let full_cache_key = (pool_a.address, pool_b.address, input_amount.amount, is_a_to_b_first_hop);
    if let Some(entry) = CALCULATION_CACHE.get(&full_cache_key) {
        return entry.value().clone();
    }
    
    let get_fee_rate = |pool: &PoolInfo| {
        if pool.fee_denominator == 0 { 0.0 } else { pool.fee_numerator as f64 / pool.fee_denominator as f64 }
    };

    let (p_a_input_token_info, p_a_output_token_info, p_a_fee_rate) = if is_a_to_b_first_hop {
        (&pool_a.token_a, &pool_a.token_b, get_fee_rate(pool_a))
    } else {
        (&pool_a.token_b, &pool_a.token_a, get_fee_rate(pool_a))
    };

    if input_amount.decimals != p_a_input_token_info.decimals {
        warn!("Decimal mismatch for input_amount in calculate_max_profit_result. Expected {}, got {}. Pool A: {}, Input Token: {}", p_a_input_token_info.decimals, input_amount.decimals, pool_a.name, p_a_input_token_info.symbol);
    }

    let input_amount_float = input_amount.to_float();
    let p_a_input_atomic_after_fee = input_amount.amount as f64 * (1.0 - p_a_fee_rate);

    let p_a_output_atomic = if p_a_input_token_info.reserve > 0 && (p_a_input_token_info.reserve as f64 + p_a_input_atomic_after_fee) > 1e-9 {
        (p_a_output_token_info.reserve as f64 * p_a_input_atomic_after_fee) / (p_a_input_token_info.reserve as f64 + p_a_input_atomic_after_fee)
    } else {
        0.0
    };
    let intermediate_amount = TokenAmount::new(p_a_output_atomic.floor() as u64, p_a_output_token_info.decimals);
    
    // Mint of the token we expect to get back to complete the cycle
    let original_input_token_mint = p_a_input_token_info.mint; 

    let (p_b_input_token_info, p_b_output_token_info, p_b_fee_rate) = 
        // pool_b trades intermediate_token (p_a_output_token_info.mint) for original_input_token
        if pool_b.token_a.mint == p_a_output_token_info.mint && pool_b.token_b.mint == original_input_token_mint {
            (&pool_b.token_a, &pool_b.token_b, get_fee_rate(pool_b))
        } else if pool_b.token_b.mint == p_a_output_token_info.mint && pool_b.token_a.mint == original_input_token_mint {
            (&pool_b.token_b, &pool_b.token_a, get_fee_rate(pool_b))
        } else {
            warn!("Pool B ({}) does not form a cycle with Pool A ({}). Intermediate: {} ({}), Expected Final: {} ({}). PoolB tokens: A={}, B={}", 
                  pool_b.name, pool_a.name, 
                  p_a_output_token_info.symbol, p_a_output_token_info.mint, 
                  p_a_input_token_info.symbol, original_input_token_mint,
                  pool_b.token_a.mint, pool_b.token_b.mint);
            let result = OpportunityCalculationResult { input_amount: input_amount_float, output_amount: 0.0, profit: -input_amount_float, profit_percentage: -1.0, price_impact: 1.0 };
            CALCULATION_CACHE.insert(full_cache_key, result.clone());
            return result;
        };

    if intermediate_amount.decimals != p_b_input_token_info.decimals {
        warn!("Decimal mismatch for intermediate_amount (input to PoolB). Expected {}, got {}. Pool B: {}, Input Token to B: {}", p_b_input_token_info.decimals, intermediate_amount.decimals, pool_b.name, p_b_input_token_info.symbol);
    }

    let p_b_input_atomic_after_fee = intermediate_amount.amount as f64 * (1.0 - p_b_fee_rate);
    let final_output_atomic = if p_b_input_token_info.reserve > 0 && (p_b_input_token_info.reserve as f64 + p_b_input_atomic_after_fee) > 1e-9 {
        (p_b_output_token_info.reserve as f64 * p_b_input_atomic_after_fee) / (p_b_input_token_info.reserve as f64 + p_b_input_atomic_after_fee)
    } else {
        0.0
    };
    
    // Final output should have decimals of original input token
    if p_b_output_token_info.decimals != p_a_input_token_info.decimals {
         warn!("Decimal mismatch for final output. Original input decimals: {}, Pool B output decimals: {}. This indicates a non-cyclic path or error.", p_a_input_token_info.decimals, p_b_output_token_info.decimals);
    }
    let final_output_tokens = TokenAmount::new(final_output_atomic.floor() as u64, p_b_output_token_info.decimals).to_float();

    let profit_val = final_output_tokens - input_amount_float;
    let profit_percentage_val = if input_amount_float > 1e-9 { profit_val / input_amount_float } else { 0.0 };

    let price_impact_a = if (p_a_input_token_info.reserve as f64 + p_a_input_atomic_after_fee) > 1e-9 {
        p_a_input_atomic_after_fee / (p_a_input_token_info.reserve as f64 + p_a_input_atomic_after_fee)
    } else { 1.0 };
    let price_impact_b = if (p_b_input_token_info.reserve as f64 + p_b_input_atomic_after_fee) > 1e-9 {
        p_b_input_atomic_after_fee / (p_b_input_token_info.reserve as f64 + p_b_input_atomic_after_fee)
    } else { 1.0 };
    
    let result = OpportunityCalculationResult {
        input_amount: input_amount_float,
        output_amount: final_output_tokens,
        profit: profit_val,
        profit_percentage: profit_percentage_val,
        price_impact: 1.0 - ((1.0-price_impact_a.max(0.0).min(1.0)) * (1.0-price_impact_b.max(0.0).min(1.0))), // Ensure impact is 0-1
    };
    
    CALCULATION_CACHE.insert(full_cache_key, result.clone());
    debug!("Calculated profit result: {:?} for input {:?} (PoolA: {}, PoolB: {}, A->B in PoolA: {})", result, input_amount, pool_a.name, pool_b.name, is_a_to_b_first_hop);
    result
}

pub fn calculate_max_profit(
    pool_a: &PoolInfo,
    pool_b: &PoolInfo,
    is_a_to_b: bool,
    input_amount: TokenAmount,
) -> f64 {
    calculate_max_profit_result(pool_a, pool_b, is_a_to_b, input_amount).profit_percentage.max(0.0)
}

pub fn calculate_transaction_cost(
    transaction_instructions_count: usize, 
    priority_fee_lamports: u64, 
    sol_price_usd: f64
) -> f64 {
    const BASE_FEE_LAMPORTS: u64 = 5000; 
    const LAMPORTS_PER_SIGNATURE: u64 = 5000;
    let signatures_count: u64 = if transaction_instructions_count > 0 { 1 } else { 0 } + 1; 
    const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

    let signature_cost_lamports = signatures_count * LAMPORTS_PER_SIGNATURE;
    let total_lamports = BASE_FEE_LAMPORTS + signature_cost_lamports + priority_fee_lamports;
    let total_sol = total_lamports as f64 / LAMPORTS_PER_SOL;
    let cost_usd = total_sol * sol_price_usd;
    debug!("Calculated tx cost: {} lamports = {} SOL = ${:.6} USD (instr_count: {}, priority_fee: {})", total_lamports, total_sol, cost_usd, transaction_instructions_count, priority_fee_lamports);
    cost_usd
}

pub fn is_profitable_calc( // Renamed from original `is_profitable`
    opportunity_result: &OpportunityCalculationResult,
    input_token_price_usd: f64, 
    transaction_cost_usd: f64,  
    min_profit_threshold_usd: f64, 
) -> bool {
    let profit_in_input_token_units = opportunity_result.profit;
    let profit_in_usd = profit_in_input_token_units * input_token_price_usd;
    
    let net_profit_usd = profit_in_usd - transaction_cost_usd;
    debug!(
        "Profitability check: OppInput: {:.6}, OppOutput: {:.6}, OppProfitTokens: {:.6}, OppProfitPct: {:.4}%, InputTokenPriceUSD: {:.2}, TxCostUSD: {:.4}, EstProfitUSD: {:.4}, NetProfitUSD: {:.4}, MinThresholdUSD: {:.4}",
        opportunity_result.input_amount,
        opportunity_result.output_amount,
        opportunity_result.profit,
        opportunity_result.profit_percentage * 100.0,
        input_token_price_usd,
        transaction_cost_usd,
        profit_in_usd,
        net_profit_usd,
        min_profit_threshold_usd
    );
    net_profit_usd > min_profit_threshold_usd
}

pub fn estimate_price_impact(
    pool: &PoolInfo,
    input_token_index: usize, 
    input_amount: TokenAmount,
) -> f64 {
    let (input_reserve_atomic, _input_decimals) = if input_token_index == 0 {
        (pool.token_a.reserve, pool.token_a.decimals)
    } else {
        (pool.token_b.reserve, pool.token_b.decimals)
    };
    let input_amount_atomic = input_amount.amount;

    let input_reserve_float = input_reserve_atomic as f64;
    let input_amount_float = input_amount_atomic as f64;

    if input_reserve_float < 1e-9 && input_amount_float < 1e-9 { return 0.0; } // Avoid NaN with 0/0
    let denominator = input_reserve_float + input_amount_float;
    if denominator < 1e-9 { return 1.0; } 

    (input_amount_float / denominator).max(0.0).min(1.0) // Clamp result
}

pub fn calculate_multihop_profit_and_slippage(
    pools: &[&PoolInfo], 
    input_amount_float: f64, 
    start_token_decimals: u8, 
    directions: &[bool], 
    last_fee_data: &[(Option<u64>, Option<u64>, Option<u64>)], 
) -> (f64, f64, f64) {
    let cache_key_parts: Vec<String> = pools.iter().map(|p| p.address.to_string())
                                        .chain(directions.iter().map(|d| d.to_string()))
                                        .chain(std::iter::once(input_amount_float.to_string()))
                                        .chain(std::iter::once(start_token_decimals.to_string()))
                                        .collect();
    let cache_key = cache_key_parts.join(":");

    if let Some(cached_result) = MULTI_HOP_CACHE.get(&cache_key) {
        return *cached_result;
    }

    if pools.is_empty() || directions.is_empty() || pools.len() != directions.len() || (last_fee_data.len() != pools.len() && !last_fee_data.is_empty()) {
        warn!("calculate_multihop_profit_and_slippage called with empty or mismatched inputs. Pools: {}, Directions: {}, Fees: {}", pools.len(), directions.len(), last_fee_data.len());
        // Provide a default value for last_fee_data if it's empty and pools is not, to avoid panic in FeeManager
        let default_last_fee_data_if_needed;
        let fees_to_use = if last_fee_data.is_empty() && !pools.is_empty() {
            default_last_fee_data_if_needed = vec![(None, None, None); pools.len()];
            &default_last_fee_data_if_needed
        } else if last_fee_data.len() != pools.len() {
             warn!("Correcting last_fee_data length mismatch for multi-hop calculation.");
             default_last_fee_data_if_needed = vec![(None, None, None); pools.len()];
            &default_last_fee_data_if_needed
        }
         else {
            last_fee_data
        };


         if pools.is_empty() { // Still handle if pools itself is empty
            MULTI_HOP_CACHE.insert(cache_key.clone(), (0.0, 1.0, 0.0));
            return (0.0, 1.0, 0.0);
         }
          // Proceed with fees_to_use for FeeManager call if pools is not empty
    }
    let fees_to_use = if last_fee_data.len() != pools.len() {
        warn!("Correcting last_fee_data length mismatch for multi-hop calculation. Pools: {}, Fees: {}", pools.len(), last_fee_data.len());
        vec![(None, None, None); pools.len()] // Create a default vec of the correct length
    } else {
        last_fee_data.to_vec() // Clone if lengths match
    };


    let mut current_amount_atomic = TokenAmount::from_float(input_amount_float, start_token_decimals);
    let mut _current_token_mint = if directions[0] { pools[0].token_a.mint } else { pools[0].token_b.mint }; 

    let mut total_slippage_fraction_product = 1.0;
    let mut amounts_for_fee_est = Vec::new();

    for (idx, &pool_ref) in pools.iter().enumerate() {
        let pool = pool_ref;
        amounts_for_fee_est.push(current_amount_atomic.clone());

        let is_a_to_b_this_hop = directions[idx];
        
        let (input_token_this_hop_mint, input_token_this_hop_decimals, input_reserve_atomic_this_hop) = if is_a_to_b_this_hop {
            (pool.token_a.mint, pool.token_a.decimals, pool.token_a.reserve)
        } else {
            (pool.token_b.mint, pool.token_b.decimals, pool.token_b.reserve)
        };
        
        // This check was problematic as _current_token_mint might not be initialized if pools is empty (handled above)
        // Also, decimals of current_amount_atomic should align with input_token_this_hop_decimals
        if current_amount_atomic.decimals != input_token_this_hop_decimals {
             warn!("Decimal mismatch for hop {}: current_amount_atomic.decimals ({}) != input_token_this_hop_decimals ({} for token {} in pool {}). Auto-adjusting for calculation.",
                  idx, current_amount_atomic.decimals, input_token_this_hop_decimals, input_token_this_hop_mint, pool.name);
             current_amount_atomic = TokenAmount::from_float(current_amount_atomic.to_float(), input_token_this_hop_decimals);
        }
        
        let input_amount_this_hop_float = current_amount_atomic.to_float(); 
        let input_reserve_this_hop_float = TokenAmount::new(input_reserve_atomic_this_hop, input_token_this_hop_decimals).to_float();

        let hop_slippage = if (input_reserve_this_hop_float + input_amount_this_hop_float) > 1e-9 { 
            input_amount_this_hop_float / (input_reserve_this_hop_float + input_amount_this_hop_float)
        } else { 
            1.0 
        };
        total_slippage_fraction_product *= 1.0 - hop_slippage.max(0.0).min(1.0); 

        current_amount_atomic = crate::utils::calculate_output_amount(pool, current_amount_atomic, is_a_to_b_this_hop);
        _current_token_mint = if is_a_to_b_this_hop { pool.token_b.mint } else { pool.token_a.mint };
    }
    
    while amounts_for_fee_est.len() < pools.len() { // Should not be needed if logic above is correct
        if let Some(last_pool) = pools.last() {
             amounts_for_fee_est.push(TokenAmount::new(0, if directions.last().copied().unwrap_or(true) { last_pool.token_a.decimals } else { last_pool.token_b.decimals } ));
        } else { break; }
    }

    let slippage_model = XYKSlippageModel;
    let fee_breakdown = FeeManager::estimate_multi_hop_with_model(
        pools, &amounts_for_fee_est, directions, &fees_to_use, &slippage_model, // Use corrected fees_to_use
    );

    let final_output_float = current_amount_atomic.to_float(); 
    let total_profit_float = final_output_float - input_amount_float; 
    let overall_slippage_fraction = 1.0 - total_slippage_fraction_product;
    
    let total_fee_in_start_token_equivalent_placeholder = fee_breakdown.expected_fee; 

    let result = (
        total_profit_float,
        overall_slippage_fraction.max(0.0).min(1.0),
        total_fee_in_start_token_equivalent_placeholder,
    );

    MULTI_HOP_CACHE.insert(cache_key, result);
    debug!("Calculated multi-hop ({} hops) result: {:?} for input {} ({} decimals)", pools.len(), result, input_amount_float, start_token_decimals);
    result
}

pub fn calculate_rebate(_pools: &[&PoolInfo], _amounts: &[TokenAmount]) -> f64 {
    debug!("calculate_rebate called, returning 0.0 (placeholder)");
    0.0
}

pub fn clear_caches_if_needed() {
    const MAX_CACHE_SIZE: usize = 10000; 
    let mut cleared_any = false;
    if CALCULATION_CACHE.len() > MAX_CACHE_SIZE {
        CALCULATION_CACHE.clear();
        cleared_any = true;
        debug!("Cleared CALCULATION_CACHE (size exceeded {})", MAX_CACHE_SIZE);
    }
    if OPTIMAL_INPUT_CACHE.len() > MAX_CACHE_SIZE {
        OPTIMAL_INPUT_CACHE.clear();
        cleared_any = true;
        debug!("Cleared OPTIMAL_INPUT_CACHE (size exceeded {})", MAX_CACHE_SIZE);
    }
    if MULTI_HOP_CACHE.len() > MAX_CACHE_SIZE {
        MULTI_HOP_CACHE.clear();
        cleared_any = true;
        debug!("Cleared MULTI_HOP_CACHE (size exceeded {})", MAX_CACHE_SIZE);
    }
    if cleared_any {
        info!("Arbitrage calculator caches cleared due to size limits.");
    }
}