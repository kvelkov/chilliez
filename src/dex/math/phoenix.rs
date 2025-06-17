//! Phoenix DEX order book math calculations
//!
//! This module provides production-ready mathematical calculations for Phoenix DEX,
//! which uses an order book model rather than AMM. It includes market order execution,
//! price impact calculations, and market depth analysis.

use anyhow::{anyhow, Result};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Order book level for calculations
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderBookLevel {
    pub price: u64, // Price in scaled units (e.g., microlamports)
    pub size: u64,  // Size in base token units
}

/// Calculate market order execution through order book levels
/// Returns (filled_amount, total_cost, weighted_average_price)
pub fn calculate_market_order_execution(
    order_amount: u64,
    levels: &[OrderBookLevel],
    is_buy_order: bool,
) -> Result<(u64, u64, u64)> {
    if levels.is_empty() {
        return Err(anyhow!("Order book levels cannot be empty"));
    }

    let mut remaining_amount = order_amount;
    let mut total_cost = 0u64;
    let mut filled_amount = 0u64;
    let mut total_value = 0u128; // For weighted average price calculation

    for level in levels {
        if remaining_amount == 0 {
            break;
        }

        let fill_amount = remaining_amount.min(level.size);

        if is_buy_order {
            // Buying: multiply amount by price
            let cost = (fill_amount as u128 * level.price as u128) / 1_000_000; // Scale down from micro units
            total_cost = total_cost.saturating_add(cost as u64);
        } else {
            // Selling: amount is what we get
            total_cost = total_cost.saturating_add(fill_amount);
        }

        total_value += fill_amount as u128 * level.price as u128;
        filled_amount = filled_amount.saturating_add(fill_amount);
        remaining_amount = remaining_amount.saturating_sub(fill_amount);
    }

    // Calculate weighted average price (don't divide by 1_000_000 again since price is already in micro units)
    let weighted_avg_price = if filled_amount > 0 {
        (total_value / filled_amount as u128) as u64
    } else {
        0
    };

    Ok((filled_amount, total_cost, weighted_avg_price))
}

/// Calculate price impact for a market order
pub fn calculate_order_book_price_impact(
    order_amount: u64,
    levels: &[OrderBookLevel],
    mid_price: u64,
) -> Result<Decimal> {
    if levels.is_empty() || mid_price == 0 {
        return Ok(Decimal::ZERO);
    }

    let (filled_amount, _total_cost, weighted_avg_price) =
        calculate_market_order_execution(order_amount, levels, true)?;

    if filled_amount == 0 {
        return Ok(Decimal::ONE); // 100% impact if no fill
    }

    let mid_price_decimal = Decimal::from(mid_price);
    let avg_price_decimal = Decimal::from(weighted_avg_price);

    let price_impact = (avg_price_decimal - mid_price_decimal).abs() / mid_price_decimal;
    Ok(price_impact)
}

/// Calculate optimal order size given price impact tolerance
pub fn calculate_optimal_order_size(
    levels: &[OrderBookLevel],
    mid_price: u64,
    max_price_impact_bps: u32,
) -> Result<u64> {
    if levels.is_empty() || mid_price == 0 {
        return Ok(0);
    }

    let max_impact = Decimal::from(max_price_impact_bps) / Decimal::from(10000u32);
    let mut optimal_size = 0u64;

    // Binary search for optimal size (simplified approach)
    let mut test_size = levels.iter().map(|l| l.size).sum::<u64>() / 4; // Start with 25% of total liquidity
    let mut step = test_size / 2;

    for _ in 0..10 {
        // Limit iterations
        let impact = calculate_order_book_price_impact(test_size, levels, mid_price)?;

        if impact <= max_impact {
            optimal_size = test_size;
            test_size = test_size.saturating_add(step);
        } else {
            test_size = test_size.saturating_sub(step);
        }

        step /= 2;
        if step == 0 {
            break;
        }
    }

    Ok(optimal_size)
}

/// Calculate spread and market depth metrics
pub fn calculate_market_metrics(
    bids: &[OrderBookLevel], // Sorted highest to lowest
    asks: &[OrderBookLevel], // Sorted lowest to highest
) -> Result<MarketMetrics> {
    let best_bid = bids.first().map(|l| l.price).unwrap_or(0);
    let best_ask = asks.first().map(|l| l.price).unwrap_or(0);

    let mid_price = if best_bid > 0 && best_ask > 0 {
        (best_bid + best_ask) / 2
    } else {
        0
    };

    let spread_bps = if mid_price > 0 && best_ask > best_bid {
        ((best_ask - best_bid) as u128 * 10000 / mid_price as u128) as u32
    } else {
        0
    };

    // Calculate depth within 1% of mid price
    let depth_threshold = mid_price / 100; // 1% of mid price

    let bid_depth = bids
        .iter()
        .take_while(|level| level.price >= mid_price.saturating_sub(depth_threshold))
        .map(|level| level.size)
        .sum::<u64>();

    let ask_depth = asks
        .iter()
        .take_while(|level| level.price <= mid_price.saturating_add(depth_threshold))
        .map(|level| level.size)
        .sum::<u64>();

    Ok(MarketMetrics {
        mid_price,
        best_bid,
        best_ask,
        spread_bps,
        bid_depth_1pct: bid_depth,
        ask_depth_1pct: ask_depth,
    })
}

