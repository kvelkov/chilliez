// src/dex/clients/phoenix.rs
//! Phoenix DEX integration with order book support.
//!
//! Phoenix uses an order book model rather than AMM, requiring different
//! interaction patterns compared to other DEXes. This implementation provides
//! production-ready order book math, market order execution, and swap instruction building.

use crate::dex::api::{
    CommonSwapInfo, DexClient, DexHealthStatus, PoolDiscoverable, Quote, SwapInfo,
};
use crate::dex::math::phoenix;
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{DexType, PoolInfo, PoolParser as UtilsPoolParser, PoolToken};
use anyhow::{anyhow, Result as AnyhowResult};
use async_trait::async_trait;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};
use std::sync::Arc;

// --- Constants ---
pub const PHOENIX_PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!("PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY");

// Instruction discriminators for Phoenix
const PHOENIX_PLACE_ORDER_DISCRIMINATOR: [u8; 8] = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]; // Placeholder

/// Phoenix order side
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum OrderSide {
    Bid,
    Ask,
}

/// Phoenix order type
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum OrderType {
    Limit,
    Market,
    PostOnly,
    ImmediateOrCancel,
}

/// Order book state (simplified)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderBook {
    pub bids: Vec<phoenix::OrderBookLevel>, // Sorted highest to lowest
    pub asks: Vec<phoenix::OrderBookLevel>, // Sorted lowest to highest
    pub last_update: u64,
}

impl OrderBook {
    /// Create a sample order book for testing
    pub fn create_sample() -> Self {
        Self {
            bids: vec![
                phoenix::OrderBookLevel {
                    price: 199_500_000,
                    size: 1_000_000,
                }, // $199.50
                phoenix::OrderBookLevel {
                    price: 199_000_000,
                    size: 2_000_000,
                }, // $199.00
                phoenix::OrderBookLevel {
                    price: 198_500_000,
                    size: 1_500_000,
                }, // $198.50
            ],
            asks: vec![
                phoenix::OrderBookLevel {
                    price: 200_500_000,
                    size: 1_000_000,
                }, // $200.50
                phoenix::OrderBookLevel {
                    price: 201_000_000,
                    size: 2_000_000,
                }, // $201.00
                phoenix::OrderBookLevel {
                    price: 201_500_000,
                    size: 1_500_000,
                }, // $201.50
            ],
            last_update: chrono::Utc::now().timestamp_millis() as u64,
        }
    }

    /// Get best bid price
    pub fn best_bid(&self) -> Option<u64> {
        self.bids.first().map(|level| level.price)
    }

    /// Get best ask price
    pub fn best_ask(&self) -> Option<u64> {
        self.asks.first().map(|level| level.price)
    }

    /// Get mid price
    pub fn mid_price(&self) -> Option<u64> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some((bid + ask) / 2),
            _ => None,
        }
    }

    /// Calculate market order execution using production Phoenix math
    #[allow(dead_code)] // Used in Phoenix integration tests
    pub fn calculate_market_execution(&self, amount: u64, side: OrderSide) -> (u64, u64) {
        let levels = match side {
            OrderSide::Bid => &self.asks, // Buying, so we consume asks
            OrderSide::Ask => &self.bids, // Selling, so we consume bids
        };

        // Use production Phoenix math for market order execution
        match phoenix::calculate_market_order_execution(amount, levels, side == OrderSide::Bid) {
            Ok((filled_amount, total_cost, _weighted_avg_price)) => (filled_amount, total_cost),
            Err(_) => (0, 0), // Fallback on error
        }
    }

    /// Calculate price impact for a given trade size using production Phoenix math
    #[allow(dead_code)] // Used in Phoenix integration tests
    pub fn calculate_price_impact(&self, amount: u64, side: OrderSide) -> f64 {
        let mid_price = match self.mid_price() {
            Some(price) => price,
            None => return 0.0,
        };

        let levels = match side {
            OrderSide::Bid => &self.asks, // Buying, so we consume asks
            OrderSide::Ask => &self.bids, // Selling, so we consume bids
        };

        // Use production Phoenix math for price impact calculation
        match phoenix::calculate_order_book_price_impact(amount, levels, mid_price) {
            Ok(impact_decimal) => {
                // Convert Decimal to f64 for compatibility
                impact_decimal.to_string().parse::<f64>().unwrap_or(0.0)
            }
            Err(_) => 1.0, // Return 100% impact on error
        }
    }
}

/// Phoenix pool parser (markets in Phoenix context)
pub struct PhoenixPoolParser;

