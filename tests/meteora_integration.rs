//! Comprehensive integration tests for Meteora DEX client
//! Tests both Dynamic AMM and DLMM functionality

use anyhow::Result;
use solana_arb_bot::{
    dex::{
        api::{CommonSwapInfo, DexClient, PoolDiscoverable},
        clients::meteora::{
            MeteoraClient, MeteoraPoolType, METEORA_DLMM_PROGRAM_ID, METEORA_DYNAMIC_AMM_PROGRAM_ID,
        },
        math::meteora::{calculate_dlmm_output, calculate_dynamic_amm_output},
    },
    utils::{DexType, PoolInfo, PoolToken},
};
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use tokio;

/// Test Meteora client initialization
#[tokio::test]
async fn test_meteora_client_initialization() {
    let client = MeteoraClient::new();
    assert_eq!(client.get_name(), "Meteora");
}

/// Test pool type identification for Dynamic AMM pools
#[tokio::test]
async fn test_dynamic_amm_pool_identification() {
    let client = MeteoraClient::new();

    // Create a Dynamic AMM pool (no tick_spacing)
    let pool = create_sample_dynamic_amm_pool();
    let pool_type = client.get_pool_type(&pool);

    assert_eq!(pool_type, MeteoraPoolType::DynamicAmm);
    assert!(pool.tick_spacing.is_none());
}

/// Test pool type identification for DLMM pools
#[tokio::test]
async fn test_dlmm_pool_identification() {
    let client = MeteoraClient::new();

    // Create a DLMM pool (has tick_spacing)
    let pool = create_sample_dlmm_pool();
    let pool_type = client.get_pool_type(&pool);

    assert_eq!(pool_type, MeteoraPoolType::Dlmm);
    assert!(pool.tick_spacing.is_some());
}

/// Test Dynamic AMM quote calculation
#[tokio::test]
async fn test_dynamic_amm_quote_calculation() -> Result<()> {
    let client = MeteoraClient::new();
    let pool = create_sample_dynamic_amm_pool();

    let input_amount = 1_000_000; // 1 USDC
    let quote = client.calculate_onchain_quote(&pool, input_amount)?;

    assert_eq!(quote.input_amount, input_amount);
    assert!(quote.output_amount > 0);
    assert_eq!(quote.dex, "Meteora");
    assert!(quote.slippage_estimate.is_some());
    assert!(quote.slippage_estimate.unwrap() > 0.0);

    println!(
        "Dynamic AMM Quote: {} {} -> {} {}",
        quote.input_amount, quote.input_token, quote.output_amount, quote.output_token
    );

    Ok(())
}

/// Test DLMM quote calculation
#[tokio::test]
async fn test_dlmm_quote_calculation() -> Result<()> {
    let client = MeteoraClient::new();
    let pool = create_sample_dlmm_pool();

    let input_amount = 1_000_000; // 1 USDT
    let quote = client.calculate_onchain_quote(&pool, input_amount)?;

    assert_eq!(quote.input_amount, input_amount);
    assert!(quote.output_amount > 0);
    assert_eq!(quote.dex, "Meteora");
    assert!(quote.slippage_estimate.is_some());

    println!(
        "DLMM Quote: {} {} -> {} {}",
        quote.input_amount, quote.input_token, quote.output_amount, quote.output_token
    );

    Ok(())
}

/// Test Dynamic AMM swap instruction building
#[tokio::test]
async fn test_dynamic_amm_swap_instruction() -> Result<()> {
    let client = MeteoraClient::new();
    let pool = Arc::new(create_sample_dynamic_amm_pool());

    let swap_info = CommonSwapInfo {
        user_wallet_pubkey: Pubkey::new_unique(),
        source_token_mint: pool.token_a.mint,
        destination_token_mint: pool.token_b.mint,
        user_source_token_account: Pubkey::new_unique(),
        user_destination_token_account: Pubkey::new_unique(),
        input_amount: 1_000_000,
        minimum_output_amount: 900_000,
        slippage_bps: Some(100), // 1.0%
        priority_fee_lamports: None,
    };

    let instruction = client
        .get_swap_instruction_enhanced(&swap_info, pool)
        .await?;

    assert_eq!(instruction.program_id, METEORA_DYNAMIC_AMM_PROGRAM_ID);
    assert!(!instruction.accounts.is_empty());
    assert!(!instruction.data.is_empty());

    println!(
        "Dynamic AMM instruction created with {} accounts",
        instruction.accounts.len()
    );

    Ok(())
}

