//! Comprehensive integration tests for Lifinity DEX client
//! Tests proactive market making and oracle integration

use anyhow::Result;
use solana_arb_bot::{
    dex::{
        api::{CommonSwapInfo, DexClient, PoolDiscoverable},
        clients::lifinity::{LifinityClient, LIFINITY_PROGRAM_ID},
        math::lifinity::calculate_lifinity_output,
    },
    utils::{DexType, PoolInfo, PoolToken},
};
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use tokio;

/// Test Lifinity client initialization
#[tokio::test]
async fn test_lifinity_client_initialization() {
    let client = LifinityClient::new();
    assert_eq!(client.get_name(), "Lifinity");
}

/// Test oracle price calculation functionality
#[tokio::test]
async fn test_oracle_price_calculation() -> Result<()> {
    let client = LifinityClient::new();
    let pool = create_sample_lifinity_pool();

    let oracle_price = client.calculate_oracle_price(&pool);

    assert!(oracle_price.is_some());
    assert!(oracle_price.unwrap() > 0.0);

    println!("Oracle price: {:.6}", oracle_price.unwrap());

    Ok(())
}

/// Test oracle price with zero reserves
#[tokio::test]
async fn test_oracle_price_zero_reserves() -> Result<()> {
    let client = LifinityClient::new();
    let mut pool = create_sample_lifinity_pool();

    // Set one reserve to zero
    pool.token_a.reserve = 0;

    let oracle_price = client.calculate_oracle_price(&pool);
    assert!(oracle_price.is_none());

    Ok(())
}

/// Test Lifinity quote calculation with oracle
#[tokio::test]
async fn test_lifinity_quote_calculation() -> Result<()> {
    let client = LifinityClient::new();
    let pool = create_sample_lifinity_pool();

    let input_amount = 1_000_000; // 1 USDC
    let quote = client.calculate_onchain_quote(&pool, input_amount)?;

    assert_eq!(quote.input_amount, input_amount);
    assert!(quote.output_amount > 0);
    assert_eq!(quote.dex, "Lifinity");
    assert!(quote.slippage_estimate.is_some());
    assert!(quote.slippage_estimate.unwrap() > 0.0);

    println!(
        "Lifinity Quote: {} {} -> {} {}",
        quote.input_amount, quote.input_token, quote.output_amount, quote.output_token
    );

    Ok(())
}

/// Test Lifinity swap instruction building
#[tokio::test]
async fn test_lifinity_swap_instruction() -> Result<()> {
    let client = LifinityClient::new();
    let pool = Arc::new(create_sample_lifinity_pool());

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

    assert_eq!(instruction.program_id, LIFINITY_PROGRAM_ID);
    assert!(!instruction.accounts.is_empty());
    assert!(!instruction.data.is_empty());

    // Should have 12 accounts (program, signer, pool, tokens, vaults, mints, programs, clock)
    assert_eq!(instruction.accounts.len(), 12);

    println!(
        "Lifinity instruction created with {} accounts",
        instruction.accounts.len()
    );

    Ok(())
}

/// Test pool discovery functionality
#[tokio::test]
async fn test_pool_discovery() -> Result<()> {
    let client = LifinityClient::new();
    let pools = DexClient::discover_pools(&client).await?;

    assert!(pools.len() >= 1); // Should have at least one sample pool

    // Verify pool properties
    for pool in &pools {
        assert_eq!(pool.dex_type, DexType::Lifinity);
        assert!(pool.oracle.is_some()); // Lifinity pools should have oracle
        assert!(pool.fee_rate_bips.is_some());

        // Test oracle price calculation for discovered pools
        let oracle_price = client.calculate_oracle_price(pool);
        assert!(oracle_price.is_some());
    }

    println!("Discovered {} Lifinity pools", pools.len());

    Ok(())
}

/// Test health check functionality
#[tokio::test]
async fn test_health_check() -> Result<()> {
    let client = LifinityClient::new();
    let health = client.health_check().await?;

    assert!(health.is_healthy);
    assert!(health.response_time_ms.is_some());
    assert_eq!(health.error_count, 0);
    assert!(!health.status_message.is_empty());

    println!("Health check: {}", health.status_message);

    Ok(())
}

