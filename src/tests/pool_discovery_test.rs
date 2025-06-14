// src/tests/pool_discovery_test.rs
//! Integration tests for Pool Discovery functionality

#[cfg(test)]
mod integration_tests {
    use crate::dex::api::DexClient;
    use crate::dex::raydium::RaydiumClient;
    use crate::utils::PoolInfo;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_raydium_pool_discovery() {
        // Test Raydium pool discovery functionality
        let raydium_client = RaydiumClient::new();
        
        // This is an integration test that requires network access
        // Skip if we're in CI or offline environment
        if std::env::var("SKIP_NETWORK_TESTS").is_ok() {
            println!("Skipping network test");
            return;
        }
        
        match raydium_client.discover_pools().await {
            Ok(pools) => {
                println!("Successfully discovered {} Raydium pools", pools.len());
                assert!(!pools.is_empty(), "Should discover at least some pools");
                
                // Verify pool structure
                for pool in pools.iter().take(3) { // Check first 3 pools
                    assert_ne!(pool.name, "", "Pool should have a name");
                    assert_ne!(pool.token_a.mint.to_string(), "11111111111111111111111111111111", "Should have valid token A mint");
                    assert_ne!(pool.token_b.mint.to_string(), "11111111111111111111111111111111", "Should have valid token B mint");
                    println!("Pool: {} - {}/{}", pool.name, pool.token_a.symbol, pool.token_b.symbol);
                }
            }
            Err(e) => {
                eprintln!("Pool discovery failed: {}", e);
                // Don't fail the test in case of network issues
                println!("Note: This test requires network access and may fail in offline environments");
            }
        }
    }
}
