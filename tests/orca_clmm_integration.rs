//! Integration tests for Orca CLMM (Whirlpool) implementation
//! 
//! These tests verify that the Orca client correctly uses CLMM math
//! for accurate quote calculations and swap instruction building.

use solana_arb_bot::dex::api::{DexClient, CommonSwapInfo};
use solana_arb_bot::dex::clients::orca::OrcaClient;
use solana_arb_bot::dex::math::orca::{
    calculate_whirlpool_swap_output, validate_pool_state, Q64
};
use solana_arb_bot::utils::{PoolInfo, PoolToken, DexType};
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

/// Create a test Whirlpool with realistic CLMM parameters
fn create_test_whirlpool() -> PoolInfo {
    PoolInfo {
        address: Pubkey::new_unique(),
        name: "Test SOL/USDC Whirlpool".to_string(),
        token_a: PoolToken {
            mint: solana_sdk::pubkey!("So11111111111111111111111111111111111111112"), // SOL
            symbol: "SOL".to_string(),
            decimals: 9,
            reserve: 1000000000000, // 1M SOL (mock reserve for validation)
        },
        token_b: PoolToken {
            mint: solana_sdk::pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"), // USDC
            symbol: "USDC".to_string(),
            decimals: 6,
            reserve: 100000000000000, // 100M USDC (mock reserve)
        },
        token_a_vault: Pubkey::new_unique(),
        token_b_vault: Pubkey::new_unique(),
        fee_numerator: Some(300),
        fee_denominator: Some(1000000),
        fee_rate_bips: Some(30), // 0.3%
        last_update_timestamp: 1672531200,
        dex_type: DexType::Orca,
        // CLMM-specific fields
        liquidity: Some(5000000000000), // 5M liquidity
        sqrt_price: Some(Q64 * 10), // sqrt(100) = 10, so price ~100 
        tick_current_index: Some(46054), // Tick for price ~100
        tick_spacing: Some(64),
        tick_array_0: None,
        tick_array_1: None,
        tick_array_2: None,
        oracle: None,
    }
}

#[tokio::test]
async fn test_orca_clmm_quote_calculation() {
    let client = OrcaClient::new();
    let pool = create_test_whirlpool();
    
    // Test quote calculation with realistic amounts
    let input_amount = 1_000_000_000; // 1 SOL
    let quote = client.calculate_onchain_quote(&pool, input_amount).unwrap();
    
    // Validate quote structure
    assert_eq!(quote.input_token, "SOL");
    assert_eq!(quote.output_token, "USDC"); 
    assert_eq!(quote.input_amount, input_amount);
    assert!(quote.output_amount > 0, "Output amount should be positive");
    assert!(quote.slippage_estimate.is_some(), "Slippage estimate should be provided");
    assert_eq!(quote.dex, "Orca");
    assert_eq!(quote.route.len(), 1);
    
    // Validate that output is reasonable (CLMM math with current pool setup)
    let expected_output_range = 950_000_000..1_050_000_000; // 950-1050 USDC (6 decimals)
    assert!(
        expected_output_range.contains(&quote.output_amount),
        "Output amount {} not in expected range {:?}",
        quote.output_amount,
        expected_output_range
    );
    
    println!("✅ CLMM Quote: {} SOL -> {} USDC (slippage: {:.4}%)", 
        input_amount as f64 / 1e9,
        quote.output_amount as f64 / 1e6,
        quote.slippage_estimate.unwrap() * 100.0
    );
}

#[tokio::test]
async fn test_orca_clmm_math_accuracy() {
    let pool = create_test_whirlpool();
    
    // Test the underlying CLMM math directly
    let input_amount = 500_000_000; // 0.5 SOL
    let sqrt_price = pool.sqrt_price.unwrap();
    let liquidity = pool.liquidity.unwrap();
    let tick_current = pool.tick_current_index.unwrap();
    let tick_spacing = pool.tick_spacing.unwrap();
    let fee_rate = pool.fee_rate_bips.unwrap();
    
    let swap_result = calculate_whirlpool_swap_output(
        input_amount,
        sqrt_price,
        liquidity,
        tick_current,
        tick_spacing,
        fee_rate,
        true, // A to B (SOL -> USDC)
    ).unwrap();
    
    // Validate CLMM calculation results
    assert!(swap_result.output_amount > 0, "Output should be positive");
    assert!(swap_result.fee_amount > 0, "Fee should be deducted");
    assert!(swap_result.price_impact >= 0.0, "Price impact should be non-negative");
    assert!(swap_result.price_impact < 0.1, "Price impact should be reasonable (<10%)");
    
    // Validate fee calculation (should be ~0.3% of input)
    let expected_fee = (input_amount as f64 * 0.003) as u64;
    let fee_tolerance = expected_fee / 10; // 10% tolerance
    assert!(
        (swap_result.fee_amount as i64 - expected_fee as i64).abs() < fee_tolerance as i64,
        "Fee amount {} not close to expected {}",
        swap_result.fee_amount,
        expected_fee
    );
    
    println!("✅ CLMM Math: {} SOL -> {} USDC (fee: {} SOL, impact: {:.4}%)",
        input_amount as f64 / 1e9,
        swap_result.output_amount as f64 / 1e6,
        swap_result.fee_amount as f64 / 1e9,
        swap_result.price_impact * 100.0
    );
}