/// Test math functions for Lifinity with oracle
#[tokio::test]
async fn test_lifinity_math_with_oracle() -> Result<()> {
    let input_amount = 1_000_000;
    let input_reserve = 100_000_000;
    let output_reserve = 200_000_000;
    let fee_bps = 20;
    let oracle_price = Some(1_900_000); // Oracle suggests slightly different price

    let output = calculate_lifinity_output(
        input_amount,
        input_reserve,
        output_reserve,
        fee_bps,
        oracle_price,
    )?;

    assert!(output > 0);

    println!(
        "Lifinity with oracle: {} input -> {} output",
        input_amount, output
    );

    Ok(())
}

/// Test math functions for Lifinity without oracle
#[tokio::test]
async fn test_lifinity_math_without_oracle() -> Result<()> {
    let input_amount = 1_000_000;
    let input_reserve = 100_000_000;
    let output_reserve = 200_000_000;
    let fee_bps = 20;
    let oracle_price = None; // No oracle price

    let output = calculate_lifinity_output(
        input_amount,
        input_reserve,
        output_reserve,
        fee_bps,
        oracle_price,
    )?;

    assert!(output > 0);

    println!(
        "Lifinity without oracle: {} input -> {} output",
        input_amount, output
    );

    Ok(())
}

/// Test edge cases for quote calculations
#[tokio::test]
async fn test_quote_edge_cases() -> Result<()> {
    let client = LifinityClient::new();
    let pool = create_sample_lifinity_pool();

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
    let client = LifinityClient::new();
    let pool_address = Pubkey::new_unique();

    let pool = client.fetch_pool_data(pool_address).await?;

    assert_eq!(pool.address, pool_address);
    assert_eq!(pool.dex_type, DexType::Lifinity);
    assert!(pool.fee_numerator.is_some());
    assert!(pool.fee_denominator.is_some());
    assert!(pool.oracle.is_some()); // Lifinity should have oracle

    // Test oracle price calculation
    let oracle_price = client.calculate_oracle_price(&pool);
    assert!(oracle_price.is_some());

    println!("Fetched pool: {} ({})", pool.name, pool.address);

    Ok(())
}

/// Test proactive market making behavior comparison
#[tokio::test]
async fn test_proactive_market_making() -> Result<()> {
    let input_amount = 10_000_000; // 10 USDC (larger amount to see MM effect)
    let input_reserve = 100_000_000;
    let output_reserve = 200_000_000;
    let fee_bps = 20;

    // Test without oracle (regular AMM behavior)
    let output_no_oracle =
        calculate_lifinity_output(input_amount, input_reserve, output_reserve, fee_bps, None)?;

    // Test with oracle suggesting higher price (>5% difference to trigger adjustment)
    let oracle_price_high = Some(2_200_000); // ~10% higher than pool price (2M)
    let output_oracle_high = calculate_lifinity_output(
        input_amount,
        input_reserve,
        output_reserve,
        fee_bps,
        oracle_price_high,
    )?;

    // Test with oracle suggesting lower price (>5% difference to trigger adjustment)
    let oracle_price_low = Some(1_800_000); // ~10% lower than pool price (2M)
    let output_oracle_low = calculate_lifinity_output(
        input_amount,
        input_reserve,
        output_reserve,
        fee_bps,
        oracle_price_low,
    )?;

    println!("Proactive MM comparison:");
    println!("  No oracle: {} -> {}", input_amount, output_no_oracle);
    println!("  Oracle high: {} -> {}", input_amount, output_oracle_high);
    println!("  Oracle low: {} -> {}", input_amount, output_oracle_low);

    // Oracle adjustments should create different outputs
    assert_ne!(output_no_oracle, output_oracle_high);
    assert_ne!(output_no_oracle, output_oracle_low);
    assert_ne!(output_oracle_high, output_oracle_low);

    Ok(())
}

