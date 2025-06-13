// src/dex/meteora_test.rs
//! Comprehensive unit tests for Meteora DEX integration.
//! 
//! This module tests both Dynamic AMM and DLMM pool types, ensuring robust
//! parsing, validation, and error handling across all functionality.

use super::meteora::*;
use crate::dex::quote::DexClient;
use crate::solana::rpc::SolanaRpcClient;
use crate::utils::{DexType, PoolParser as UtilsPoolParser};
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::str::FromStr;
use std::time::Duration;

// ====================================================================
// TEST CONSTANTS AND HELPERS
// ====================================================================

/// Test Dynamic AMM program ID
const TEST_DYNAMIC_AMM_PROGRAM_ID: &str = "Eo7WjKq67rjJQSZxS6z3YkapzY3eMj6Xy8X5EQVn5UaB";

/// Test DLMM program ID
const TEST_DLMM_PROGRAM_ID: &str = "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo";

/// Creates a mock DynamicAmmPoolState for testing
fn create_test_dynamic_amm_pool_state() -> DynamicAmmPoolState {
    DynamicAmmPoolState {
        enabled: 1,
        bump: 255,
        pool_type: 1,
        padding1: [0; 5],
        lp_mint: Pubkey::from_str("11111111111111111111111111111112").unwrap(),
        token_a_mint: Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap(), // SOL
        token_b_mint: Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap(), // USDC
        a_vault: Pubkey::from_str("11111111111111111111111111111113").unwrap(),
        b_vault: Pubkey::from_str("11111111111111111111111111111114").unwrap(),
        lp_vault: Pubkey::from_str("11111111111111111111111111111115").unwrap(),
        a_vault_lp: Pubkey::from_str("11111111111111111111111111111116").unwrap(),
        b_vault_lp: Pubkey::from_str("11111111111111111111111111111117").unwrap(),
        a_vault_lp_mint: Pubkey::from_str("11111111111111111111111111111118").unwrap(),
        b_vault_lp_mint: Pubkey::from_str("11111111111111111111111111111119").unwrap(),
        pool_authority: Pubkey::from_str("1111111111111111111111111111111A").unwrap(),
        token_a_fees: Pubkey::from_str("1111111111111111111111111111111B").unwrap(),
        token_b_fees: Pubkey::from_str("1111111111111111111111111111111C").unwrap(),
        oracle: Pubkey::from_str("1111111111111111111111111111111D").unwrap(),
        fee_rate: 300, // 3% (300 basis points)
        protocol_fee_rate: 100, // 1%
        lp_fee_rate: 200, // 2%
        curve_type: 1,
        padding2: [0; 7],
        padding3: [0; 32],
    }
}

/// Creates a mock DlmmLbPairState for testing
fn create_test_dlmm_lb_pair_state() -> DlmmLbPairState {
    DlmmLbPairState {
        active_id: 8388608, // 2^23, representing neutral price
        bin_step: 25, // 25 basis points per bin
        status: 1, // Active
        padding1: 0,
        token_x_mint: Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap(), // SOL
        token_y_mint: Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap(), // USDC
        reserve_x: Pubkey::from_str("11111111111111111111111111111113").unwrap(),
        reserve_y: Pubkey::from_str("11111111111111111111111111111114").unwrap(),
        protocol_fee_x: Pubkey::from_str("11111111111111111111111111111115").unwrap(),
        protocol_fee_y: Pubkey::from_str("11111111111111111111111111111116").unwrap(),
        fee_bps: 25, // 0.25% (25 basis points)
        protocol_share: 1000, // 10% of fees go to protocol
        pair_type: 1,
        padding2: [0; 3],
        oracle: Pubkey::from_str("11111111111111111111111111111117").unwrap(),
        padding3: [0; 64],
    }
}

// ====================================================================
// STRUCT SIZE AND LAYOUT TESTS
// ====================================================================

