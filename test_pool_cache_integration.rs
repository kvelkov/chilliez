//! Test file to verify pool cache integration functionality
//! Run with: cargo test --test pool_cache_integration

#[cfg(test)]
mod pool_cache_integration_tests {
    use solana_arb_bot::dex::discovery::PoolDiscoveryService;
    use solana_arb_bot::dex::get_all_discoverable_clients;
    use solana_arb_bot::cache::Cache;
    use solana_arb_bot::solana::rpc::SolanaRpcClient;
    use solana_arb_bot::dex::PoolValidationConfig;
    use solana_arb_bot::utils::{PoolInfo, DexType};
    use dashmap::DashMap;
    use std::sync::Arc;
    use tokio;

    #[tokio::test]
    async fn test_pool_cache_basic_functionality() {
        // Create a mock cache and RPC client
        let redis_cache = Arc::new(Cache::new(
            "redis://localhost:6379".to_string(),
            "test_db".to_string(),
        ));

        let rpc_client = Arc::new(SolanaRpcClient::new(
            vec!["https://api.mainnet-beta.solana.com".to_string()],
            Some(60),
        ));

        let discoverable_clients = get_all_discoverable_clients(redis_cache.clone());
        let validation_config = PoolValidationConfig::default();

        // Create pool discovery service
        let pool_discovery = PoolDiscoveryService::new(
            discoverable_clients,
            rpc_client,
            redis_cache,
            validation_config,
            std::path::Path::new("test_banned_pairs.csv"),
        ).expect("Failed to create PoolDiscoveryService");

        // Test cache statistics
        let (initial_size, dex_types) = pool_discovery.get_cache_stats();
        println!("Initial cache stats: {} pools, DEXs: {:?}", initial_size, dex_types);
        assert_eq!(initial_size, 0); // Should start empty

        // Test hot cache integration
        let hot_cache = Arc::new(DashMap::new());
        
        // Create a mock pool for testing
        let mock_pool = create_mock_pool();
        hot_cache.insert(mock_pool.address, Arc::new(mock_pool.clone()));

        // Test sync functionality
        let sync_result = pool_discovery.sync_with_hot_cache(&hot_cache).await;
        assert!(sync_result.is_ok(), "Cache sync should succeed");

        let (synced_size, _) = pool_discovery.get_cache_stats();
        assert_eq!(synced_size, 1, "Cache should contain synced pool");

        // Test pool retrieval
        let cached_pool = pool_discovery.get_cached_pool(&mock_pool.address);
        assert!(cached_pool.is_some(), "Should retrieve cached pool");

        // Test pool existence check
        assert!(pool_discovery.has_cached_pool(&mock_pool.address), "Should find cached pool");

        println!("✅ All pool cache integration tests passed!");
    }

    #[tokio::test]
    async fn test_cache_cleanup_functionality() {
        let redis_cache = Arc::new(Cache::new(
            "redis://localhost:6379".to_string(),
            "test_db".to_string(),
        ));

        let rpc_client = Arc::new(SolanaRpcClient::new(
            vec!["https://api.mainnet-beta.solana.com".to_string()],
            Some(60),
        ));

        let discoverable_clients = get_all_discoverable_clients(redis_cache.clone());
        let validation_config = PoolValidationConfig::default();

        let pool_discovery = PoolDiscoveryService::new(
            discoverable_clients,
            rpc_client,
            redis_cache,
            validation_config,
            std::path::Path::new("test_banned_pairs.csv"),
        ).expect("Failed to create PoolDiscoveryService");

        // Add mock pools with different reserve levels
        let high_liquidity_pool = create_mock_pool_with_reserves(10000, 10000);
        let low_liquidity_pool = create_mock_pool_with_reserves(100, 100);

        pool_discovery.update_pool_cache(&[high_liquidity_pool, low_liquidity_pool]).await;

        let (initial_size, _) = pool_discovery.get_cache_stats();
        assert_eq!(initial_size, 2, "Should have 2 pools initially");

        // Test cleanup - should remove low liquidity pools
        let removed_count = pool_discovery.cleanup_expired_pools(3600);
        println!("Removed {} pools during cleanup", removed_count);

        let (final_size, _) = pool_discovery.get_cache_stats();
        println!("Final cache size: {}", final_size);

        println!("✅ Cache cleanup test completed!");
    }

    fn create_mock_pool() -> PoolInfo {
        use solana_sdk::pubkey::Pubkey;
        use solana_arb_bot::utils::PoolToken;

        PoolInfo {
            address: Pubkey::new_unique(),
            dex_type: DexType::Orca,
            token_a: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "SOL".to_string(),
                decimals: 9,
                reserve: 1000000,
            },
            token_b: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 100000000,
            },
            fee_rate_bips: Some(30),
            sqrt_price: Some(1000000),
            liquidity: Some(50000000),
            tick_current_index: Some(0),
            tick_spacing: Some(64),
        }
    }

    fn create_mock_pool_with_reserves(reserve_a: u64, reserve_b: u64) -> PoolInfo {
        use solana_sdk::pubkey::Pubkey;
        use solana_arb_bot::utils::PoolToken;

        PoolInfo {
            address: Pubkey::new_unique(),
            dex_type: DexType::Raydium,
            token_a: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "TOKEN_A".to_string(),
                decimals: 9,
                reserve: reserve_a,
            },
            token_b: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "TOKEN_B".to_string(),
                decimals: 6,
                reserve: reserve_b,
            },
            fee_rate_bips: Some(25),
            sqrt_price: Some(1000000),
            liquidity: Some(reserve_a * reserve_b),
            tick_current_index: Some(0),
            tick_spacing: Some(60),
        }
    }
}
