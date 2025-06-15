//! Comprehensive DEX tests for all clients and functionality.
//! This file includes tests for Meteora, Lifinity, Phoenix, and enhanced functionality.

#[cfg(test)]
mod meteora_tests {
    use super::super::clients::meteora::*;
    use super::super::api::DexClient;
    use crate::utils::{DexType, PoolInfo, PoolToken};
    use solana_sdk::pubkey::Pubkey;

    fn create_test_meteora_client() -> MeteoraClient {
        MeteoraClient::new()
    }

    fn create_mock_dynamic_amm_pool() -> PoolInfo {
        PoolInfo {
            address: Pubkey::new_unique(),
            name: "Test Meteora Dynamic AMM Pool".to_string(),
            token_a: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "SOL".to_string(),
                decimals: 9,
                reserve: 1000000000, // 1 SOL
            },
            token_b: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 150000000, // 150 USDC
            },
            token_a_vault: Pubkey::new_unique(),
            token_b_vault: Pubkey::new_unique(),
            fee_numerator: Some(25),
            fee_denominator: Some(10000),
            fee_rate_bips: Some(25),
            last_update_timestamp: 1640995200,
            dex_type: DexType::Meteora,
            liquidity: None,
            sqrt_price: None,
            tick_current_index: None,
            tick_spacing: None,
            tick_array_0: None,
            tick_array_1: None,
            tick_array_2: None,
            oracle: Some(Pubkey::new_unique()),
        }
    }

    fn create_mock_dlmm_pool() -> PoolInfo {
        PoolInfo {
            address: Pubkey::new_unique(),
            name: "Test Meteora DLMM Pool".to_string(),
            token_a: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "SOL".to_string(),
                decimals: 9,
                reserve: 1000000000,
            },
            token_b: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 150000000,
            },
            token_a_vault: Pubkey::new_unique(),
            token_b_vault: Pubkey::new_unique(),
            fee_numerator: Some(25),
            fee_denominator: Some(10000),
            fee_rate_bips: Some(25),
            last_update_timestamp: 1640995200,
            dex_type: DexType::Meteora,
            liquidity: None,
            sqrt_price: None,
            tick_current_index: Some(8388608), // 2^23 (neutral bin)
            tick_spacing: Some(64), // DLMM bin step
            tick_array_0: None,
            tick_array_1: None,
            tick_array_2: None,
            oracle: Some(Pubkey::new_unique()),
        }
    }

    #[test]
    fn test_meteora_client_creation() {
        let client = create_test_meteora_client();
        assert_eq!(client.get_name(), "Meteora");
    }

    #[test]
    fn test_meteora_pool_type_detection() {
        let client = create_test_meteora_client();
        
        // Test Dynamic AMM pool (no tick_spacing)
        let dynamic_pool = create_mock_dynamic_amm_pool();
        assert_eq!(client.get_pool_type(&dynamic_pool), MeteoraPoolType::DynamicAmm);

        // Test DLMM pool (has tick_spacing)
        let dlmm_pool = create_mock_dlmm_pool();
        assert_eq!(client.get_pool_type(&dlmm_pool), MeteoraPoolType::Dlmm);
    }

    #[test]
    fn test_meteora_quote_calculation_dynamic_amm() {
        let client = create_test_meteora_client();
        let pool = create_mock_dynamic_amm_pool();
        let input_amount = 1000000; // 0.001 SOL

        let result = client.calculate_onchain_quote(&pool, input_amount);
        assert!(result.is_ok());
        
        let quote = result.unwrap();
        assert_eq!(quote.input_amount, input_amount);
        assert!(quote.output_amount > 0);
        assert_eq!(quote.dex, "Meteora");
        assert!(quote.slippage_estimate.is_some());
    }

    #[test]
    fn test_meteora_quote_calculation_dlmm() {
        let client = create_test_meteora_client();
        let pool = create_mock_dlmm_pool();
        let input_amount = 1000000;

        let result = client.calculate_onchain_quote(&pool, input_amount);
        assert!(result.is_ok());
        
        let quote = result.unwrap();
        assert_eq!(quote.input_amount, input_amount);
        assert!(quote.output_amount > 0);
        assert_eq!(quote.dex, "Meteora");
    }

    #[test]
    fn test_meteora_program_id_identification() {
        // Test Dynamic AMM identification
        let dynamic_type = MeteoraPoolParser::identify_pool_type(
            &METEORA_DYNAMIC_AMM_PROGRAM_ID,
            DYNAMIC_AMM_POOL_STATE_SIZE,
        );
        assert_eq!(dynamic_type, Some(MeteoraPoolType::DynamicAmm));

        // Test DLMM identification
        let dlmm_type = MeteoraPoolParser::identify_pool_type(
            &METEORA_DLMM_PROGRAM_ID,
            DLMM_LB_PAIR_STATE_SIZE,
        );
        assert_eq!(dlmm_type, Some(MeteoraPoolType::Dlmm));
    }

    #[tokio::test]
    async fn test_meteora_health_check() {
        let client = create_test_meteora_client();
        let result = client.health_check().await;
        assert!(result.is_ok());
        
        let health = result.unwrap();
        assert!(health.is_healthy);
        assert!(health.response_time_ms.is_some());
    }
}