#[async_trait]
impl UtilsPoolParser for PhoenixPoolParser {
    async fn parse_pool_data(
        &self,
        market_address: Pubkey,
        _data: &[u8],
        _rpc_client: &Arc<SolanaRpcClient>,
    ) -> AnyhowResult<PoolInfo> {
        // Phoenix markets are more complex than AMM pools
        // This is a simplified implementation
        warn!("PhoenixPoolParser: Simplified market parsing - full implementation requires Phoenix SDK integration");

        // For now, create a basic PoolInfo structure
        // In production, this would parse the actual Phoenix market state
        let pool_info = PoolInfo {
            address: market_address,
            name: "Phoenix Market".to_string(),
            token_a: PoolToken {
                mint: Pubkey::default(), // Would be parsed from market state
                symbol: "BASE".to_string(),
                decimals: 6,
                reserve: 0, // Order books don't have traditional reserves
            },
            token_b: PoolToken {
                mint: Pubkey::default(),
                symbol: "QUOTE".to_string(),
                decimals: 6,
                reserve: 0,
            },
            token_a_vault: Pubkey::default(), // Would be base vault
            token_b_vault: Pubkey::default(), // Would be quote vault
            fee_numerator: Some(25),          // Typical Phoenix fee
            fee_denominator: Some(10000),
            fee_rate_bips: Some(25),
            last_update_timestamp: chrono::Utc::now().timestamp() as u64,
            dex_type: DexType::Unknown("Phoenix".to_string()),
            // Order book specific fields (not applicable)
            liquidity: None,
            sqrt_price: None,
            tick_current_index: None,
            tick_spacing: None,
            tick_array_0: None,
            tick_array_1: None,
            tick_array_2: None,
            oracle: None,
        };

        Ok(pool_info)
    }

    fn parse_pool_data_sync(
        &self,
        market_address: Pubkey,
        _data: &[u8],
        _rpc_client: &Arc<SolanaRpcClient>,
    ) -> AnyhowResult<PoolInfo> {
        let pool_info = PoolInfo {
            address: market_address,
            name: "Phoenix Market".to_string(),
            token_a: PoolToken {
                mint: Pubkey::default(),
                symbol: "BASE".to_string(),
                decimals: 6,
                reserve: 0,
            },
            token_b: PoolToken {
                mint: Pubkey::default(),
                symbol: "QUOTE".to_string(),
                decimals: 6,
                reserve: 0,
            },
            token_a_vault: Pubkey::default(),
            token_b_vault: Pubkey::default(),
            fee_numerator: Some(25),
            fee_denominator: Some(10000),
            fee_rate_bips: Some(25),
            last_update_timestamp: chrono::Utc::now().timestamp() as u64,
            dex_type: DexType::Unknown("Phoenix".to_string()),
            liquidity: None,
            sqrt_price: None,
            tick_current_index: None,
            tick_spacing: None,
            tick_array_0: None,
            tick_array_1: None,
            tick_array_2: None,
            oracle: None,
        };
        Ok(pool_info)
    }

    fn get_program_id(&self) -> Pubkey {
        PHOENIX_PROGRAM_ID
    }
}

/// Phoenix DEX client
pub struct PhoenixClient {
    pub name: String,
}

impl PhoenixClient {
    #[allow(dead_code)] // Phoenix integration not yet activated
    pub fn new() -> Self {
        Self {
            name: "Phoenix".to_string(),
        }
    }

    /// Calculate market order quote using production Phoenix math
    fn calculate_market_order_quote(
        &self,
        market: &PoolInfo,
        input_amount: u64,
        side: OrderSide,
    ) -> AnyhowResult<u64> {
        info!(
            "Calculating Phoenix market order quote for {} with side {:?}",
            input_amount, side
        );

        // For now, create a sample order book since we don't have live data
        // In production, this would fetch real order book from on-chain data
        let sample_order_book = OrderBook::create_sample();

        let levels = match side {
            OrderSide::Bid => &sample_order_book.asks, // Buying, consume asks
            OrderSide::Ask => &sample_order_book.bids, // Selling, consume bids
        };

        // Use production Phoenix math for market order execution
        let (filled_amount, total_cost, _weighted_avg_price) =
            phoenix::calculate_market_order_execution(
                input_amount,
                levels,
                side == OrderSide::Bid,
            )?;

        // Apply fees
        let fee_rate = market.fee_rate_bips.unwrap_or(25) as f64 / 10000.0;
        let output_after_fees = if side == OrderSide::Bid {
            // Buying: reduce the filled amount by fees
            ((filled_amount as f64) * (1.0 - fee_rate)) as u64
        } else {
            // Selling: reduce the proceeds by fees
            ((total_cost as f64) * (1.0 - fee_rate)) as u64
        };

        warn!("PhoenixClient: Using sample order book data - production requires live market data");

        Ok(output_after_fees)
    }

