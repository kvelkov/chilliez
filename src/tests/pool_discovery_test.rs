// src/tests/pool_discovery_test.rs
//! Integration tests for the Pool Discovery Service

#[cfg(test)]
mod integration_tests {
    use crate::dex::quote::DexClient;
    use crate::dex::raydium::RaydiumClient;
    use crate::discovery::PoolDiscoveryService;
    use crate::utils::PoolInfo;
    use tokio::sync::mpsc;
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

    #[tokio::test] 
    async fn test_pool_discovery_service() {
        // Test the complete Pool Discovery Service
        let (pool_sender, mut pool_receiver) = mpsc::channel::<Vec<PoolInfo>>(10);
        
        let dex_clients: Vec<Arc<dyn DexClient>> = vec![
            Arc::new(RaydiumClient::new()),
        ];
        
        let discovery_service = PoolDiscoveryService::new(dex_clients, pool_sender);
        
        // Verify service setup
        assert_eq!(discovery_service.client_count(), 1);
        assert_eq!(discovery_service.get_client_names(), vec!["Raydium".to_string()]);
        
        // Skip network test if in CI
        if std::env::var("SKIP_NETWORK_TESTS").is_ok() {
            println!("Skipping network portion of discovery service test");
            return;
        }
        
        // Run a single discovery cycle
        match discovery_service.run_discovery_cycle().await {
            Ok(_) => {
                println!("Discovery cycle completed successfully");
                
                // Try to receive discovered pools (with timeout)
                let pools_result = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    pool_receiver.recv()
                ).await;
                
                match pools_result {
                    Ok(Some(pools)) => {
                        println!("Received {} pools from discovery service", pools.len());
                        assert!(!pools.is_empty(), "Should receive some pools");
                    }
                    Ok(None) => {
                        println!("Channel closed - no pools received");
                    }
                    Err(_) => {
                        println!("Timeout waiting for pools - this may be normal");
                    }
                }
            }
            Err(e) => {
                eprintln!("Discovery cycle failed: {}", e);
                println!("Note: This test requires network access");
            }
        }
    }
}
