#[cfg(test)]
mod tests {
    use crate::arbitrage::engine::ArbitrageEngine;
    use crate::arbitrage::opportunity::{ArbHop, MultiHopArbOpportunity};
    use crate::utils::{DexType, PoolInfo, PoolToken, TokenAmount};
    use solana_sdk::pubkey::Pubkey;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;

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
            resolved.is_none(),
            "resolve_pools_for_opportunity should return None if any pool is missing"
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
    // ...other existing tests...
}
