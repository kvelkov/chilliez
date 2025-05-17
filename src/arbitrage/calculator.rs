use crate::arbitrage::fee_manager::{get_gas_cost_for_dex, FeeManager, XYKSlippageModel};
use crate::utils::{PoolInfo, TokenAmount};

/// Calculates the optimal input amount for maximum profit in an arbitrage opportunity.
/// Uses constant product AMM formula to determine the ideal trade size.
///
/// This implementation uses a simplified approach based on the constant product formula
/// where the optimal input can be derived from the reserves and price difference between pools.
pub fn calculate_optimal_input(
    pool_a: &PoolInfo,
    pool_b: &PoolInfo,
    is_a_to_b: bool,
    max_input_amount: TokenAmount,
) -> TokenAmount {
    // Extract reserves based on trade direction
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

    // Extract reserves for the second pool (reverse direction)
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

    // Calculate price ratios (simplified, ignoring fees for the formula)
    let price_a = b_out_reserve as f64 / a_in_reserve as f64;
    let price_b = a_out_reserve as f64 / b_in_reserve as f64;

    // Calculate optimal input using square root formula for constant product AMMs
    // Formula: optimal_input = sqrt(reserve_in * reserve_out * (1 - fee_a) * (1 - fee_b) / price_ratio) - reserve_in
    // This is a simplified version - production code would need precise fee handling

    // For now, use a more conservative approach based on max amount
    let optimal_percentage = if price_b * price_a > 1.01 {
        // If there's a significant arbitrage opportunity, use more capital
        0.75
    } else if price_b * price_a > 1.005 {
        // For smaller opportunities, be more conservative
        0.5
    } else {
        // For minimal opportunities, use even less
        0.25
    };

    let amount = (max_input_amount.amount as f64 * optimal_percentage) as u64;
    TokenAmount::new(amount, a_in_decimals)
}

/// Calculates the maximum theoretical profit possible between two pools.
///
/// This function simulates the complete arbitrage cycle:
/// 1. Trade token A for token B in pool_a
/// 2. Trade token B for token A in pool_b
/// 3. Calculate the net profit after fees
///
/// Returns the profit as a decimal percentage (e.g., 0.01 = 1%)
pub fn calculate_max_profit(
    pool_a: &PoolInfo,
    pool_b: &PoolInfo,
    is_a_to_b: bool,
    input_amount: TokenAmount,
) -> f64 {
    // Helper closure to compute fee as a ratio
    let get_fee = |pool: &PoolInfo| pool.fee_numerator as f64 / pool.fee_denominator as f64;

    // Extract reserves and compute fee percentage as a float
    let (a_in_reserve, a_fee, b_out_reserve, _) = if is_a_to_b {
        (
            pool_a.token_a.reserve,
            get_fee(pool_a),
            pool_a.token_b.reserve,
            pool_a.token_b.decimals,
        )
    } else {
        (
            pool_a.token_b.reserve,
            get_fee(pool_a),
            pool_a.token_a.reserve,
            pool_a.token_a.decimals,
        )
    };

    // Extract reserves for the second pool (reverse direction)
    let (b_in_reserve, b_fee, a_out_reserve, _) = if is_a_to_b {
        (
            pool_b.token_b.reserve,
            get_fee(pool_b),
            pool_b.token_a.reserve,
            pool_b.token_a.decimals,
        )
    } else {
        (
            pool_b.token_a.reserve,
            get_fee(pool_b),
            pool_b.token_b.reserve,
            pool_b.token_b.decimals,
        )
    };

    // Calculate output from first swap (constant product formula with fees)
    let input_with_fee = input_amount.amount as f64 * (1.0 - a_fee);
    let numerator = input_with_fee * b_out_reserve as f64;
    let denominator = a_in_reserve as f64 + input_with_fee;
    let b_amount = numerator / denominator;

    // Calculate output from second swap
    let b_input_with_fee = b_amount * (1.0 - b_fee);
    let numerator2 = b_input_with_fee * a_out_reserve as f64;
    let denominator2 = b_in_reserve as f64 + b_input_with_fee;
    let a_final_amount = numerator2 / denominator2;

    // Calculate profit percentage
    let profit = a_final_amount - input_amount.amount as f64;
    let profit_percentage = profit / input_amount.amount as f64;

    profit_percentage.max(0.0) // Ensure we don't return negative profit
}