#[cfg(test)]
mod lifinity_tests {
    use super::super::clients::lifinity::*;
    use super::super::api::DexClient;
    use crate::utils::{DexType, PoolInfo, PoolToken};
    use solana_sdk::pubkey::Pubkey;

    fn create_test_lifinity_client() -> LifinityClient {
        LifinityClient::new()
    }

    fn create_mock_lifinity_pool() -> PoolInfo {
        PoolInfo {
            address: Pubkey::new_unique(),
            name: "Test Lifinity Pool".to_string(),
            token_a: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "SOL".to_string(),
                decimals: 9,
                reserve: 1000000000, // 1 SOL
            },
            token_b: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 150000000, // 150 USDC
            },
            token_a_vault: Pubkey::new_unique(),
            token_b_vault: Pubkey::new_unique(),
            fee_numerator: Some(25),
            fee_denominator: Some(10000),
            fee_rate_bips: Some(25),
            last_update_timestamp: 1640995200,
            dex_type: DexType::Lifinity,
            liquidity: Some(1000000000000), // 1M liquidity
            sqrt_price: Some(12247448713915890491), // sqrt(150)
            tick_current_index: Some(0),
            tick_spacing: Some(64),
            tick_array_0: None,
            tick_array_1: None,
            tick_array_2: None,
            oracle: None,
        }
    }

    #[test]
    fn test_lifinity_client_creation() {
        let client = create_test_lifinity_client();
        assert_eq!(client.get_name(), "Lifinity");
    }

    #[test]
    fn test_lifinity_quote_calculation() {
        let client = create_test_lifinity_client();
        let pool = create_mock_lifinity_pool();
        let input_amount = 1000000; // 0.001 SOL

        let result = client.calculate_onchain_quote(&pool, input_amount);
        assert!(result.is_ok());
        
        let quote = result.unwrap();
        assert_eq!(quote.input_amount, input_amount);
        assert!(quote.output_amount > 0);
        assert_eq!(quote.dex, "Lifinity");
        assert!(quote.slippage_estimate.is_some());
        assert!(quote.slippage_estimate.unwrap() < 0.1); // Lifinity should have low slippage
    }

    #[test]
    fn test_lifinity_oracle_price_calculation() {
        let client = create_test_lifinity_client();
        let pool = create_mock_lifinity_pool();

        let oracle_price = client.calculate_oracle_price(&pool);
        assert!(oracle_price.is_some());
        assert_eq!(oracle_price.unwrap(), 150.0); // 150 USDC per SOL
    }

    #[test]
    fn test_lifinity_oracle_price_zero_reserves() {
        let client = create_test_lifinity_client();
        let mut pool = create_mock_lifinity_pool();
        pool.token_a.reserve = 0;

        let oracle_price = client.calculate_oracle_price(&pool);
        assert!(oracle_price.is_none());
    }

    #[tokio::test]
    async fn test_lifinity_health_check() {
        let client = create_test_lifinity_client();
        let result = client.health_check().await;
        assert!(result.is_ok());
        
        let health = result.unwrap();
        assert!(health.is_healthy);
        assert!(health.response_time_ms.is_some());
        assert!(health.status_message.contains("healthy"));
    }
}