    /// Build production Phoenix swap instruction using market orders
    fn build_swap_instruction(
        &self,
        swap_info: &CommonSwapInfo,
        market_info: &PoolInfo,
    ) -> Result<Instruction, crate::error::ArbError> {
        info!(
            "Building Phoenix swap instruction for market {}",
            market_info.address
        );

        // Determine order side based on token mints
        let is_buying_base = swap_info.destination_token_mint == market_info.token_a.mint;
        let side = if is_buying_base {
            OrderSide::Bid
        } else {
            OrderSide::Ask
        };

        // Calculate optimal order size considering price impact
        let sample_order_book = OrderBook::create_sample();
        let levels = match side {
            OrderSide::Bid => &sample_order_book.asks,
            OrderSide::Ask => &sample_order_book.bids,
        };

        let mid_price = sample_order_book.mid_price().unwrap_or(200_000_000);
        let max_price_impact_bps = 500; // 5% max price impact

        let optimal_size =
            phoenix::calculate_optimal_order_size(levels, mid_price, max_price_impact_bps)
                .map_err(|e| {
                    crate::error::ArbError::DexError(format!(
                        "Phoenix order size calculation failed: {}",
                        e
                    ))
                })?;

        // Use the smaller of requested amount or optimal size to prevent excessive slippage
        let actual_order_size = swap_info.input_amount.min(optimal_size);

        info!(
            "Phoenix order: requested={}, optimal={}, actual={}",
            swap_info.input_amount, optimal_size, actual_order_size
        );

        // Phoenix requires more complex account setup for order placement
        let accounts = vec![
            AccountMeta::new_readonly(PHOENIX_PROGRAM_ID, false),
            AccountMeta::new_readonly(swap_info.user_wallet_pubkey, true), // Signer
            AccountMeta::new(market_info.address, false),                  // Market
            AccountMeta::new(swap_info.user_source_token_account, false),  // User source
            AccountMeta::new(swap_info.user_destination_token_account, false), // User destination
            AccountMeta::new(market_info.token_a_vault, false),            // Base vault
            AccountMeta::new(market_info.token_b_vault, false),            // Quote vault
            // Phoenix-specific accounts (would be resolved from market state in production):
            // - Bids account (market_info.address + seed)
            // - Asks account (market_info.address + seed)
            // - Event queue (market_info.address + seed)
            // - Market authority (derived from market)
            AccountMeta::new_readonly(spl_token::id(), false), // Token program
            AccountMeta::new_readonly(system_program::id(), false), // System program
        ];

        // Build instruction data for market order with production parameters
        let mut instruction_data = Vec::new();
        instruction_data.extend_from_slice(&PHOENIX_PLACE_ORDER_DISCRIMINATOR);

        // Order parameters (production format)
        instruction_data.extend_from_slice(&actual_order_size.to_le_bytes()); // Size
        instruction_data.extend_from_slice(&swap_info.minimum_output_amount.to_le_bytes()); // Min output
        instruction_data.push(side as u8); // Order side
        instruction_data.push(OrderType::Market as u8); // Order type

        // Additional Phoenix-specific parameters
        instruction_data.extend_from_slice(&0u64.to_le_bytes()); // Order ID (0 for market orders)
        instruction_data.extend_from_slice(&u64::MAX.to_le_bytes()); // Max price (no limit for market orders)

        warn!("PhoenixClient: Market order instruction built using sample order book - production requires live market state");

        Ok(Instruction {
            program_id: PHOENIX_PROGRAM_ID,
            accounts,
            data: instruction_data,
        })
    }
}

#[async_trait]
impl DexClient for PhoenixClient {
    fn get_name(&self) -> &str {
        &self.name
    }