/// Market metrics for order book analysis
#[derive(Debug, Clone)]
pub struct MarketMetrics {
    pub mid_price: u64,
    #[allow(dead_code)] // Used by WebSocket feed but not directly accessed in other modules
    pub best_bid: u64,
    #[allow(dead_code)] // Used by WebSocket feed but not directly accessed in other modules
    pub best_ask: u64,
    pub spread_bps: u32,
    pub bid_depth_1pct: u64, // Total size within 1% of mid price
    pub ask_depth_1pct: u64, // Total size within 1% of mid price
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_order_book() -> (Vec<OrderBookLevel>, Vec<OrderBookLevel>) {
        let bids = vec![
            OrderBookLevel {
                price: 199_500_000,
                size: 1_000_000,
            }, // $199.50
            OrderBookLevel {
                price: 199_000_000,
                size: 2_000_000,
            }, // $199.00
            OrderBookLevel {
                price: 198_500_000,
                size: 1_500_000,
            }, // $198.50
        ];

        let asks = vec![
            OrderBookLevel {
                price: 200_500_000,
                size: 1_000_000,
            }, // $200.50
            OrderBookLevel {
                price: 201_000_000,
                size: 2_000_000,
            }, // $201.00
            OrderBookLevel {
                price: 201_500_000,
                size: 1_500_000,
            }, // $201.50
        ];

        (bids, asks)
    }

    #[test]
    fn test_market_order_execution_buy() {
        let (_bids, asks) = create_test_order_book();

        // Test buying 1.5M tokens
        let (filled, cost, avg_price) = calculate_market_order_execution(
            1_500_000, &asks, true, // buy order
        )
        .unwrap();

        assert_eq!(filled, 1_500_000);
        assert!(cost > 0);
        assert!(avg_price > 0);

        // Should fill first level completely and half of second level
        let expected_cost = (1_000_000 * 200_500_000 + 500_000 * 201_000_000) / 1_000_000;
        assert_eq!(cost, expected_cost);
    }

    #[test]
    fn test_market_order_execution_sell() {
        let (bids, _asks) = create_test_order_book();

        // Test selling 1.5M tokens
        let (filled, proceeds, avg_price) = calculate_market_order_execution(
            1_500_000, &bids, false, // sell order
        )
        .unwrap();

        assert_eq!(filled, 1_500_000);
        assert_eq!(proceeds, 1_500_000); // For sell orders, proceeds = filled amount
        assert!(avg_price > 0);
    }

    #[test]
    fn test_price_impact_calculation() {
        let (_bids, asks) = create_test_order_book();
        let mid_price = 200_000_000; // $200.00

        let impact = calculate_order_book_price_impact(1_000_000, &asks, mid_price).unwrap();

        // Should have some positive price impact
        assert!(impact > Decimal::ZERO);
        assert!(impact < Decimal::ONE); // Less than 100%
    }

    #[test]
    fn test_market_metrics_calculation() {
        let (bids, asks) = create_test_order_book();

        let metrics = calculate_market_metrics(&bids, &asks).unwrap();

        assert_eq!(metrics.best_bid, 199_500_000);
        assert_eq!(metrics.best_ask, 200_500_000);
        assert_eq!(metrics.mid_price, 200_000_000);
        assert!(metrics.spread_bps > 0);
        assert!(metrics.bid_depth_1pct > 0);
        assert!(metrics.ask_depth_1pct > 0);
    }

    #[test]
    fn test_optimal_order_size() {
        let (_bids, asks) = create_test_order_book();
        let mid_price = 200_000_000;
        let max_impact_bps = 100; // 1% max impact

        let optimal_size = calculate_optimal_order_size(&asks, mid_price, max_impact_bps).unwrap();

        assert!(optimal_size > 0);
        assert!(optimal_size <= asks.iter().map(|l| l.size).sum::<u64>());
    }

    #[test]
    fn test_empty_order_book_handling() {
        let empty_levels: Vec<OrderBookLevel> = vec![];

        // Should return error for empty order book
        let result = calculate_market_order_execution(1_000_000, &empty_levels, true);
        assert!(result.is_err());

        let result = calculate_order_book_price_impact(1_000_000, &empty_levels, 200_000_000);
        assert!(result.unwrap() == Decimal::ZERO);
    }

    #[test]
    fn test_zero_mid_price_handling() {
        let (_bids, asks) = create_test_order_book();

        let impact = calculate_order_book_price_impact(1_000_000, &asks, 0).unwrap();
        assert_eq!(impact, Decimal::ZERO);

        let optimal_size = calculate_optimal_order_size(&asks, 0, 100).unwrap();
        assert_eq!(optimal_size, 0);
    }
}
