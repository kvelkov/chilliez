#[cfg(test)]
mod tests {
    use super::super::calculator::*; // Imports is_profitable_calc, OpportunityCalculationResult

    // Helper to create a dummy OpportunityCalculationResult for tests
    fn create_dummy_opp_result(profit_in_token: f64) -> OpportunityCalculationResult {
        OpportunityCalculationResult {
            input_amount: 1.0, // Arbitrary, not directly used by is_profitable_calc logic itself
            output_amount: 1.0 + profit_in_token, // To make profit consistent
            profit: profit_in_token,
            profit_percentage: if 1.0 > 0.0 { profit_in_token / 1.0 } else { 0.0 }, // Based on input_amount=1.0
            price_impact: 0.0, // Arbitrary
        }
    }

    #[test]
    fn test_is_profitable_true() {
        let profit_in_token = 2.0;
        let opp_result = create_dummy_opp_result(profit_in_token);
        let token_price_in_sol = 1.0; // For tests, assuming SOL is the currency for costs/thresholds
        let tx_cost_in_sol = 0.5;
        let min_profit_threshold_sol = 1.0;
        assert!(is_profitable_calc(&opp_result, token_price_in_sol, tx_cost_in_sol, min_profit_threshold_sol));
    }

    #[test]
    fn test_is_profitable_false() {
        let profit_in_token = 0.5;
        let opp_result = create_dummy_opp_result(profit_in_token);
        let token_price_in_sol = 1.0;
        let tx_cost_in_sol = 0.5;
        let min_profit_threshold_sol = 1.0;
        assert!(!is_profitable_calc(&opp_result, token_price_in_sol, tx_cost_in_sol, min_profit_threshold_sol));
    }

    #[test]
    fn test_is_profitable_equal_to_min() {
        let profit_in_token = 1.0;
        let opp_result = create_dummy_opp_result(profit_in_token);
        let token_price_in_sol = 1.0;
        let tx_cost_in_sol = 0.0; // Net profit will be 1.0, which is not > min_profit (1.0)
        let min_profit_threshold_sol = 1.0;
        // is_profitable checks if net_profit > min_profit_threshold, so equal is false
        assert!(!is_profitable_calc(&opp_result, token_price_in_sol, tx_cost_in_sol, min_profit_threshold_sol));
    }

    #[test]
    fn test_is_profitable_with_slippage() {
        // Note: The is_profitable_calc function itself doesn't directly handle slippage.
        // Slippage should be accounted for when calculating the `profit_in_token` 
        // that goes into OpportunityCalculationResult.
        let initial_profit = 1.5;
        let slippage_factor = 0.4; // 40% slippage
        let profit_after_slippage = initial_profit * (1.0 - slippage_factor); // 0.9

        let opp_result = create_dummy_opp_result(profit_after_slippage);
        let token_price_in_sol = 1.0;
        let tx_cost_in_sol = 0.0;
        let min_profit_threshold_sol = 1.0; // Net profit 0.9, min profit 1.0
        assert!(!is_profitable_calc(&opp_result, token_price_in_sol, tx_cost_in_sol, min_profit_threshold_sol));
    }

    #[test]
    fn test_is_profitable_with_tx_cost() {
        let profit_in_token = 1.5; // Profit before tx_cost
        let opp_result = create_dummy_opp_result(profit_in_token);
        let token_price_in_sol = 1.0;
        let tx_cost_in_sol = 0.6;     // Profit in SOL is 1.5. Net profit after tx_cost is 1.5 - 0.6 = 0.9
        let min_profit_threshold_sol = 1.0; // Min profit 1.0
        assert!(!is_profitable_calc(&opp_result, token_price_in_sol, tx_cost_in_sol, min_profit_threshold_sol));
    }
}