#[test]
fn test_dynamic_amm_pool_state_size() {
    assert_eq!(
        std::mem::size_of::<DynamicAmmPoolState>(),
        DYNAMIC_AMM_POOL_STATE_SIZE,
        "DynamicAmmPoolState size should match expected constant"
    );
}

#[test]
fn test_dlmm_lb_pair_state_size() {
    assert_eq!(
        std::mem::size_of::<DlmmLbPairState>(),
        DLMM_LB_PAIR_STATE_SIZE,
        "DlmmLbPairState size should match expected constant"
    );
}

#[test]
fn test_dynamic_amm_pool_state_pod_compliance() {
    let state = create_test_dynamic_amm_pool_state();
    
    // Test that we can safely convert to/from bytes
    let bytes = bytemuck::bytes_of(&state);
    assert_eq!(bytes.len(), std::mem::size_of::<DynamicAmmPoolState>());
    
    // Test that we can parse back from bytes
    let parsed_state: &DynamicAmmPoolState = bytemuck::try_from_bytes(bytes)
        .expect("Should be able to parse DynamicAmmPoolState from bytes");
    
    assert_eq!(parsed_state.enabled, state.enabled);
    assert_eq!(parsed_state.token_a_mint, state.token_a_mint);
    assert_eq!(parsed_state.token_b_mint, state.token_b_mint);
    assert_eq!(parsed_state.fee_rate, state.fee_rate);
}

#[test]
fn test_dlmm_lb_pair_state_pod_compliance() {
    let state = create_test_dlmm_lb_pair_state();
    
    // Test that we can safely convert to/from bytes
    let bytes = bytemuck::bytes_of(&state);
    assert_eq!(bytes.len(), std::mem::size_of::<DlmmLbPairState>());
    
    // Test that we can parse back from bytes
    let parsed_state: &DlmmLbPairState = bytemuck::try_from_bytes(bytes)
        .expect("Should be able to parse DlmmLbPairState from bytes");
    
    assert_eq!(parsed_state.active_id, state.active_id);
    assert_eq!(parsed_state.bin_step, state.bin_step);
    assert_eq!(parsed_state.token_x_mint, state.token_x_mint);
    assert_eq!(parsed_state.fee_bps, state.fee_bps);
}

// ====================================================================
// POOL TYPE IDENTIFICATION TESTS
// ====================================================================

#[test]
fn test_identify_dynamic_amm_pool_type() {
    let program_id = Pubkey::from_str(TEST_DYNAMIC_AMM_PROGRAM_ID).unwrap();
    let data_size = DYNAMIC_AMM_POOL_STATE_SIZE;
    
    let pool_type = identify_pool_type(&program_id, data_size)
        .expect("Should identify Dynamic AMM pool type");
    
    assert_eq!(pool_type, MeteoraPoolType::DynamicAmm);
}

#[test]
fn test_identify_dlmm_pool_type() {
    let program_id = Pubkey::from_str(TEST_DLMM_PROGRAM_ID).unwrap();
    let data_size = DLMM_LB_PAIR_STATE_SIZE;
    
    let pool_type = identify_pool_type(&program_id, data_size)
        .expect("Should identify DLMM pool type");
    
    assert_eq!(pool_type, MeteoraPoolType::Dlmm);
}

#[test]
fn test_identify_pool_type_unknown_program_id() {
    let unknown_program_id = Pubkey::from_str("11111111111111111111111111111112").unwrap();
    let data_size = DYNAMIC_AMM_POOL_STATE_SIZE;
    
    let result = identify_pool_type(&unknown_program_id, data_size);
    
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Unknown Meteora program ID"));
}

#[test]
fn test_identify_pool_type_insufficient_data_size() {
    let program_id = Pubkey::from_str(TEST_DYNAMIC_AMM_PROGRAM_ID).unwrap();
    let insufficient_size = DYNAMIC_AMM_POOL_STATE_SIZE - 1;
    
    let result = identify_pool_type(&program_id, insufficient_size);
    
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("data size"));
}

// ====================================================================
// PARSER IMPLEMENTATION TESTS
// ====================================================================