/// Test DLMM swap instruction building
#[tokio::test]
async fn test_dlmm_swap_instruction() -> Result<()> {
    let client = MeteoraClient::new();
    let pool = Arc::new(create_sample_dlmm_pool());

    let swap_info = CommonSwapInfo {
        user_wallet_pubkey: Pubkey::new_unique(),
        source_token_mint: pool.token_a.mint,
        destination_token_mint: pool.token_b.mint,
        user_source_token_account: Pubkey::new_unique(),
        user_destination_token_account: Pubkey::new_unique(),
        input_amount: 1_000_000,
        minimum_output_amount: 950_000,
        slippage_bps: Some(50), // 0.5%
        priority_fee_lamports: None,
    };

    let instruction = client
        .get_swap_instruction_enhanced(&swap_info, pool)
        .await?;

    assert_eq!(instruction.program_id, METEORA_DLMM_PROGRAM_ID);
    assert!(!instruction.accounts.is_empty());
    assert!(!instruction.data.is_empty());

    println!(
        "DLMM instruction created with {} accounts",
        instruction.accounts.len()
    );

    Ok(())
}

/// Test pool discovery functionality
#[tokio::test]
async fn test_pool_discovery() -> Result<()> {
    let client = MeteoraClient::new();
    let pools = DexClient::discover_pools(&client).await?;

    assert!(pools.len() >= 2); // Should have at least Dynamic AMM and DLMM samples

    // Verify we have both pool types
    let mut has_dynamic = false;
    let mut has_dlmm = false;

    for pool in &pools {
        match client.get_pool_type(pool) {
            MeteoraPoolType::DynamicAmm => has_dynamic = true,
            MeteoraPoolType::Dlmm => has_dlmm = true,
        }
    }

    assert!(has_dynamic, "Should discover at least one Dynamic AMM pool");
    assert!(has_dlmm, "Should discover at least one DLMM pool");

    println!(
        "Discovered {} pools: {} Dynamic AMM, {} DLMM",
        pools.len(),
        pools
            .iter()
            .filter(|p| client.get_pool_type(p) == MeteoraPoolType::DynamicAmm)
            .count(),
        pools
            .iter()
            .filter(|p| client.get_pool_type(p) == MeteoraPoolType::Dlmm)
            .count()
    );

    Ok(())
}

/// Test health check functionality
#[tokio::test]
async fn test_health_check() -> Result<()> {
    let client = MeteoraClient::new();
    let health = client.health_check().await?;

    assert!(health.is_healthy);
    assert!(health.response_time_ms.is_some());
    assert_eq!(health.error_count, 0);
    assert!(!health.status_message.is_empty());

    println!("Health check: {}", health.status_message);

    Ok(())
}

/// Test math functions for Dynamic AMM
#[tokio::test]
async fn test_dynamic_amm_math() -> Result<()> {
    let input_amount = 1_000_000;
    let input_reserve = 100_000_000;
    let output_reserve = 500_000_000;
    let base_fee_bps = 25;
    let dynamic_fee_bps = 0;

    let output = calculate_dynamic_amm_output(
        input_amount,
        input_reserve,
        output_reserve,
        base_fee_bps,
        dynamic_fee_bps,
    )?;

    assert!(output > 0);
    assert!(output < input_amount * 5); // Sanity check for 1:5 ratio

    println!("Dynamic AMM: {} input -> {} output", input_amount, output);

    Ok(())
}

/// Test math functions for DLMM
#[tokio::test]
async fn test_dlmm_math() -> Result<()> {
    let input_amount = 1_000_000;
    let active_bin_id = 8388608; // Neutral bin
    let bin_step = 64;
    let liquidity_in_bin = 10_000_000_000u128;
    let fee_rate = 10;

    let output = calculate_dlmm_output(
        input_amount,
        active_bin_id,
        bin_step,
        liquidity_in_bin,
        fee_rate,
    )?;

    assert!(output > 0);

    println!(
        "DLMM: {} input -> {} output (bin {}, step {})",
        input_amount, output, active_bin_id, bin_step
    );

    Ok(())
}

