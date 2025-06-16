//! Raydium V4 AMM Integration Tests
//! 
//! This test suite validates the complete Raydium integration including:
//! - Production-grade math calculations
//! - Swap instruction building
//! - Pool discovery and parsing
//! - Error handling and edge cases

use solana_arb_bot::{
    dex::{
        api::{DexClient, CommonSwapInfo},
        clients::raydium::{RaydiumClient, RaydiumPoolParser, RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID},
        math::raydium,
    },
    utils::{PoolInfo, PoolToken, DexType, PoolParser},
};
use anyhow::Result;
use solana_sdk::pubkey::Pubkey;
use std::{str::FromStr, sync::Arc};

/// Test basic Raydium client functionality
#[tokio::test]
async fn test_raydium_client_basic_functionality() -> Result<()> {
    let client = RaydiumClient::new();
    assert_eq!(client.get_name(), "Raydium");
    Ok(())
}

/// Test Raydium math calculations with various scenarios
#[tokio::test]
async fn test_raydium_math_calculations() -> Result<()> {
    // Test normal swap calculation
    let result = raydium::calculate_raydium_swap_output(
        1_000_000,    // 1 token input (6 decimals)
        100_000_000,  // 100 tokens in pool
        200_000_000,  // 200 tokens in pool
        25,           // 0.25% fee (25 basis points)
        10_000,       // fee denominator
    )?;

    assert!(result.output_amount > 0);
    assert!(result.fee_amount > 0);
    assert!(result.price_impact >= 0.0);
    assert!(result.price_impact < 0.1); // Should be reasonable for this trade size

    // Verify fee calculation (should be 0.25% of input)
    let expected_fee = 1_000_000 * 25 / 10_000;
    assert!((result.fee_amount as i64 - expected_fee as i64).abs() < 10);

    println!("✅ Raydium swap calculation - Input: {}, Output: {}, Fee: {}, Price Impact: {:.4}%", 
        1_000_000, result.output_amount, result.fee_amount, result.price_impact * 100.0);

    Ok(())
}

/// Test Raydium reverse calculation (input needed for specific output)
#[tokio::test]
async fn test_raydium_reverse_calculation() -> Result<()> {
    let result = raydium::calculate_raydium_input_for_output(
        500_000,      // Want 0.5 tokens output
        100_000_000,  // 100 tokens in pool
        200_000_000,  // 200 tokens in pool  
        25,           // 0.25% fee
        10_000,       // fee denominator
    )?;

    assert_eq!(result.output_amount, 500_000);
    assert!(result.fee_amount > 0);
    assert!(result.price_impact >= 0.0);

    println!("✅ Raydium reverse calculation - Target Output: {}, Required Input: {}, Fee: {}", 
        500_000, result.output_amount + result.fee_amount, result.fee_amount);

    Ok(())
}

/// Test pool validation
#[tokio::test]
async fn test_raydium_pool_validation() -> Result<()> {
    // Valid pool state
    assert!(raydium::validate_pool_state(1_000_000, 2_000_000, 25, 10_000).is_ok());

    // Invalid states should fail
    assert!(raydium::validate_pool_state(0, 1_000_000, 25, 10_000).is_err()); // Zero reserve
    assert!(raydium::validate_pool_state(1_000_000, 0, 25, 10_000).is_err()); // Zero reserve
    assert!(raydium::validate_pool_state(1_000_000, 1_000_000, 25, 0).is_err()); // Zero denominator
    assert!(raydium::validate_pool_state(1_000_000, 1_000_000, 10_000, 10_000).is_err()); // 100% fee

    println!("✅ Raydium pool validation tests passed");
    Ok(())
}

/// Test slippage calculations
#[tokio::test]
async fn test_raydium_slippage_calculations() -> Result<()> {
    // Test slippage calculation
    let slippage = raydium::calculate_slippage(1_000_000, 990_000, 980_000)?;
    assert!((slippage - 0.0101).abs() < 0.001); // Should be ~1.01%

    // Test minimum output with slippage protection
    let min_output = raydium::calculate_minimum_output_with_slippage(1_000_000, 500); // 5% slippage
    assert_eq!(min_output, 950_000); // Should be 95% of expected

    println!("✅ Raydium slippage calculation - Expected: 1000000, Actual: 980000, Slippage: {:.4}%", slippage * 100.0);
    Ok(())
}

