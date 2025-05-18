#[cfg(test)]
mod tests {
    use super::super::calculator::*;

    #[test]
    fn test_is_profitable_true() {
        let profit = 2.0;
        let min_profit = 1.0;
        let tx_cost = 0.5;
        let slippage = 0.001;
        assert!(is_profitable(profit, min_profit, tx_cost, slippage));
    }

    #[test]
    fn test_is_profitable_false() {
        let profit = 0.5;
        let min_profit = 1.0;
        let tx_cost = 0.5;
        let slippage = 0.001;
        assert!(!is_profitable(profit, min_profit, tx_cost, slippage));
    }

    #[test]
    fn test_is_profitable_equal_to_min() {
        let profit = 1.0;
        let min_profit = 1.0;
        let tx_cost = 0.0;
        let slippage = 0.0;
        assert!(is_profitable(profit, min_profit, tx_cost, slippage));
    }

    #[test]
    fn test_is_profitable_with_slippage() {
        let profit = 1.5;
        let min_profit = 1.0;
        let tx_cost = 0.0;
        let slippage = 0.4; // 40% slippage
                            // After slippage: profit = 1.5 * (1 - 0.4) = 0.9, which is < min_profit
        assert!(!is_profitable(profit, min_profit, tx_cost, slippage));
    }

    #[test]
    fn test_is_profitable_with_tx_cost() {
        let profit = 1.5;
        let min_profit = 1.0;
        let tx_cost = 0.6;
        let slippage = 0.0;
        // After tx_cost: profit = 1.5 - 0.6 = 0.9, which is < min_profit
        assert!(!is_profitable(profit, min_profit, tx_cost, slippage));
    }
}