/// Test edge cases for quote calculations
#[tokio::test]
async fn test_quote_edge_cases() -> Result<()> {
    let client = MeteoraClient::new();
    let pool = create_sample_dynamic_amm_pool();

    // Test zero input
    let quote_zero = client.calculate_onchain_quote(&pool, 0)?;
    assert_eq!(quote_zero.output_amount, 0);

    // Test very small input - output amount is always >= 0 for u64
    let quote_small = client.calculate_onchain_quote(&pool, 1)?;

    // Test large input
    let quote_large = client.calculate_onchain_quote(&pool, 1_000_000_000)?;
    assert!(quote_large.output_amount > 0);

    println!(
        "Edge cases: 0 -> {}, 1 -> {}, 1B -> {}",
        quote_zero.output_amount, quote_small.output_amount, quote_large.output_amount
    );

    Ok(())
}

/// Test fetch pool data functionality
#[tokio::test]
async fn test_fetch_pool_data() -> Result<()> {
    let client = MeteoraClient::new();
    let pool_address = Pubkey::new_unique();

    let pool = client.fetch_pool_data(pool_address).await?;

    assert_eq!(pool.address, pool_address);
    assert_eq!(pool.dex_type, DexType::Meteora);
    assert!(pool.fee_numerator.is_some());
    assert!(pool.fee_denominator.is_some());

    println!("Fetched pool: {} ({})", pool.name, pool.address);

    Ok(())
}

/// Performance benchmark for quote calculations
#[tokio::test]
async fn test_quote_performance() -> Result<()> {
    let client = MeteoraClient::new();
    let pool = create_sample_dynamic_amm_pool();
    let input_amount = 1_000_000;

    let start_time = std::time::Instant::now();
    let iterations = 1000;

    for _ in 0..iterations {
        let _ = client.calculate_onchain_quote(&pool, input_amount)?;
    }

    let elapsed = start_time.elapsed();
    let avg_time = elapsed.as_micros() / iterations;

    // Should be fast enough for production use
    assert!(avg_time < 100); // Less than 100 microseconds average

    println!(
        "Performance: {} quotes in {:?} (avg: {}µs per quote)",
        iterations, elapsed, avg_time
    );

    Ok(())
}

// Helper functions

fn create_sample_dynamic_amm_pool() -> PoolInfo {
    PoolInfo {
        address: Pubkey::new_unique(),
        name: "Test Meteora USDC-SOL Dynamic AMM".to_string(),
        token_a: PoolToken {
            mint: solana_sdk::pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"), // USDC
            symbol: "USDC".to_string(),
            decimals: 6,
            reserve: 1_000_000_000, // 1M USDC
        },
        token_b: PoolToken {
            mint: solana_sdk::pubkey!("So11111111111111111111111111111111111111112"), // SOL
            symbol: "SOL".to_string(),
            decimals: 9,
            reserve: 5_000_000_000, // 5K SOL
        },
        token_a_vault: Pubkey::new_unique(),
        token_b_vault: Pubkey::new_unique(),
        fee_numerator: Some(25),
        fee_denominator: Some(10000),
        fee_rate_bips: Some(25), // 0.25%
        last_update_timestamp: chrono::Utc::now().timestamp() as u64,
        dex_type: DexType::Meteora,
        liquidity: Some(500_000_000_000),
        sqrt_price: None,
        tick_current_index: None,
        tick_spacing: None, // Dynamic AMM doesn't use ticks
        tick_array_0: None,
        tick_array_1: None,
        tick_array_2: None,
        oracle: Some(Pubkey::new_unique()),
    }
}

fn create_sample_dlmm_pool() -> PoolInfo {
    PoolInfo {
        address: Pubkey::new_unique(),
        name: "Test Meteora USDT-USDC DLMM".to_string(),
        token_a: PoolToken {
            mint: solana_sdk::pubkey!("Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"), // USDT
            symbol: "USDT".to_string(),
            decimals: 6,
            reserve: 2_000_000_000, // 2M USDT
        },
        token_b: PoolToken {
            mint: solana_sdk::pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"), // USDC
            symbol: "USDC".to_string(),
            decimals: 6,
            reserve: 2_000_000_000, // 2M USDC
        },
        token_a_vault: Pubkey::new_unique(),
        token_b_vault: Pubkey::new_unique(),
        fee_numerator: Some(10),
        fee_denominator: Some(10000),
        fee_rate_bips: Some(10), // 0.1%
        last_update_timestamp: chrono::Utc::now().timestamp() as u64,
        dex_type: DexType::Meteora,
        liquidity: Some(1_000_000_000_000),
        sqrt_price: None,
        tick_current_index: Some(8388608), // Active bin ID
        tick_spacing: Some(64),            // Bin step for DLMM
        tick_array_0: None,
        tick_array_1: None,
        tick_array_2: None,
        oracle: Some(Pubkey::new_unique()),
    }
}