// Phoenix tests commented out since Phoenix integration is not currently active
// Uncomment when Phoenix integration is enabled
/*
#[cfg(test)]
mod phoenix_tests {
    use super::super::clients::phoenix::*;
    use super::super::api::DexClient;
    use crate::utils::{DexType, PoolInfo, PoolToken};
    use solana_sdk::pubkey::Pubkey;

    fn create_test_phoenix_client() -> PhoenixClient {
        PhoenixClient::new()
    }

    fn create_mock_phoenix_market() -> PoolInfo {
        PoolInfo {
            address: Pubkey::new_unique(),
            name: "Test Phoenix Market".to_string(),
            token_a: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "SOL".to_string(),
                decimals: 9,
                reserve: 0, // Order books don't have traditional reserves
            },
            token_b: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 0,
            },
            token_a_vault: Pubkey::new_unique(),
            token_b_vault: Pubkey::new_unique(),
            fee_numerator: Some(25),
            fee_denominator: Some(10000),
            fee_rate_bips: Some(25),
            last_update_timestamp: 1640995200,
            dex_type: DexType::Unknown("Phoenix".to_string()),
            liquidity: None,
            sqrt_price: None,
            tick_current_index: None,
            tick_spacing: None,
            tick_array_0: None,
            tick_array_1: None,
            tick_array_2: None,
            oracle: None,
        }
    }

    #[test]
    fn test_phoenix_client_creation() {
        let client = create_test_phoenix_client();
        assert_eq!(client.get_name(), "Phoenix");
    }

    #[test]
    fn test_phoenix_order_side_enum() {
        let bid = OrderSide::Bid;
        let ask = OrderSide::Ask;
        
        match bid {
            OrderSide::Bid => assert!(true),
            OrderSide::Ask => assert!(false),
        }
        
        match ask {
            OrderSide::Bid => assert!(false),
            OrderSide::Ask => assert!(true),
        }
    }

    #[test]
    fn test_phoenix_order_type_enum() {
        let market_order = OrderType::Market;
        let limit_order = OrderType::Limit;
        let _post_only = OrderType::PostOnly;
        let _ioc = OrderType::ImmediateOrCancel;
        
        // Test that we can differentiate order types
        match market_order {
            OrderType::Market => assert!(true),
            _ => assert!(false),
        }
        
        match limit_order {
            OrderType::Limit => assert!(true),
            _ => assert!(false),
        }
    }

    #[test]
    fn test_phoenix_quote_calculation() {
        let client = create_test_phoenix_client();
        let market = create_mock_phoenix_market();
        let input_amount = 1000000;

        let result = client.calculate_onchain_quote(&market, input_amount);
        assert!(result.is_ok());
        
        let quote = result.unwrap();
        assert_eq!(quote.input_amount, input_amount);
        assert!(quote.output_amount > 0);
        assert_eq!(quote.dex, "Phoenix");
        assert!(quote.slippage_estimate.is_some());
        assert!(quote.slippage_estimate.unwrap() > 0.1); // Order books can have higher slippage
    }

    #[tokio::test]
    async fn test_phoenix_health_check() {
        let client = create_test_phoenix_client();
        let result = client.health_check().await;
        assert!(result.is_ok());
        
        let health = result.unwrap();
        assert!(health.is_healthy);
        assert!(health.response_time_ms.is_some());
        assert!(health.status_message.contains("order book"));
    }
}
*/

#[cfg(test)]
mod integration_tests {
    use super::super::*;
    use super::super::clients; // Add direct import for clients module
    use crate::cache::Cache;
    use crate::config::settings::Config;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_get_all_clients_enhanced() {
        let cache = Arc::new(Cache::new("redis://localhost:6379", 300).await.unwrap());
        let config = Arc::new(Config::test_default()); // Fix: Use test_default() instead of default()
        
        let clients = get_all_clients(cache, config);
        
        // Should include all DEX clients including new ones
        assert_eq!(clients.len(), 4); // Orca, Raydium, Meteora, Lifinity
        
        let client_names: Vec<&str> = clients.iter().map(|c| c.get_name()).collect();
        assert!(client_names.contains(&"Orca"));
        assert!(client_names.contains(&"Raydium"));
        assert!(client_names.contains(&"Meteora"));
        assert!(client_names.contains(&"Lifinity"));
    }

    #[tokio::test]
    async fn test_get_all_discoverable_clients_enhanced() {
        let cache = Arc::new(Cache::new("redis://localhost:6379", 300).await.unwrap());
        let config = Arc::new(Config::test_default()); // Fix: Use test_default() instead of default()
        
        let clients = get_all_discoverable_clients(cache, config);
        
        assert_eq!(clients.len(), 4);
        
        for client in &clients {
            assert!(!client.dex_name().is_empty());
        }
    }