/// Transaction cost modeling for opportunity evaluation.
/// Used in testing/profit simulation to determine if an arbitrage is worth executing.
///
/// Estimates the cost of executing a transaction on Solana based on its size
/// and any priority fees that might be required during network congestion.
///
/// Parameters:
/// - transaction_size: Size of the transaction in bytes
/// - priority_fee: Additional fee to prioritize transaction (in micro-lamports per CU)
///
/// Returns the estimated cost in SOL
pub fn calculate_transaction_cost(transaction_size: usize, priority_fee: u64) -> f64 {
    // Solana transaction cost components:
    const BASE_FEE_LAMPORTS: f64 = 5000.0; // Base fee in lamports
    const LAMPORTS_PER_SIGNATURE: f64 = 5000.0; // Fee per signature
    const SIGNATURES_PER_TX: usize = 2; // Typical number of signatures for an arb tx
    const COMPUTE_UNITS_PER_TX: u64 = 200_000; // Estimated CUs for a swap transaction
    const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0; // Conversion rate

    // Calculate signature cost
    let signature_cost = SIGNATURES_PER_TX as f64 * LAMPORTS_PER_SIGNATURE;

    // Calculate priority fee cost (if any)
    let priority_cost = if priority_fee > 0 {
        (priority_fee as f64 * COMPUTE_UNITS_PER_TX as f64) / 1_000_000.0 // Convert from micro-lamports
    } else {
        0.0
    };

    // Calculate size-based cost (simplified)
    let size_cost = transaction_size as f64 * 0.0; // Currently Solana doesn't charge by size

    // Total cost in SOL
    let total_lamports = BASE_FEE_LAMPORTS + signature_cost + priority_cost + size_cost;
    total_lamports / LAMPORTS_PER_SOL
}

/// Check if an arbitrage opportunity is profitable after accounting for
/// transaction costs and minimum profit threshold requirements.
///
/// This is a core decision function used to determine whether to execute a trade.
///
/// Parameters:
/// - profit_in_token: The raw profit amount in token units
/// - token_price_in_sol: The price of the profit token in SOL
/// - transaction_cost_in_sol: The estimated cost to execute the transaction
/// - min_profit_threshold: Minimum acceptable profit in SOL
///
/// Returns true if the opportunity should be executed
pub fn is_profitable(
    profit_in_token: f64,
    token_price_in_sol: f64,
    transaction_cost_in_sol: f64,
    min_profit_threshold: f64,
) -> bool {
    let profit_in_sol = profit_in_token * token_price_in_sol;

    // Calculate profit after costs
    let net_profit = profit_in_sol - transaction_cost_in_sol;

    // Check if profit exceeds our minimum threshold
    net_profit > min_profit_threshold
}

/// Estimates the price impact of a trade on a pool.
/// Useful for determining how much slippage to expect.
///
/// Parameters:
/// - pool: The pool information
/// - input_token_index: 0 for token A, 1 for token B
/// - input_amount: Amount of tokens to swap
///
/// Returns the estimated price impact as a percentage (0.0-1.0)
pub fn estimate_price_impact(
    pool: &PoolInfo,
    input_token_index: usize,
    input_amount: TokenAmount,
) -> f64 {
    let (input_reserve, _) = if input_token_index == 0 {
        (pool.token_a.reserve, pool.token_b.reserve)
    } else {
        (pool.token_b.reserve, pool.token_a.reserve)
    };

    // Calculate price impact using AMM formula
    let input_amount_f64 = input_amount.amount as f64;
    let input_reserve_f64 = input_reserve as f64;

    // Price impact formula: 1 - (x / (x + Δx))
    // where x is the input reserve and Δx is the input amount
    let price_impact = input_amount_f64 / (input_reserve_f64 + input_amount_f64);

    price_impact.min(1.0)
}

/// Calculates the total profit, slippage, and fees for a multi-hop arbitrage opportunity.
/// Uses FeeManager for fee/slippage/gas analytics and aggregates across all hops.
pub fn calculate_multihop_profit_and_slippage(
    pools: &[&PoolInfo],
    input_amount: f64,
    directions: &[bool],
    last_fee_data: &[(Option<u64>, Option<u64>, Option<u64>)],
) -> (f64, f64, f64) {
    // Simulate each hop sequentially (placeholder: use real AMM math for each hop)
    let mut amounts = Vec::new();
    let mut current_amount = input_amount;
    for i in 0..pools.len() {
        // Placeholder: apply 2% loss per hop for slippage/fee (replace with real swap math)
        current_amount *= 0.98;
        amounts.push(TokenAmount::new(
            current_amount as u64,
            pools[i].token_a.decimals,
        ));
    }
    // Use advanced fee/slippage/gas estimation
    let slippage_model = XYKSlippageModel;
    let fee_breakdown = FeeManager::estimate_multi_hop_with_model(
        pools,
        &amounts,
        directions,
        last_fee_data,
        &slippage_model,
    );
    // Example: get gas cost for first DEX
    let _gas_cost = get_gas_cost_for_dex(pools[0].dex_type);
    // Example: convert fee to USDC (stub)
    let _fee_in_usdc = FeeManager::convert_fee_to_reference_token(
        fee_breakdown.expected_fee,
        &pools[0].token_a.symbol,
        "USDC",
    );
    let total_profit = current_amount - input_amount - fee_breakdown.expected_fee;
    (
        total_profit,
        fee_breakdown.expected_slippage,
        fee_breakdown.expected_fee,
    )
}

/// Calculates the total rebate for a multi-hop arbitrage opportunity (if supported by DEX).
pub fn calculate_rebate(_pools: &[&PoolInfo], _amounts: &[TokenAmount]) -> f64 {
    // Placeholder: implement DEX-specific rebate logic here
    0.0
}
