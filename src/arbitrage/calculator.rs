use crate::arbitrage::fee_manager::{FeeManager, XYKSlippageModel};
use crate::utils::{PoolInfo, TokenAmount};
use dashmap::DashMap;
use log::{debug, warn};
use once_cell::sync::Lazy;
use solana_sdk::pubkey::Pubkey;

#[derive(Debug, Clone)]

pub struct OpportunityCalculationResult {
    pub input_amount: f64,
    pub output_amount: f64,
    pub profit: f64,
    pub profit_percentage: f64,
}

static MULTI_HOP_CACHE: Lazy<DashMap<String, (f64, f64, f64)>> = Lazy::new(DashMap::new);

/// Calculates the transaction cost in USD.
pub fn calculate_transaction_cost(
    num_swaps: usize, // Renamed from num_hops for clarity if each hop is one swap; adjust if num_hops means something else.
    priority_fee_lamports_per_swap: u64,
    sol_price_usd: f64,
) -> f64 {
    // Assuming priority_fee is per swap/transaction instruction.
    // Total lamports = num_swaps * priority_fee_lamports_per_swap
    // Total SOL = Total lamports / 1_000_000_000
    // Total USD = Total SOL * sol_price_usd
    (num_swaps as f64 * priority_fee_lamports_per_swap as f64 * sol_price_usd) / 1_000_000_000.0
}

/// Determines if an opportunity is profitable after considering token price, transaction costs, and a minimum profit threshold in SOL.
pub fn is_profitable_calc(
    opp_result: &OpportunityCalculationResult,
    token_price_in_sol: f64, // Price of the profit token in terms of SOL
    tx_cost_in_sol: f64,       // Total transaction cost for the arbitrage, in SOL
    min_profit_threshold_sol: f64, // Minimum net profit required, in SOL
) -> bool {
    let gross_profit_in_sol = opp_result.profit * token_price_in_sol;
    let net_profit_in_sol = gross_profit_in_sol - tx_cost_in_sol;
    net_profit_in_sol > min_profit_threshold_sol
}

/// Calculates arbitrage profit and slippage across multi-hop routes
pub fn calculate_multihop_profit_and_slippage(
    pools: &[&PoolInfo],
    input_amount_float: f64,
    start_token_decimals: u8,
    directions: &[bool],
    last_fee_data: &[(Option<u64>, Option<u64>, Option<u64>)],
) -> (f64, f64, f64) {
    let cache_key = format!(
        "{}:{}:{}:{}",
        pools
            .iter()
            .map(|p| p.address.to_string())
            .collect::<Vec<_>>()
            .join(":"),
        directions
            .iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
            .join(":"),
        input_amount_float,
        start_token_decimals
    );

    if let Some(cached_result) = MULTI_HOP_CACHE.get(&cache_key) {
        return *cached_result;
    }

    if pools.is_empty() || directions.is_empty() || pools.len() != directions.len() {
        warn!(
            "Invalid multi-hop calculation input. Pools: {}, Directions: {}",
            pools.len(),
            directions.len()
        );
        return (0.0, 1.0, 0.0);
    }

    let mut current_amount_atomic =
        TokenAmount::from_float(input_amount_float, start_token_decimals);
    let mut total_slippage_fraction_product = 1.0;
    let mut amounts_for_fee_est: Vec<TokenAmount> = Vec::with_capacity(pools.len());

    for (idx, &pool) in pools.iter().enumerate() {
        let is_a_to_b = directions[idx];
        let input_reserve = if is_a_to_b {
            pool.token_a.reserve
        } else {
            pool.token_b.reserve
        };
        let input_decimals = if is_a_to_b {
            pool.token_a.decimals
        } else {
            pool.token_b.decimals
        };

        if current_amount_atomic.decimals != input_decimals {
            warn!(
                "Decimal mismatch at hop {}: expected {}, got {}. Auto-adjusting.",
                idx, input_decimals, current_amount_atomic.decimals
            );
            current_amount_atomic =
                TokenAmount::from_float(current_amount_atomic.to_float(), input_decimals);
        }

        amounts_for_fee_est.push(current_amount_atomic.clone());

        let input_amount_float = current_amount_atomic.to_float();
        let input_reserve_float = TokenAmount::new(input_reserve, input_decimals).to_float();

        let hop_slippage = if input_reserve_float + input_amount_float > 1e-9 {
            input_amount_float / (input_reserve_float + input_amount_float)
        } else {
            1.0
        };

        total_slippage_fraction_product *= 1.0 - hop_slippage.max(0.0).min(1.0);
        current_amount_atomic =
            crate::utils::calculate_output_amount(pool, current_amount_atomic, is_a_to_b);
    }

    let fee_breakdown = FeeManager::estimate_multi_hop_with_model(
        pools,
        &amounts_for_fee_est,
        directions,
        last_fee_data,
        &XYKSlippageModel,
    );



    let final_output_float = current_amount_atomic.to_float();
    let total_profit_float = final_output_float - input_amount_float;
    let overall_slippage_fraction = 1.0 - total_slippage_fraction_product;
    let total_fee_usd = fee_breakdown.expected_fee_usd;

    let result = (
        total_profit_float,
        overall_slippage_fraction.max(0.0).min(1.0),
        total_fee_usd,
    );

    MULTI_HOP_CACHE.insert(cache_key, result);
    debug!(
        "Multi-hop calculation result: {:?} for input {} ({})",
        result, input_amount_float, start_token_decimals
    );
    result
}
