//! Unit tests for Raydium DEX integration
//! Tests the RaydiumPoolParser and RaydiumClient implementations

#[cfg(test)]
mod tests {
    use super::super::raydium::{RaydiumPoolParser, RaydiumClient, LiquidityStateV4, RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID, RAYDIUM_V4_POOL_STATE_SIZE};
    use crate::dex::quote::{DexClient, SwapInfo};
    use crate::solana::rpc::SolanaRpcClient;
    use crate::utils::PoolParser;
    use solana_sdk::pubkey::Pubkey;
    use std::sync::Arc;
    use std::time::Duration;
    
    #[test]
    fn test_raydium_client_creation() {
        let client = RaydiumClient::new();
        assert_eq!(client.get_name(), "Raydium");
    }

    #[test]
    fn test_raydium_pool_parser_creation() {
        let parser = RaydiumPoolParser;
        assert_eq!(parser.get_program_id(), RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID);
    }

    #[test]
    fn test_liquidity_state_v4_struct_size() {
        // Verify the LiquidityStateV4 struct has the correct size (752 bytes)
        let size = std::mem::size_of::<LiquidityStateV4>();
        assert_eq!(size, RAYDIUM_V4_POOL_STATE_SIZE, 
            "LiquidityStateV4 struct size should be {} bytes, got {}", 
            RAYDIUM_V4_POOL_STATE_SIZE, size);
    }

    #[test]
    fn test_liquidity_state_v4_field_alignment() {
        // Test that we can create and access the struct safely
        let state = LiquidityStateV4 {
            status: 1,
            nonce: 255,
            max_order: 1000,
            depth: 5,
            base_decimal: 6,
            quote_decimal: 6,
            state: 7,
            reset_flag: 0,
            min_size: 100,
            vol_max_cut_ratio: 50,
            amount_wave_ratio: 10,
            base_lot_size: 1000000,
            quote_lot_size: 1000000,
            min_price_multiplier: 900,
            max_price_multiplier: 1100,
            system_decimal_value: 1000000,
            min_separate_numerator: 1,
            min_separate_denominator: 1000,
            trade_fee_numerator: 25,
            trade_fee_denominator: 10000,
            pnl_numerator: 0,
            pnl_denominator: 1,
            swap_fee_numerator: 25,
            swap_fee_denominator: 10000,
            base_need_take_pnl: 0,
            quote_need_take_pnl: 0,
            quote_total_pnl: 0,
            base_total_pnl: 0,
            pool_open_time: 1640995200, // Some timestamp
            punish_pc_amount: 0,
            punish_coin_amount: 0,
            orderbook_to_init_time: 0,
            swap_base_in_amount: 1000000000000u128,
            swap_quote_out_amount: 1000000000000u128,
            swap_base_2_quote_fee: 0,
            swap_quote_in_amount: 1000000000000u128,
            swap_base_out_amount: 1000000000000u128,
            swap_quote_2_base_fee: 0,
            base_vault: Pubkey::new_unique(),
            quote_vault: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            quote_mint: Pubkey::new_unique(),
            lp_mint: Pubkey::new_unique(),
            open_orders: Pubkey::new_unique(),
            market_id: Pubkey::new_unique(),
            market_program_id: Pubkey::new_unique(),
            target_orders: Pubkey::new_unique(),
            withdraw_queue: Pubkey::new_unique(),
            lp_vault: Pubkey::new_unique(),
            owner: Pubkey::new_unique(),
            lp_reserve: 1000000000,
            padding: [0u64; 3],
        };
        
        // Verify key fields are accessible (using local variables to avoid packed field issues)
        let status = state.status;
        let swap_fee_numerator = state.swap_fee_numerator;
        let swap_fee_denominator = state.swap_fee_denominator;
        let base_decimal = state.base_decimal;
        let quote_decimal = state.quote_decimal;
        
        assert_eq!(status, 1);
        assert_eq!(swap_fee_numerator, 25);
        assert_eq!(swap_fee_denominator, 10000);
        assert_eq!(base_decimal, 6);
        assert_eq!(quote_decimal, 6);
    }