#[tokio::test]
async fn test_orca_pool_state_validation() {
    let mut pool = create_test_whirlpool();
    
    // Test valid pool state
    assert!(validate_pool_state(
        pool.sqrt_price,
        pool.liquidity,
        pool.tick_current_index,
        pool.tick_spacing,
    ).is_ok());
    
    // Test invalid sqrt_price
    pool.sqrt_price = Some(0);
    assert!(validate_pool_state(
        pool.sqrt_price,
        pool.liquidity,
        pool.tick_current_index,
        pool.tick_spacing,
    ).is_err());
    
    // Test missing liquidity
    pool.sqrt_price = Some(Q64);
    pool.liquidity = None;
    assert!(validate_pool_state(
        pool.sqrt_price,
        pool.liquidity,
        pool.tick_current_index,
        pool.tick_spacing,
    ).is_err());
    
    println!("✅ Pool state validation working correctly");
}

#[tokio::test]
async fn test_orca_swap_instruction_building() {
    let client = OrcaClient::new();
    let pool = Arc::new(create_test_whirlpool());
    
    let swap_info = CommonSwapInfo {
        user_wallet_pubkey: Pubkey::new_unique(),
        user_source_token_account: Pubkey::new_unique(),
        user_destination_token_account: Pubkey::new_unique(),
        source_token_mint: pool.token_a.mint,
        destination_token_mint: pool.token_b.mint,
        input_amount: 1_000_000_000,
        minimum_output_amount: 90_000_000,
        slippage_bps: Some(500), // 5%
        priority_fee_lamports: Some(5000),
    };
    
    // Test enhanced swap instruction building
    let instruction = client
        .get_swap_instruction_enhanced(&swap_info, pool.clone())
        .await
        .unwrap();
    
    // Validate instruction structure
    assert_eq!(instruction.program_id, solana_arb_bot::dex::clients::orca::ORCA_WHIRLPOOL_PROGRAM_ID);
    assert!(instruction.accounts.len() >= 10, "Should have all required accounts");
    assert!(!instruction.data.is_empty(), "Instruction data should not be empty");
    
    // Validate account structure
    let accounts = &instruction.accounts;
    assert!(!accounts[0].is_signer, "Token program should not be signer");
    assert!(accounts[1].is_signer, "Payer should be signer");
    assert!(!accounts[2].is_signer, "Pool account should not be signer");
    
    println!("✅ Swap instruction built successfully with {} accounts and {} bytes of data",
        instruction.accounts.len(),
        instruction.data.len()
    );
}

#[tokio::test]
async fn test_orca_different_swap_amounts() {
    let client = OrcaClient::new();
    let pool = create_test_whirlpool();
    
    // Test different swap amounts to verify scaling
    let test_amounts = vec![
        100_000_000,   // 0.1 SOL
        1_000_000_000, // 1 SOL  
        10_000_000_000, // 10 SOL
    ];
    
    let mut previous_output = 0;
    let mut previous_impact = 0.0;
    
    for amount in test_amounts {
        let quote = client.calculate_onchain_quote(&pool, amount).unwrap();
        
        assert!(quote.output_amount > 0, "Output should be positive for amount {}", amount);
        assert!(quote.output_amount > previous_output, "Larger input should yield larger output");
        
        let impact = quote.slippage_estimate.unwrap();
        assert!(impact >= previous_impact, "Larger swaps should have equal or higher price impact");
        
        println!("Amount: {} SOL -> {} USDC (impact: {:.4}%)",
            amount as f64 / 1e9,
            quote.output_amount as f64 / 1e6,
            impact * 100.0
        );
        
        previous_output = quote.output_amount;
        previous_impact = impact;
    }
    
    println!("✅ Different swap amounts scale correctly");
}

#[tokio::test]
async fn test_orca_reverse_swap_direction() {
    let client = OrcaClient::new();
    let mut pool = create_test_whirlpool();
    
    // Test USDC -> SOL swap (reverse direction)
    // Swap the tokens to simulate B->A direction
    let original_token_a = pool.token_a.clone();
    let original_token_b = pool.token_b.clone();
    pool.token_a = original_token_b;
    pool.token_b = original_token_a;
    
    let input_amount = 100_000_000; // 100 USDC
    let quote = client.calculate_onchain_quote(&pool, input_amount).unwrap();
    
    assert_eq!(quote.input_token, "USDC");
    assert_eq!(quote.output_token, "SOL");
    assert!(quote.output_amount > 0, "Reverse swap should produce output");
    
    // Should get roughly 0.1 SOL for 100 USDC based on current CLMM math
    let expected_sol_output = 80_000_000..120_000_000; // 0.08-0.12 SOL
    assert!(
        expected_sol_output.contains(&quote.output_amount),
        "Reverse swap output {} not in expected range {:?}",
        quote.output_amount,
        expected_sol_output
    );
    
    println!("✅ Reverse swap: {} USDC -> {} SOL",
        input_amount as f64 / 1e6,
        quote.output_amount as f64 / 1e9
    );
}
