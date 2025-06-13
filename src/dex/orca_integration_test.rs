// src/dex/orca_integration_test.rs
//! Integration test for Orca API integration

use crate::dex::orca::OrcaClient;
use crate::dex::quote::DexClient;
use anyhow::Result;
use log::info;

#[cfg(test)]
mod tests {
    use super::*;
    use tokio;

    #[tokio::test]
    async fn test_orca_api_integration() -> Result<()> {
        // Initialize logger for test output
        let _ = env_logger::try_init();

        info!("Testing Orca API integration...");

        let orca_client = OrcaClient::new();
        
        // Test pool discovery using the real API
        let pools = orca_client.discover_pools().await?;
        
        // Verify we got some pools
        assert!(!pools.is_empty(), "Should discover at least some pools from Orca API");
        
        info!("Successfully discovered {} pools", pools.len());
        
        // Verify pool data structure
        let first_pool = &pools[0];
        
        // Check that essential fields are populated
        assert!(!first_pool.name.is_empty(), "Pool name should not be empty");
        assert!(!first_pool.token_a.symbol.is_empty(), "Token A symbol should not be empty");
        assert!(!first_pool.token_b.symbol.is_empty(), "Token B symbol should not be empty");
        assert!(first_pool.token_a.decimals > 0, "Token A should have valid decimals");
        assert!(first_pool.token_b.decimals > 0, "Token B should have valid decimals");
        assert!(first_pool.fee_rate_bips.is_some(), "Fee rate should be set");
        assert!(first_pool.tick_spacing.is_some(), "Tick spacing should be set");
        
        info!("First pool: {}", first_pool.name);
        info!("  Token A: {} ({})", first_pool.token_a.symbol, first_pool.token_a.mint);
        info!("  Token B: {} ({})", first_pool.token_b.symbol, first_pool.token_b.mint);
        info!("  Fee rate: {} bips", first_pool.fee_rate_bips.unwrap_or(0));
        info!("  Tick spacing: {}", first_pool.tick_spacing.unwrap_or(0));
        
        // Test that we have a variety of pools (at least 10)
        assert!(pools.len() >= 10, "Should discover at least 10 pools, got {}", pools.len());
        
        // Test that pools have different token pairs
        let mut unique_pairs = std::collections::HashSet::new();
        for pool in &pools[..std::cmp::min(10, pools.len())] {
            let pair = format!("{}-{}", pool.token_a.symbol, pool.token_b.symbol);
            unique_pairs.insert(pair);
        }
        
        assert!(unique_pairs.len() >= 3, "Should have at least 3 different token pairs in first 10 pools");
        
        info!("API integration test passed successfully!");
        Ok(())
    }

    #[tokio::test]
    async fn test_orca_pool_conversion() -> Result<()> {
        let _ = env_logger::try_init();

        info!("Testing Orca pool data conversion...");

        let orca_client = OrcaClient::new();
        
        // Create a sample API pool data for testing
        let api_pool = crate::dex::orca::OrcaApiPool {
            address: "HJPjoWUrhoZzkNfRpHuieeFk9WcZWjwy6PBjZ81ngndJ".to_string(),
            token_a: crate::dex::orca::OrcaApiToken {
                mint: "So11111111111111111111111111111111111111112".to_string(),
                symbol: "SOL".to_string(),
                name: "Solana".to_string(),
                decimals: 9,
                logo_uri: None,
                coingecko_id: Some("solana".to_string()),
                whitelisted: true,
                pool_token: false,
            },
            token_b: crate::dex::orca::OrcaApiToken {
                mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(),
                symbol: "USDC".to_string(),
                name: "USD Coin".to_string(),
                decimals: 6,
                logo_uri: None,
                coingecko_id: Some("usd-coin".to_string()),
                whitelisted: true,
                pool_token: false,
            },
            tick_spacing: 64,
            price: 150.5,
            lp_fee_rate: 0.003,
            protocol_fee_rate: 0.0003,
            whitelisted: true,
            tvl: 1000000.0,
            volume: crate::dex::orca::OrcaApiVolume {
                day: 500000.0,
                week: 3000000.0,
                month: 12000000.0,
            },
            volume_denominated_in_token: Some("USDC".to_string()),
            price_range: Some(crate::dex::orca::OrcaApiPriceRange {
                min: 140.0,
                max: 160.0,
            }),
        };
        
        // Test conversion
        let pool_info = orca_client.convert_api_pool_to_pool_info(api_pool).await?;
        
        // Verify conversion results
        assert_eq!(pool_info.token_a.symbol, "SOL");
        assert_eq!(pool_info.token_b.symbol, "USDC");
        assert_eq!(pool_info.token_a.decimals, 9);
        assert_eq!(pool_info.token_b.decimals, 6);
        assert_eq!(pool_info.fee_rate_bips, Some(30)); // 0.003 * 10000 = 30 bips
        assert_eq!(pool_info.tick_spacing, Some(64));
        assert!(pool_info.sqrt_price.is_some());
        
        info!("Pool conversion test passed successfully!");
        Ok(())
    }
}

/// Manual test function that can be called from main for testing
#[allow(dead_code)]
pub async fn run_manual_test() -> Result<()> {
    env_logger::init();
    
    info!("Running manual Orca API integration test...");
    
    let orca_client = OrcaClient::new();
    let pools = orca_client.discover_pools().await?;
    
    info!("Discovered {} Orca pools", pools.len());
    
    // Print details of first few pools
    for (i, pool) in pools.iter().take(5).enumerate() {
        info!("Pool {}: {}", i + 1, pool.name);
        info!("  Address: {}", pool.address);
        info!("  {} ({}) / {} ({})", 
              pool.token_a.symbol, pool.token_a.decimals,
              pool.token_b.symbol, pool.token_b.decimals);
        info!("  Fee: {} bips", pool.fee_rate_bips.unwrap_or(0));
        info!("  Tick spacing: {}", pool.tick_spacing.unwrap_or(0));
        info!("");
    }
    
    Ok(())
}