#[test]
fn test_meteora_pool_parser_program_id() {
    let parser = MeteoraPoolParser;
    let expected_program_id = Pubkey::from_str(METEORA_DYNAMIC_AMM_PROGRAM_ID).unwrap();
    
    assert_eq!(parser.get_program_id(), expected_program_id);
}

#[test]
fn test_empty_data_handling() {
    let parser = MeteoraPoolParser;
    let pool_address = Pubkey::from_str("11111111111111111111111111111112").unwrap();
    let empty_data = &[];
    
    // Create a minimal mock RPC client
    let rpc_client = Arc::new(SolanaRpcClient::new(
        "https://api.mainnet-beta.solana.com",
        vec![],
        3,
        Duration::from_millis(1000),
    ));
    
    // This should return an error for empty data
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = runtime.block_on(async {
        parser.parse_pool_data(pool_address, empty_data, &rpc_client).await
    });
    
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Empty pool data"));
}

// ====================================================================
// DEX CLIENT TESTS
// ====================================================================

#[test]
fn test_meteora_client_creation() {
    let client = MeteoraClient::new();
    assert_eq!(client.get_name(), "Meteora");
}

#[test]
fn test_meteora_client_default() {
    let client = MeteoraClient::default();
    assert_eq!(client.get_name(), "Meteora");
}

#[test]
fn test_swap_instruction_not_implemented() {
    use crate::dex::quote::SwapInfo;
    use crate::utils::{PoolInfo, PoolToken};
    
    let client = MeteoraClient::new();
    let pool = PoolInfo {
        address: Pubkey::from_str("11111111111111111111111111111112").unwrap(),
        name: "Test Pool".to_string(),
        token_a: PoolToken {
            mint: Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap(),
            symbol: "SOL".to_string(),
            decimals: 9,
            reserve: 1_000_000,
        },
        token_b: PoolToken {
            mint: Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap(),
            symbol: "USDC".to_string(),
            decimals: 6,
            reserve: 1_000_000,
        },
        token_a_vault: Pubkey::from_str("11111111111111111111111111111113").unwrap(),
        token_b_vault: Pubkey::from_str("11111111111111111111111111111114").unwrap(),
        fee_numerator: Some(25),
        fee_denominator: Some(10000),
        fee_rate_bips: Some(25),
        last_update_timestamp: 0,
        dex_type: DexType::Meteora,
        liquidity: None,
        sqrt_price: None,
        tick_current_index: None,
        tick_spacing: None,
    };
    
    let swap_info = SwapInfo {
        dex_name: "Meteora",
        pool: &pool,
        user_wallet: Pubkey::from_str("11111111111111111111111111111115").unwrap(),
        user_source_token_account: Pubkey::from_str("11111111111111111111111111111116").unwrap(),
        user_destination_token_account: Pubkey::from_str("11111111111111111111111111111117").unwrap(),
        amount_in: 1_000_000,
        min_output_amount: 990_000,
        pool_account: pool.address,
        pool_authority: Pubkey::from_str("11111111111111111111111111111118").unwrap(),
        pool_open_orders: Pubkey::from_str("11111111111111111111111111111119").unwrap(),
        pool_target_orders: Pubkey::from_str("1111111111111111111111111111111A").unwrap(),
        pool_base_vault: pool.token_a_vault,
        pool_quote_vault: pool.token_b_vault,
        market_id: Pubkey::from_str("1111111111111111111111111111111B").unwrap(),
        market_bids: Pubkey::from_str("1111111111111111111111111111111C").unwrap(),
        market_asks: Pubkey::from_str("1111111111111111111111111111111D").unwrap(),
        market_event_queue: Pubkey::from_str("1111111111111111111111111111111E").unwrap(),
        market_program_id: Pubkey::from_str("1111111111111111111111111111111F").unwrap(),
        market_authority: Pubkey::from_str("1111111111111111111111111111111G").unwrap(),
        user_owner: Pubkey::from_str("1111111111111111111111111111111H").unwrap(),
    };
    
    let result = client.get_swap_instruction(&swap_info);
    
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Unable to determine Meteora pool type"));
}

