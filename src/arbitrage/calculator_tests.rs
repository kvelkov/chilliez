#[cfg(test)]
mod tests {
    use super::super::calculator::*;

    #[test]
    fn test_is_profitable_true() {
        let profit = 2.0;
        let token_price_in_sol = 1.0;
        let tx_cost = 0.5;
        let min_profit = 1.0;
        assert!(is_profitable(profit, token_price_in_sol, tx_cost, min_profit));
    }

    #[test]
    fn test_is_profitable_false() {
        let profit = 0.5;
        let token_price_in_sol = 1.0;
        let tx_cost = 0.5;
        let min_profit = 1.0;
        assert!(!is_profitable(profit, token_price_in_sol, tx_cost, min_profit));
    }

    #[test]
    fn test_is_profitable_equal_to_min() {
        let profit = 1.0;
        let token_price_in_sol = 1.0;
        let tx_cost = 0.0;
        let min_profit = 1.0;
        assert!(!is_profitable(profit, token_price_in_sol, tx_cost, min_profit));
    }

    #[test]
    fn test_is_profitable_with_slippage() {
        let profit = 1.5;
        let token_price_in_sol = 1.0;
        let tx_cost = 0.0;
        let min_profit = 1.0;
        let slippage = 0.4; // 40% slippage
        let profit_after_slippage = profit * (1.0 - slippage);
        assert!(!is_profitable(profit_after_slippage, token_price_in_sol, tx_cost, min_profit));
    }

    #[test]
    fn test_is_profitable_with_tx_cost() {
        let profit = 1.5;
        let token_price_in_sol = 1.0;
        let tx_cost = 0.6;
        let min_profit = 1.0;
        assert!(!is_profitable(profit, token_price_in_sol, tx_cost, min_profit));
    }
}
