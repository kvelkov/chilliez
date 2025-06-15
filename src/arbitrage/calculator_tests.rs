#[cfg(test)]
mod tests {
    // Import all relevant functions and types from our calculator module.
    // Adjust the import path as needed depending on your project hierarchy.
    use crate::arbitrage::analysis::{OpportunityCalculationResult, is_profitable_calc};
    
    /// Helper function that creates a dummy `OpportunityCalculationResult`
    /// to simulate different arbitrage outcomes. It uses an input amount of 1.0,
    /// so that profit percentage is computed as profit_in_token (since 1.0 is the base).
    fn create_dummy_opp_result(profit_in_token: f64) -> OpportunityCalculationResult {
        OpportunityCalculationResult {
            input_amount: 1.0,
            output_amount: 1.0 + profit_in_token,
            profit: profit_in_token,
            profit_percentage: if 1.0 > 0.0 { profit_in_token / 1.0 } else { 0.0 },
        }
    }

    /// Test where the arbitrage net profit (after tx cost) is clearly above the minimum threshold.
    #[test]
    fn test_is_profitable_true() {
        let profit_in_token = 2.0;
        let opp_result = create_dummy_opp_result(profit_in_token);
        let token_price_in_sol = 1.0; // Assume 1:1 conversion for simplicity.
        let tx_cost_in_sol = 0.5;     // Transaction cost in SOL
        let min_profit_threshold_sol = 1.0;
        // Net profit = (2.0 * 1.0) - 0.5 = 1.5, which is > 1.0
        assert!(
            is_profitable_calc(&opp_result, token_price_in_sol, tx_cost_in_sol, min_profit_threshold_sol),
            "Expected net profit to exceed the minimum threshold"
        );
    }

    /// Test where the arbitrage profit is too low to exceed the threshold.
    #[test]
    fn test_is_profitable_false() {
        let profit_in_token = 0.5;
        let opp_result = create_dummy_opp_result(profit_in_token);
        let token_price_in_sol = 1.0;
        let tx_cost_in_sol = 0.5;
        let min_profit_threshold_sol = 1.0;
        // Net profit = (0.5 * 1.0) - 0.5 = 0.0 which is below 1.0
        assert!(
            !is_profitable_calc(&opp_result, token_price_in_sol, tx_cost_in_sol, min_profit_threshold_sol),
            "Expected net profit to be below the minimum threshold"
        );
    }

    /// Test when the net profit is exactly equal to the threshold.
    /// The function is designed to return false when profit is not strictly greater than the threshold.
    #[test]
    fn test_is_profitable_equal_to_min() {
        let profit_in_token = 1.0;
        let opp_result = create_dummy_opp_result(profit_in_token);
        let token_price_in_sol = 1.0;
        let tx_cost_in_sol = 0.0; // Net profit equals 1.0, exactly equal to min_profit (1.0)
        let min_profit_threshold_sol = 1.0;
        // Here net profit == threshold; our function expects profit > threshold, so return false.
        assert!(
            !is_profitable_calc(&opp_result, token_price_in_sol, tx_cost_in_sol, min_profit_threshold_sol),
            "Expected profit equal to threshold to return false"
        );
    }

    /// Test how profit calculations behave under slippage conditions.
    /// Slippage reduces the effective profit, so if the profit after slippage is too low, the opportunity should not be profitable.
    #[test]
    fn test_is_profitable_with_slippage() {
        let initial_profit = 1.5;
        let slippage_factor = 0.4; // 40% slippage reduces profit by 40%
        let profit_after_slippage = initial_profit * (1.0 - slippage_factor); // 0.9 profit effectively
        let opp_result = create_dummy_opp_result(profit_after_slippage);
        let token_price_in_sol = 1.0;
        let tx_cost_in_sol = 0.0;
        let min_profit_threshold_sol = 1.0; // Required net profit 1.0, but effective is 0.9
        assert!(
            !is_profitable_calc(&opp_result, token_price_in_sol, tx_cost_in_sol, min_profit_threshold_sol),
            "Expected profit after slippage to be insufficient"
        );
    }

    /// Test how transaction cost diminishes profit.
    #[test]
    fn test_is_profitable_with_tx_cost() {
        let profit_in_token = 1.5; // Gross profit before tx cost.
        let opp_result = create_dummy_opp_result(profit_in_token);
        let token_price_in_sol = 1.0;
        let tx_cost_in_sol = 0.6; // Net profit = 1.5 - 0.6 = 0.9, below the threshold.
        let min_profit_threshold_sol = 1.0;
        assert!(
            !is_profitable_calc(&opp_result, token_price_in_sol, tx_cost_in_sol, min_profit_threshold_sol),
            "Expected net profit after transaction cost to be insufficient"
        );
    }
}