    #[test]
    fn test_raydium_quote_calculation_basic() {
        let client = RaydiumClient::new();
        
        // Create a mock pool info with reasonable values
        let mut pool_info = crate::utils::PoolInfo::default();
        pool_info.token_a.reserve = 1000000000; // 1000 tokens with 6 decimals
        pool_info.token_b.reserve = 2000000000; // 2000 tokens with 6 decimals  
        pool_info.fee_numerator = Some(25);      // 0.25% fee
        pool_info.fee_denominator = Some(10000);
        
        // Test quote calculation
        let input_amount = 1000000; // 1 token with 6 decimals
        let result = client.calculate_onchain_quote(&pool_info, input_amount);
        
        assert!(result.is_ok());
        let quote = result.unwrap();
        assert_eq!(quote.input_amount, input_amount);
        assert!(quote.output_amount > 0);
        assert!(quote.output_amount < input_amount * 2); // Should be reasonable ratio
        assert_eq!(quote.dex, "Raydium");
    }

    #[test]
    fn test_raydium_quote_calculation_zero_reserves() {
        let client = RaydiumClient::new();
        let pool_info = crate::utils::PoolInfo::default(); // Default has zero reserves
        
        let result = client.calculate_onchain_quote(&pool_info, 1000000);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("zero reserves"));
    }

    #[test]
    fn test_raydium_quote_calculation_zero_fee_denominator() {
        let client = RaydiumClient::new();
        
        let mut pool_info = crate::utils::PoolInfo::default();
        pool_info.token_a.reserve = 1000000000;
        pool_info.token_b.reserve = 2000000000;
        pool_info.fee_numerator = Some(25);
        pool_info.fee_denominator = Some(0); // Invalid: zero denominator
        
        let result = client.calculate_onchain_quote(&pool_info, 1000000);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("fee denominator cannot be zero"));
    }

    #[test]
    fn test_raydium_swap_instruction_creation() {
        let client = RaydiumClient::new();
        let pool_info = crate::utils::PoolInfo::default();
        let swap_info = SwapInfo {
            dex_name: "Raydium",
            pool: &pool_info,
            user_wallet: Pubkey::new_unique(),
            user_source_token_account: Pubkey::new_unique(),
            user_destination_token_account: Pubkey::new_unique(),
            amount_in: 1000000,
            min_output_amount: 990000,
            pool_account: Pubkey::new_unique(),
            pool_authority: Pubkey::new_unique(),
            pool_open_orders: Pubkey::new_unique(),
            pool_target_orders: Pubkey::new_unique(),
            pool_base_vault: Pubkey::new_unique(),
            pool_quote_vault: Pubkey::new_unique(),
            market_id: Pubkey::new_unique(),
            market_bids: Pubkey::new_unique(),
            market_asks: Pubkey::new_unique(),
            market_event_queue: Pubkey::new_unique(),
            market_program_id: Pubkey::new_unique(),
            market_authority: Pubkey::new_unique(),
            user_owner: Pubkey::new_unique(),
        };
        
        let result = client.get_swap_instruction(&swap_info);
        assert!(result.is_ok());
        
        let instruction = result.unwrap();
        assert_eq!(instruction.program_id, RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID);
        assert_eq!(instruction.accounts.len(), 16); // Expected number of accounts for Raydium V4 swap
        assert!(!instruction.data.is_empty());
        
        // Verify instruction data format
        assert_eq!(instruction.data[0], 9); // Swap instruction discriminator
        assert_eq!(instruction.data.len(), 17); // 1 byte discriminator + 8 bytes amount_in + 8 bytes min_amount_out
    }

    #[tokio::test]
    async fn test_raydium_pool_parser_data_validation() {
        let parser = RaydiumPoolParser;
        let address = Pubkey::new_unique();
        
        // Create a mock RPC client
        let rpc_client = Arc::new(SolanaRpcClient::new(
            "http://localhost:8899",
            vec![],
            3,
            Duration::from_millis(1000),
        ));
        
        // Test with insufficient data
        let insufficient_data = vec![0u8; 100];
        let result = parser.parse_pool_data(address, &insufficient_data, &rpc_client).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid Raydium V4 pool account data length"));
        
        // Test with correct size but inactive pool (status = 0)
        let inactive_pool_data = vec![0u8; RAYDIUM_V4_POOL_STATE_SIZE];
        // Status is at offset 0, and it's already 0 (inactive)
        
        let result = parser.parse_pool_data(address, &inactive_pool_data, &rpc_client).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("appears to be inactive"));
    }

    #[test]
    fn test_raydium_program_id_constant() {
        // Verify the program ID constant is correct
        let expected_program_id = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";
        assert_eq!(RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID.to_string(), expected_program_id);
    }

    // Note: Integration tests with real RPC calls would require:
    // 1. A test environment with actual Solana RPC endpoint
    // 2. Real Raydium V4 pool account data
    // 3. Proper environment setup with valid token accounts
    // This can be added in a separate integration test file when needed
}