// ====================================================================
// QUOTE CALCULATION TESTS
// ====================================================================

#[test]
fn test_quote_calculation_zero_reserves() {
    use crate::utils::{PoolInfo, PoolToken};
    
    let client = MeteoraClient::new();
    let pool = PoolInfo {
        address: Pubkey::from_str("11111111111111111111111111111112").unwrap(),
        name: "Test Pool".to_string(),
        token_a: PoolToken {
            mint: Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap(),
            symbol: "SOL".to_string(),
            decimals: 9,
            reserve: 0, // Zero reserves should cause error
        },
        token_b: PoolToken {
            mint: Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap(),
            symbol: "USDC".to_string(),
            decimals: 6,
            reserve: 1_000_000,
        },
        token_a_vault: Pubkey::from_str("11111111111111111111111111111113").unwrap(),
        token_b_vault: Pubkey::from_str("11111111111111111111111111111114").unwrap(),
        fee_numerator: Some(25),
        fee_denominator: Some(10000),
        fee_rate_bips: Some(25),
        last_update_timestamp: 0,
        dex_type: DexType::Meteora,
        liquidity: None,
        sqrt_price: None,
        tick_current_index: None,
        tick_spacing: None,
    };
    
    let result = client.calculate_onchain_quote(&pool, 1_000_000);
    
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("insufficient reserves"));
}

#[test]
fn test_quote_calculation_valid_pool() {
    use crate::utils::{PoolInfo, PoolToken};
    
    let client = MeteoraClient::new();
    let pool = PoolInfo {
        address: Pubkey::from_str("11111111111111111111111111111112").unwrap(),
        name: "Test Pool".to_string(),
        token_a: PoolToken {
            mint: Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap(),
            symbol: "SOL".to_string(),
            decimals: 9,
            reserve: 1_000_000_000, // 1 SOL
        },
        token_b: PoolToken {
            mint: Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap(),
            symbol: "USDC".to_string(),
            decimals: 6,
            reserve: 100_000_000, // 100 USDC
        },
        token_a_vault: Pubkey::from_str("11111111111111111111111111111113").unwrap(),
        token_b_vault: Pubkey::from_str("11111111111111111111111111111114").unwrap(),
        fee_numerator: Some(25),
        fee_denominator: Some(10000),
        fee_rate_bips: Some(25), // 0.25%
        last_update_timestamp: 0,
        dex_type: DexType::Meteora,
        liquidity: None,
        sqrt_price: None,
        tick_current_index: None,
        tick_spacing: None,
    };
    
    let input_amount = 100_000_000; // 0.1 SOL
    let quote = client.calculate_onchain_quote(&pool, input_amount)
        .expect("Should calculate quote successfully");
    
    assert_eq!(quote.input_amount, input_amount);
    assert_eq!(quote.dex, "Meteora");
    assert_eq!(quote.route.len(), 1);
    assert!(quote.output_amount > 0);
    assert!(quote.slippage_estimate.is_some());
}

// ====================================================================
// FIELD VALIDATION TESTS
// ====================================================================

#[test]
fn test_dynamic_amm_pool_state_field_alignment() {
    let state = create_test_dynamic_amm_pool_state();
    
    // Test that all important fields are accessible and have expected values
    assert_eq!(state.enabled, 1);
    assert_eq!(state.pool_type, 1);
    assert_eq!(state.fee_rate, 300);
    assert_eq!(state.protocol_fee_rate, 100);
    assert_eq!(state.lp_fee_rate, 200);
    assert_eq!(state.curve_type, 1);
    
    // Test that Pubkey fields are correctly set
    assert_ne!(state.token_a_mint, Pubkey::default());
    assert_ne!(state.token_b_mint, Pubkey::default());
    assert_ne!(state.a_vault, Pubkey::default());
    assert_ne!(state.b_vault, Pubkey::default());
}