/// Test quote calculation through client
#[tokio::test]
async fn test_raydium_client_quote_calculation() -> Result<()> {
    let client = RaydiumClient::new();
    
    // Create mock pool info for testing
    let mock_pool = PoolInfo {
        address: Pubkey::from_str("11111111111111111111111111111112")?,
        name: "Test Raydium Pool".to_string(),
        dex_type: DexType::Raydium,
        token_a: PoolToken {
            mint: Pubkey::from_str("So11111111111111111111111111111111111111112")?, // SOL
            symbol: "SOL".to_string(),
            decimals: 9,
            reserve: 1_000_000_000_000, // 1000 SOL
        },
        token_b: PoolToken {
            mint: Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v")?, // USDC
            symbol: "USDC".to_string(),
            decimals: 6,
            reserve: 100_000_000_000, // 100k USDC
        },
        token_a_vault: Pubkey::from_str("11111111111111111111111111111113")?,
        token_b_vault: Pubkey::from_str("11111111111111111111111111111114")?,
        fee_numerator: Some(25),
        fee_denominator: Some(10_000),
        fee_rate_bips: Some(25),
        last_update_timestamp: 0,
        sqrt_price: None,
        liquidity: None,
        tick_current_index: None,
        tick_spacing: None,
        tick_array_0: None,
        tick_array_1: None,
        tick_array_2: None,
        oracle: None,
    };

    // Test quote calculation
    let quote = client.calculate_onchain_quote(&mock_pool, 1_000_000_000)?; // 1 SOL
    
    assert!(quote.output_amount > 0);
    assert_eq!(quote.input_amount, 1_000_000_000);
    assert_eq!(quote.dex, "Raydium");
    assert!(quote.slippage_estimate.is_some());
    assert!(quote.slippage_estimate.unwrap() >= 0.0);

    println!("✅ Raydium quote - Input: {} SOL, Output: {} USDC, Slippage: {:.4}%", 
        quote.input_amount as f64 / 1e9, 
        quote.output_amount as f64 / 1e6,
        quote.slippage_estimate.unwrap() * 100.0);

    Ok(())
}

/// Test swap instruction building
#[tokio::test]
async fn test_raydium_swap_instruction_building() -> Result<()> {
    let client = RaydiumClient::new();
    
    // Create mock pool info
    let mock_pool = PoolInfo {
        address: Pubkey::from_str("58oQChx4yWmvKdwLLZzBi4ChoCc2fqCUWBkwMihLYQo2")?, // Real Raydium SOL/USDC pool
        name: "Raydium SOL/USDC".to_string(),
        dex_type: DexType::Raydium,
        token_a: PoolToken {
            mint: Pubkey::from_str("So11111111111111111111111111111111111111112")?, // SOL
            symbol: "SOL".to_string(),
            decimals: 9,
            reserve: 1_000_000_000_000,
        },
        token_b: PoolToken {
            mint: Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v")?, // USDC
            symbol: "USDC".to_string(),
            decimals: 6,
            reserve: 100_000_000_000,
        },
        token_a_vault: Pubkey::from_str("11111111111111111111111111111113")?,
        token_b_vault: Pubkey::from_str("11111111111111111111111111111114")?,
        fee_numerator: Some(25),
        fee_denominator: Some(10_000),
        fee_rate_bips: Some(25),
        last_update_timestamp: 0,
        sqrt_price: None,
        liquidity: None,
        tick_current_index: None,
        tick_spacing: None,
        tick_array_0: None,
        tick_array_1: None,
        tick_array_2: None,
        oracle: None,
    };

    // Create swap info
    let swap_info = CommonSwapInfo {
        user_wallet_pubkey: Pubkey::from_str("11111111111111111111111111111111")?,
        source_token_mint: mock_pool.token_a.mint,
        destination_token_mint: mock_pool.token_b.mint,
        user_source_token_account: Pubkey::from_str("11111111111111111111111111111115")?,
        user_destination_token_account: Pubkey::from_str("11111111111111111111111111111116")?,
        input_amount: 1_000_000_000, // 1 SOL
        minimum_output_amount: 90_000_000, // 90 USDC minimum
        slippage_bps: Some(500), // 5% slippage tolerance
        priority_fee_lamports: Some(5000), // 0.000005 SOL priority fee
    };

    // Test instruction building
    let instruction = client.get_swap_instruction_enhanced(&swap_info, Arc::new(mock_pool)).await?;
    
    assert_eq!(instruction.program_id, RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID);
    assert!(instruction.accounts.len() >= 10); // Should have all required accounts
    assert!(!instruction.data.is_empty()); // Should have instruction data
    assert_eq!(instruction.data[0], 9); // Raydium swap discriminator

    println!("✅ Raydium swap instruction built - {} accounts, {} bytes data", 
        instruction.accounts.len(), instruction.data.len());

    Ok(())
}

/// Test pool parser program ID
#[tokio::test]
async fn test_raydium_pool_parser() -> Result<()> {
    let parser = RaydiumPoolParser;
    assert_eq!(parser.get_program_id(), RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID);
    
    println!("✅ Raydium pool parser program ID: {}", parser.get_program_id());
    Ok(())
}

/// Test error handling for invalid inputs
#[tokio::test]
async fn test_raydium_error_handling() -> Result<()> {
    // Test zero input amount
    let result = raydium::calculate_raydium_swap_output(0, 1_000_000, 1_000_000, 25, 10_000);
    assert!(result.is_err());

    // Test zero reserves
    let result = raydium::calculate_raydium_swap_output(1_000, 0, 1_000_000, 25, 10_000);
    assert!(result.is_err());

    let result = raydium::calculate_raydium_swap_output(1_000, 1_000_000, 0, 25, 10_000);
    assert!(result.is_err());

    // Test invalid fee denominator
    let result = raydium::calculate_raydium_swap_output(1_000, 1_000_000, 1_000_000, 25, 0);
    assert!(result.is_err());

    println!("✅ Raydium error handling tests passed");
    Ok(())
}