/// Performance benchmark for quote calculations
#[tokio::test]
async fn test_quote_performance() -> Result<()> {
    let client = LifinityClient::new();
    let pool = create_sample_lifinity_pool();
    let input_amount = 1_000_000;

    let start_time = std::time::Instant::now();
    let iterations = 1000;

    for _ in 0..iterations {
        let _ = client.calculate_onchain_quote(&pool, input_amount)?;
    }

    let elapsed = start_time.elapsed();
    let avg_time = elapsed.as_micros() / iterations;

    // Should be fast enough for production use
    assert!(avg_time < 150); // Less than 150 microseconds average (allowing for oracle calculations)

    println!(
        "Performance: {} quotes in {:?} (avg: {}µs per quote)",
        iterations, elapsed, avg_time
    );

    Ok(())
}

/// Test concentration parameter behavior (math function test)
#[tokio::test]
async fn test_concentration_effects() -> Result<()> {
    use solana_arb_bot::dex::math::lifinity::calculate_concentration_adjustment;

    let base_liquidity = 1_000_000_000u128;

    // Test different concentration levels
    let low_concentration = calculate_concentration_adjustment(1000, base_liquidity)?; // 10%
    let medium_concentration = calculate_concentration_adjustment(2500, base_liquidity)?; // 25%
    let high_concentration = calculate_concentration_adjustment(5000, base_liquidity)?; // 50%

    // Higher concentration should result in higher effective liquidity
    assert!(low_concentration < medium_concentration);
    assert!(medium_concentration < high_concentration);

    println!("Concentration effects:");
    println!("  Base: {}", base_liquidity);
    println!("  Low (10%): {}", low_concentration);
    println!("  Medium (25%): {}", medium_concentration);
    println!("  High (50%): {}", high_concentration);

    Ok(())
}

/// Test inventory adjustment behavior (math function test)
#[tokio::test]
async fn test_inventory_adjustments() -> Result<()> {
    use solana_arb_bot::dex::math::lifinity::calculate_inventory_adjustment;

    let target_ratio = 1.0; // Target 1:1 ratio

    // Test balanced inventory
    let balanced = calculate_inventory_adjustment(1000, 1000, target_ratio)?;

    // Test excess token A
    let excess_a = calculate_inventory_adjustment(1200, 1000, target_ratio)?;

    // Test excess token B
    let excess_b = calculate_inventory_adjustment(1000, 1200, target_ratio)?;

    println!("Inventory adjustments:");
    println!("  Balanced: {}", balanced);
    println!("  Excess A: {}", excess_a);
    println!("  Excess B: {}", excess_b);

    // Excess token A should reduce price (< 1.0)
    // Excess token B should increase price (> 1.0)
    assert!(excess_a < balanced);
    assert!(excess_b > balanced);

    Ok(())
}

// Helper functions

fn create_sample_lifinity_pool() -> PoolInfo {
    PoolInfo {
        address: Pubkey::new_unique(),
        name: "Test Lifinity USDC-SOL Oracle Pool".to_string(),
        token_a: PoolToken {
            mint: solana_sdk::pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"), // USDC
            symbol: "USDC".to_string(),
            decimals: 6,
            reserve: 10_000_000_000, // 10M USDC
        },
        token_b: PoolToken {
            mint: solana_sdk::pubkey!("So11111111111111111111111111111111111111112"), // SOL
            symbol: "SOL".to_string(),
            decimals: 9,
            reserve: 50_000_000_000, // 50K SOL
        },
        token_a_vault: Pubkey::new_unique(),
        token_b_vault: Pubkey::new_unique(),
        fee_numerator: Some(20),
        fee_denominator: Some(10000),
        fee_rate_bips: Some(20), // 0.2%
        last_update_timestamp: chrono::Utc::now().timestamp() as u64,
        dex_type: DexType::Lifinity,
        liquidity: Some(1_000_000_000_000),
        sqrt_price: Some(2_000_000_000_000), // Represents ~200 USDC per SOL
        tick_current_index: Some(0),
        tick_spacing: Some(64),
        tick_array_0: None,
        tick_array_1: None,
        tick_array_2: None,
        oracle: Some(Pubkey::new_unique()), // Oracle for price feeds
    }
}