#[test]
fn test_dlmm_lb_pair_state_field_alignment() {
    let state = create_test_dlmm_lb_pair_state();
    
    // Test that all important fields are accessible and have expected values
    assert_eq!(state.active_id, 8388608);
    assert_eq!(state.bin_step, 25);
    assert_eq!(state.status, 1);
    assert_eq!(state.fee_bps, 25);
    assert_eq!(state.protocol_share, 1000);
    assert_eq!(state.pair_type, 1);
    
    // Test that Pubkey fields are correctly set
    assert_ne!(state.token_x_mint, Pubkey::default());
    assert_ne!(state.token_y_mint, Pubkey::default());
    assert_ne!(state.reserve_x, Pubkey::default());
    assert_ne!(state.reserve_y, Pubkey::default());
}

#[test]
fn test_struct_zero_initialization() {
    // Test that structs can be zero-initialized (Zeroable trait requirement)
    let zero_dynamic_amm: DynamicAmmPoolState = bytemuck::Zeroable::zeroed();
    assert_eq!(zero_dynamic_amm.enabled, 0);
    assert_eq!(zero_dynamic_amm.fee_rate, 0);
    assert_eq!(zero_dynamic_amm.token_a_mint, Pubkey::default());
    
    let zero_dlmm: DlmmLbPairState = bytemuck::Zeroable::zeroed();
    assert_eq!(zero_dlmm.active_id, 0);
    assert_eq!(zero_dlmm.bin_step, 0);
    assert_eq!(zero_dlmm.token_x_mint, Pubkey::default());
}

// ====================================================================
// INTEGRATION AND STRESS TESTS
// ====================================================================

#[test]
fn test_multiple_pool_type_handling() {
    // Test that we can handle multiple pool types in sequence
    let dynamic_amm_program_id = Pubkey::from_str(TEST_DYNAMIC_AMM_PROGRAM_ID).unwrap();
    let dlmm_program_id = Pubkey::from_str(TEST_DLMM_PROGRAM_ID).unwrap();
    
    let dynamic_amm_type = identify_pool_type(&dynamic_amm_program_id, DYNAMIC_AMM_POOL_STATE_SIZE)
        .expect("Should identify Dynamic AMM");
    let dlmm_type = identify_pool_type(&dlmm_program_id, DLMM_LB_PAIR_STATE_SIZE)
        .expect("Should identify DLMM");
    
    assert_eq!(dynamic_amm_type, MeteoraPoolType::DynamicAmm);
    assert_eq!(dlmm_type, MeteoraPoolType::Dlmm);
    assert_ne!(dynamic_amm_type, dlmm_type);
}

#[test]
fn test_struct_memory_layout_consistency() {
    // Ensure that the structs maintain consistent memory layout across compilations
    let dynamic_amm_state = create_test_dynamic_amm_pool_state();
    let dlmm_state = create_test_dlmm_lb_pair_state();
    
    // Convert to bytes and back to ensure layout consistency
    let dynamic_amm_bytes = bytemuck::bytes_of(&dynamic_amm_state);
    let dlmm_bytes = bytemuck::bytes_of(&dlmm_state);
    
    let parsed_dynamic_amm: &DynamicAmmPoolState = bytemuck::try_from_bytes(dynamic_amm_bytes)
        .expect("Dynamic AMM struct should parse consistently");
    let parsed_dlmm: &DlmmLbPairState = bytemuck::try_from_bytes(dlmm_bytes)
        .expect("DLMM struct should parse consistently");
    
    // Verify critical fields match
    assert_eq!(parsed_dynamic_amm.token_a_mint, dynamic_amm_state.token_a_mint);
    assert_eq!(parsed_dynamic_amm.fee_rate, dynamic_amm_state.fee_rate);
    assert_eq!(parsed_dlmm.active_id, dlmm_state.active_id);
    assert_eq!(parsed_dlmm.fee_bps, dlmm_state.fee_bps);
}