    #[tokio::test]
    async fn test_all_clients_health_checks() {
        let cache = Arc::new(Cache::new("redis://localhost:6379", 300).await.unwrap());
        let config = Arc::new(Config::test_default()); // Fix: Use test_default() instead of default()
        
        let clients = get_all_clients(cache, config);
        
        for client in clients {
            let health_result = client.health_check().await;
            assert!(health_result.is_ok(), "Health check failed for {}", client.get_name());
            
            let health = health_result.unwrap();
            assert!(health.is_healthy, "{} reported unhealthy", client.get_name());
            assert!(health.response_time_ms.is_some());
        }
    }

    #[test]
    fn test_enhanced_quote_calculations() {
        use crate::utils::{DexType, PoolInfo, PoolToken};
        use solana_sdk::pubkey::Pubkey;

        // Test each DEX client's quote calculation
        let pool = PoolInfo {
            address: Pubkey::new_unique(),
            name: "Test Pool".to_string(),
            token_a: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "SOL".to_string(),
                decimals: 9,
                reserve: 1000000000,
            },
            token_b: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 150000000,
            },
            token_a_vault: Pubkey::new_unique(),
            token_b_vault: Pubkey::new_unique(),
            fee_rate_bips: Some(25),
            dex_type: DexType::Meteora,
            ..Default::default()
        };

        let input_amount = 1000000;

        // Test Meteora client
        let meteora_client = clients::MeteoraClient::new();
        let meteora_quote = meteora_client.calculate_onchain_quote(&pool, input_amount);
        assert!(meteora_quote.is_ok());

        // Test Lifinity client
        let lifinity_client = clients::LifinityClient::new();
        let lifinity_quote = lifinity_client.calculate_onchain_quote(&pool, input_amount);
        assert!(lifinity_quote.is_ok());

        // Note: Phoenix client test commented out since Phoenix integration is not currently active
        // Uncomment when Phoenix is integrated:
        // let phoenix_client = clients::PhoenixClient::new();
        // let phoenix_quote = phoenix_client.calculate_onchain_quote(&pool, input_amount);
        // assert!(phoenix_quote.is_ok());
    }
}

#[cfg(test)]
mod math_integration_tests {
    use crate::dex::math::{meteora, lifinity};

    #[test]
    fn test_meteora_math_integration() {
        // Test Dynamic AMM calculation
        let result = meteora::calculate_dynamic_amm_output(
            1_000_000,    // 1 token input
            100_000_000,  // 100 tokens reserve
            200_000_000,  // 200 tokens reserve
            25,           // 0.25% fee
            1000,         // Fix: Add missing 5th parameter (max_slippage_bps)
        );
        assert!(result.is_ok());
        assert!(result.unwrap() > 0);

        // Test DLMM calculation
        let dlmm_result = meteora::calculate_dlmm_output(
            1_000_000,  // 1 token input
            8388608,    // Neutral bin (2^23)
            25,         // 0.25% bin step
            25,         // 0.25% fee
            64,         // Fix: Add missing 5th parameter (bin_step)
        );
        assert!(dlmm_result.is_ok());
        assert!(dlmm_result.unwrap() > 0);
    }

    #[test]
    fn test_lifinity_math_integration() {
        // Test without oracle
        let result = lifinity::calculate_lifinity_output(
            1_000_000,    // 1 token input
            100_000_000,  // 100 tokens reserve
            200_000_000,  // 200 tokens reserve
            25,           // 0.25% fee
            None,         // No oracle price
        );
        assert!(result.is_ok());
        assert!(result.unwrap() > 0);

        // Test with oracle
        let oracle_result = lifinity::calculate_lifinity_output(
            1_000_000,
            100_000_000,
            200_000_000,
            25,
            Some(2100000), // Oracle price as u64 (scaled by 1M for precision)
        );
        assert!(oracle_result.is_ok());
        assert!(oracle_result.unwrap() > 0);
    }

    // TODO: Implement dynamic fee calculation for Lifinity
    // #[test]
    // fn test_dynamic_fee_calculation() {
    //     let result = lifinity::calculate_dynamic_fee(
    //         25,        // Base fee 0.25%
    //         Some(0.5), // 50% volatility
    //         Some(1.0), // Normal liquidity
    //     );
    //     assert!(result.is_ok());
    //     
    //     let dynamic_fee = result.unwrap();
    //     assert!(dynamic_fee >= 25); // Should be at least base fee
    //     assert!(dynamic_fee <= 100); // Should not exceed 1%
    // }
}