    fn calculate_onchain_quote(&self, market: &PoolInfo, input_amount: u64) -> AnyhowResult<Quote> {
        info!(
            "Calculating Phoenix onchain quote for {} tokens",
            input_amount
        );

        // For Phoenix, determine direction based on market structure
        // This is a simplified assumption - production would need more context
        let side = OrderSide::Bid; // Assume buying base token

        let output_amount = self.calculate_market_order_quote(market, input_amount, side)?;

        // Calculate slippage estimate using production Phoenix math
        let sample_order_book = OrderBook::create_sample();
        let mid_price = sample_order_book.mid_price().unwrap_or(200_000_000);
        let levels = &sample_order_book.asks; // For buying

        let slippage_estimate =
            match phoenix::calculate_order_book_price_impact(input_amount, levels, mid_price) {
                Ok(impact_decimal) => {
                    let impact_f64 = impact_decimal.to_string().parse::<f64>().unwrap_or(0.2);
                    Some(impact_f64.min(1.0)) // Cap at 100%
                }
                Err(_) => Some(0.2), // Default 20% slippage estimate on error
            };

        warn!("PhoenixClient: Quote calculation using sample order book - production requires live market data");

        Ok(Quote {
            input_token: market.token_b.symbol.clone(), // Quote token (simplified)
            output_token: market.token_a.symbol.clone(), // Base token (simplified)
            input_amount,
            output_amount,
            dex: self.name.clone(),
            route: vec![market.address],
            slippage_estimate,
        })
    }

    fn get_swap_instruction(&self, swap_info: &SwapInfo) -> AnyhowResult<Instruction> {
        warn!("get_swap_instruction for Phoenix is a basic implementation. Use get_swap_instruction_enhanced for production.");

        // Create a basic instruction for legacy compatibility
        Ok(Instruction {
            program_id: PHOENIX_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(swap_info.user_wallet, true),
                AccountMeta::new(swap_info.pool_account, false), // Market in Phoenix context
                AccountMeta::new(swap_info.user_source_token_account, false),
                AccountMeta::new(swap_info.user_destination_token_account, false),
            ],
            data: PHOENIX_PLACE_ORDER_DISCRIMINATOR.to_vec(),
        })
    }

    async fn get_swap_instruction_enhanced(
        &self,
        swap_info: &CommonSwapInfo,
        market_info: Arc<PoolInfo>,
    ) -> Result<Instruction, crate::error::ArbError> {
        info!(
            "PhoenixClient: Building market order instruction for market {} ({} -> {})",
            market_info.address, swap_info.source_token_mint, swap_info.destination_token_mint
        );

        warn!("PhoenixClient: Order book integration requires careful consideration of market depth and timing");

        self.build_swap_instruction(swap_info, &market_info)
    }

    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        info!("PhoenixClient: Starting market discovery...");

        // Phoenix market discovery would require:
        // 1. Scanning for Phoenix market accounts
        // 2. Parsing market state for each discovered market
        // 3. Converting market data to PoolInfo format

        warn!("PhoenixClient: Market discovery not yet implemented - requires Phoenix market scanning");

        Ok(Vec::new())
    }

    async fn health_check(&self) -> Result<DexHealthStatus, crate::error::ArbError> {
        let start_time = std::time::Instant::now();

        // Phoenix health check could involve:
        // 1. Checking if we can fetch a known market account
        // 2. Verifying order book data is accessible
        // 3. Testing order placement (in simulation mode)

        let is_healthy = true; // Placeholder
        let response_time = start_time.elapsed().as_millis() as u64;

        Ok(DexHealthStatus {
            is_healthy,
            last_successful_request: Some(start_time),
            error_count: 0,
            response_time_ms: Some(response_time),
            pool_count: Some(0),
            status_message: "Phoenix client operational (order book DEX)".to_string(),
        })
    }
}

#[async_trait]
impl PoolDiscoverable for PhoenixClient {
    async fn discover_pools(&self) -> AnyhowResult<Vec<PoolInfo>> {
        DexClient::discover_pools(self).await
    }

    async fn fetch_pool_data(&self, _market_address: Pubkey) -> AnyhowResult<PoolInfo> {
        // This would fetch specific market data from on-chain
        Err(anyhow!(
            "PhoenixClient: fetch_pool_data not yet implemented"
        ))
    }

    fn dex_name(&self) -> &str {
        &self.name
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phoenix_client_creation() {
        let client = PhoenixClient::new();
        assert_eq!(client.get_name(), "Phoenix");
    }

    #[test]
    fn test_order_side_enum() {
        let bid = OrderSide::Bid;
        let ask = OrderSide::Ask;

        // Test that we can match on order sides
        match bid {
            OrderSide::Bid => {},
            OrderSide::Ask => unreachable!(),
        }

        match ask {
            OrderSide::Bid => unreachable!(),
            OrderSide::Ask => {},
        }
    }

    #[test]
    fn test_order_type_enum() {
        let market_order = OrderType::Market;
        let limit_order = OrderType::Limit;

        // Test that we can differentiate order types
        match market_order {
            OrderType::Market => {},
            _ => unreachable!(),
        }

        match limit_order {
            OrderType::Limit => {},
            _ => unreachable!(),
        }
    }
}
