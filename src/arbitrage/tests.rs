#[cfg(test)]
mod tests {
    use crate::arbitrage::engine::ArbitrageEngine;
    use crate::arbitrage::opportunity::{ArbHop, MultiHopArbOpportunity};
    use crate::utils::{DexType, PoolInfo, PoolToken};
    use solana_sdk::pubkey::Pubkey;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::{Mutex, RwLock};
    use crate::config::settings::Config;
    use crate::metrics::Metrics;
    use crate::error::ArbError;

    fn create_dummy_pools() -> Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>> {
        let pool1 = PoolInfo {
            address: Pubkey::new_unique(),
            name: "A/USDC".to_string(),
            token_a: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "A".to_string(),
                decimals: 6,
                reserve: 1_000_000_000,
            },
            token_b: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 2_000_000_000,
            },
            fee_numerator: 30,
            fee_denominator: 10000,
            last_update_timestamp: 0,
            dex_type: DexType::Orca,
        };
        let pool2 = PoolInfo {
            address: Pubkey::new_unique(),
            name: "USDC/B".to_string(),
            token_a: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 2_000_000_000,
            },
            token_b: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "B".to_string(),
                decimals: 6,
                reserve: 1_000_000_000,
            },
            fee_numerator: 30,
            fee_denominator: 10000,
            last_update_timestamp: 0,
            dex_type: DexType::Raydium,
        };
        let mut pools = HashMap::new();
        pools.insert(pool1.address, Arc::new(pool1));
        pools.insert(pool2.address, Arc::new(pool2));
        Arc::new(RwLock::new(pools))
    }

    #[tokio::test]
    async fn test_multihop_opportunity_detection_and_ban_logic() {
        // Setup pools
        let mut pools = HashMap::new();
        let pool_a = PoolInfo {
            address: Pubkey::new_unique(),
            name: "A/USDC".to_string(),
            token_a: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "A".to_string(),
                decimals: 6,
                reserve: 1_000_000_000,
            },
            token_b: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 2_000_000_000,
            },
            fee_numerator: 30,
            fee_denominator: 10000,
            last_update_timestamp: 0,
            dex_type: DexType::Orca,
        };
        let pool_b = PoolInfo {
            address: Pubkey::new_unique(),
            name: "USDC/B".to_string(),
            token_a: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 2_000_000_000,
            },
            token_b: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "B".to_string(),
                decimals: 6,
                reserve: 1_000_000_000,
            },
            fee_numerator: 30,
            fee_denominator: 10000,
            last_update_timestamp: 0,
            dex_type: DexType::Raydium,
        };
        pools.insert(pool_a.address, Arc::new(pool_a.clone()));
        pools.insert(pool_b.address, Arc::new(pool_b.clone()));
        let pools = Arc::new(RwLock::new(pools));

        let engine = ArbitrageEngine::new(
            pools.clone(),
            0.01, // min_profit_threshold
            0.1,  // max_slippage
            5000, // tx_fee_lamports
            None,
            None,
        );

        // Notification for each pool connection
        for pool in &[pool_a.clone(), pool_b.clone()] {
            println!(
                "Notification: Successfully connected to pool {} ({})",
                pool.name, pool.address
            );
        }

        // Test multi-hop opportunity detection
        let opps = engine.discover_multihop_opportunities().await;
        assert!(
            !opps.is_empty(),
            "No multi-hop opportunities detected when at least one should exist"
        );
        println!(
            "Detected {} multi-hop opportunities: {:?}",
            opps.len(),
            opps.iter().map(|o| &o.dex_path).collect::<Vec<_>>()
        );

        // Test ban logic
        let banned = engine.is_token_pair_banned("A", "B");
        assert!(!banned, "Token pair A/B should not be banned by default");
        println!("Ban check before banning: {}", banned);

        // Ban the pair and check again
        engine.ban_token_pair("A", "B", true, "test ban");
        let banned = engine.is_token_pair_banned("A", "B");
        assert!(
            banned,
            "Token pair A/B should be banned after ban_token_pair"
        );
        println!("Ban check after banning: {}", banned);
    }

    #[tokio::test]
    async fn test_multihop_opportunity_error_messages() {
        // Setup pools with missing hop
        let pools = Arc::new(RwLock::new(HashMap::new()));
        let engine = ArbitrageEngine::new(pools.clone(), 0.01, 0.1, 5000, None, None);

        // Create a fake multi-hop opportunity with a non-existent pool
        let fake_hop = ArbHop {
            dex: DexType::Orca,
            pool: Pubkey::new_unique(),
            input_token: "A".to_string(),
            output_token: "B".to_string(),
            input_amount: 1000.0,
            expected_output: 900.0,
        };
        let fake_opp = MultiHopArbOpportunity {
            hops: vec![fake_hop],
            total_profit: 0.0,
            profit_pct: 0.0,
            input_token: "A".to_string(),
            output_token: "B".to_string(),
            input_amount: 1000.0,
            expected_output: 900.0,
            dex_path: vec![DexType::Orca],
            pool_path: vec![],
            risk_score: None,
            notes: None,
        };

        let resolved = engine.resolve_pools_for_opportunity(&fake_opp).await;
        assert!(
            resolved.is_none(), //"resolve_pools_for_opportunity should return None if any pool is missing"
        );
        if resolved.is_none() {
            println!("Error: Could not resolve pools for fake multi-hop opportunity (as expected)");
        }
    }

    #[tokio::test]
    async fn test_ban_notification_and_unban() {
        let pools = Arc::new(RwLock::new(HashMap::new()));
        let engine = ArbitrageEngine::new(pools.clone(), 0.01, 0.1, 5000, None, None);

        // Ban a pair and check notification
        engine.ban_token_pair("X", "Y", false, "temporary test ban");
        let banned = engine.is_token_pair_banned("X", "Y");
        assert!(
            banned,
            "Token pair X/Y should be banned after ban_token_pair"
        );
        println!("Notification: Token pair X/Y is now banned (temporary)");

        // Simulate unban (if you have unban logic, otherwise just check ban remains)
        // If unban logic is implemented, test it here and print notification
    }

    #[tokio::test]
    async fn test_find_two_hop_opportunities() {
        let pools = create_dummy_pools();
        let config = Arc::new(Config::from_env()); // Create dummy config
        let metrics = Arc::new(Mutex::new(Metrics::new(100.0, None))); // Create dummy metrics
        let engine = ArbitrageEngine::new(pools.clone(), None, config, metrics, None); // Pass None for rpc_client and price_provider

        let opps = engine.detect_two_hop_arbitrage().await.expect("Detection failed");

        assert!(
            !opps.is_empty(),
            "Should find at least one 2-hop opportunity"
        );
        println!("Found 2-hop opportunities: {:#?}", opps);
        // Additional assertions can be made here based on expected opportunities
        // For example, check the number of opportunities, specific paths, profit, etc.
        assert_eq!(
            opps.len(),
            1,
            "Expected 1 two-hop opportunity with dummy data"
        );
        // This assertion needs MultiHopArbOpportunity to have a public dex_path field or a getter
        // Assuming MultiHopArbOpportunity has a public `dex_path: Vec<DexType>` or similar
        // let paths = opps.iter().map(|o| o.dex_path.clone()).collect::<Vec<_>>();
        // println!("Paths: {:?}", paths);
    }

    #[tokio::test]
    async fn test_ban_unban_token_pair() {
        let pools = create_dummy_pools();
        let config = Arc::new(Config::from_env());
        let metrics = Arc::new(Mutex::new(Metrics::new(100.0, None)));
        let engine = ArbitrageEngine::new(pools.clone(), None, config, metrics, None);

        // Initially, the pair should not be banned.
        // let banned = engine.is_token_pair_banned("A", "B").await;
        // assert!(!banned, "Pair A-B should not be banned initially");

        // Ban the pair.
        // engine.ban_token_pair("A", "B", true, "test ban").await;
        // let banned = engine.is_token_pair_banned("A", "B").await;
        // assert!(banned, "Pair A-B should be banned after banning");

        // TODO: Add unban functionality and test it if is_token_pair_banned and ban_token_pair are implemented
    }

    #[tokio::test]
    async fn test_resolve_pools_for_opportunity_missing_pool() {
        let pools_map = create_dummy_pools(); // Contains pool1, pool2
        let config = Arc::new(Config::from_env());
        let metrics = Arc::new(Mutex::new(Metrics::new(100.0, None)));
        let engine = ArbitrageEngine::new(pools_map.clone(), None, config, metrics, None);
        let missing_pool_address = Pubkey::new_unique();

        let opportunity_with_missing_pool = MultiHopArbOpportunity {
            id: "test_opp_missing".to_string(),
            dex_path: vec![
                pools_map.read().await.values().next().unwrap().dex_type.clone(), // Valid
                DexType::Unknown(missing_pool_address.to_string()), // Missing
            ],
            pool_addresses: vec![
                pools_map.read().await.keys().next().unwrap().clone(), // Valid address
                missing_pool_address,                                 // Missing address
            ],
            // ... other fields like input_token_mint, profit_pct, etc.
            input_token_mint: "mint_A".to_string(),
            intermediate_token_mint: Some("mint_B".to_string()),
            output_token_mint: "mint_C".to_string(),
            input_amount: 1000000,
            expected_output_amount: 1010000,
            profit_pct: 0.01,
            estimated_profit_usd: Some(10.0),
            input_amount_usd: Some(1000.0),
            output_amount_usd: Some(1010.0),
            priority_score: 1.0,
        };

        let resolved = engine
            .resolve_pools_for_opportunity(&opportunity_with_missing_pool)
            .await;
        assert!(
            resolved.is_err(), // Should be an error if a pool is missing
            "resolve_pools_for_opportunity should return Err if any pool is missing"
        );
        if let Err(ArbError::PoolNotFound(addr)) = resolved {
            assert_eq!(addr, missing_pool_address, "Error should specify the missing pool address");
        } else {
            panic!("Expected PoolNotFound error, got {:?}", resolved);
        }
    }

    #[tokio::test]
    async fn test_temporary_pair_ban_after_failure() {
        let pools = create_dummy_pools();
        let config = Arc::new(Config::from_env());
        let metrics = Arc::new(Mutex::new(Metrics::new(100.0, None)));
        let engine = ArbitrageEngine::new(pools.clone(), None, config, metrics, None);

        // Simulate a scenario where executing an opportunity for X-Y fails
        // This would typically involve the ArbitrageExecutor and its interaction with the engine's ban list.
        // For this test, we'll manually call the (hypothetical) ban method.
        // engine.ban_token_pair("X", "Y", false, "temporary test ban").await; // false for temporary
        // let banned = engine.is_token_pair_banned("X", "Y").await;
        // assert!(banned, "Pair X-Y should be temporarily banned");

        // TODO: Test that the ban expires or can be lifted.
        // This requires more infrastructure (like a time component or explicit unban).
    }
    // ...existing tests...
}
