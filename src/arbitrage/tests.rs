#[cfg(test)]
mod tests {
    use crate::arbitrage::engine::ArbitrageEngine;
    use crate::arbitrage::opportunity::{ArbHop, MultiHopArbOpportunity};
    use crate::utils::{DexType, PoolInfo, PoolToken, TokenAmount}; // Added TokenAmount
    use solana_sdk::pubkey::Pubkey;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::{Mutex, RwLock};
    use crate::config::settings::Config; 
    use crate::metrics::Metrics;
    use crate::error::ArbError;
    use crate::dex::DexClient; 
    use crate::dex::quote::Quote;
    use crate::arbitrage::detector::ArbitrageDetector;
    use crate::solana::rpc::SolanaRpcClient; 
    use std::time::Duration; 
    use std::str::FromStr;

    fn dummy_config() -> Arc<Config> {
        Arc::new(Config::from_env()) 
    }

    fn dummy_metrics() -> Arc<Mutex<Metrics>> {
        Arc::new(Mutex::new(Metrics::new(100.0, None))) 
    }

    fn create_dummy_pools_map() -> Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>> {
        let token_a_mint = Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap(); 
        let usdc_mint = Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap(); 

        let pool1 = PoolInfo {
            address: Pubkey::new_from_array([1; 32]), // Fixed address for deterministic tests
            name: "A/USDC-Orca".to_string(),
            token_a: PoolToken { mint: token_a_mint, symbol: "A".to_string(), decimals: 9, reserve: 2_000_000_000_000 }, // 2k A (SOL decimals)
            token_b: PoolToken { mint: usdc_mint, symbol: "USDC".to_string(), decimals: 6, reserve: 1_000_000_000_000 }, // 1M USDC
            fee_numerator: 30, fee_denominator: 10000, last_update_timestamp: 0, dex_type: DexType::Orca,
        };
        
        let pool2 = PoolInfo {
            address: Pubkey::new_from_array([2; 32]), // Fixed address
            name: "USDC/A-Raydium".to_string(), 
            // For USDC/A, token_a is USDC, token_b is A
            token_a: PoolToken { mint: usdc_mint, symbol: "USDC".to_string(), decimals: 6, reserve: 2_000_000_000_000 }, // 2M USDC
            token_b: PoolToken { mint: token_a_mint, symbol: "A".to_string(), decimals: 9, reserve: 950_000_000_000 },   // 950 A (SOL decimals)
            fee_numerator: 25, fee_denominator: 10000, last_update_timestamp: 0, dex_type: DexType::Raydium,
        };
        let mut pools = HashMap::new();
        pools.insert(pool1.address, Arc::new(pool1));
        pools.insert(pool2.address, Arc::new(pool2));
        
        println!("\n=== Test Pool Setup ===");
        for (addr, pool) in &pools {
            println!("Pool {}: {} ({}:{})", addr, pool.name, pool.token_a.symbol, pool.token_b.symbol);
            println!("  token_a: {} {} (Dec: {}) reserve {}", pool.token_a.symbol, pool.token_a.mint, pool.token_a.decimals, pool.token_a.reserve);
            println!("  token_b: {} {} (Dec: {}) reserve {}", pool.token_b.symbol, pool.token_b.mint, pool.token_b.decimals, pool.token_b.reserve);
            println!("  dex_type: {:?}", pool.dex_type);
        }
        println!("======================\n");
        Arc::new(RwLock::new(pools))
    }
    
    struct MockDexClient;
    #[async_trait::async_trait]
    impl DexClient for MockDexClient {
        async fn get_best_swap_quote(&self, _input_token: &str, _output_token: &str, _amount: u64) -> anyhow::Result<Quote> {
            unimplemented!()
        }
        fn get_supported_pairs(&self) -> Vec<(String, String)> { vec![] }
        fn get_name(&self) -> &str { "MockDex" }
    }


    #[tokio::test]
    async fn test_multihop_opportunity_detection_and_ban_logic() {
        let pools_map_arc = create_dummy_pools_map(); // Renamed for clarity
        let config_arc = dummy_config(); // Renamed
        let metrics_arc = dummy_metrics();
        let dummy_dex_clients: Vec<Arc<dyn DexClient>> = vec![Arc::new(MockDexClient)];

        // Engine creates its own detector based on config
        let engine = ArbitrageEngine::new(
            pools_map_arc.clone(), // Use Arc
            None, 
            config_arc.clone(), // Use Arc
            metrics_arc.clone(), // Use Arc
            None, 
            None, 
            dummy_dex_clients,
        );
        
        // Set threshold directly on the engine, which propagates to its detector
        engine.set_min_profit_threshold_pct(0.01).await; // Set to 0.01%

        { // Scope for read lock
            let pools_guard = pools_map_arc.read().await;
            for pool_arc_val in pools_guard.values() { // Iterate over values
                let pool_val = pool_arc_val.as_ref(); // Dereference Arc
                println!("Notification: Using pool {} ({})", pool_val.name, pool_val.address);
                println!("  token_a: {} {} reserve {}", pool_val.token_a.symbol, pool_val.token_a.mint, pool_val.token_a.reserve);
                println!("  token_b: {} {} reserve {}", pool_val.token_b.symbol, pool_val.token_b.mint, pool_val.token_b.reserve);
                println!("  dex_type: {:?}", pool_val.dex_type);
            }
        }

        let opps_result = engine._discover_direct_opportunities().await; // Call engine's method
        println!("discover_direct_opportunities result: {:?}", opps_result);

        if let Ok(ref opps) = opps_result {
            println!("Number of opportunities found: {}", opps.len());
            for (i, opp) in opps.iter().enumerate() {
                println!("Opportunity {}: id={} total_profit={:.8} profit_pct={:.4}% input_token_mint: {}", i, opp.id, opp.total_profit, opp.profit_pct, opp.input_token_mint);
                 opp.log_summary(); // Use the opportunity's own logging method
            }
        } else {
            println!("discover_direct_opportunities error: {:?}", opps_result.as_ref().err());
        }
        assert!(opps_result.is_ok(), "Opportunity detection failed: {:?}", opps_result.err());
        let opps = opps_result.unwrap();
        
        // The test expects opportunities with the dummy pools. If this fails, review detector logic and pool setup.
        assert!(!opps.is_empty(), "No 2-hop cyclic opportunities detected when at least one should exist based on test pool setup.");
        
        let token_a_sym = "A"; // Symbol for Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap();
        let token_b_sym = "USDC"; // Symbol for Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap();

        assert!(!ArbitrageDetector::is_permanently_banned(token_a_sym, token_b_sym), "Pair A/USDC should not be banned by default");
        
        ArbitrageDetector::log_banned_pair(token_a_sym, token_b_sym, "permanent", "test permanent ban from test_multihop");
        assert!(ArbitrageDetector::is_permanently_banned(token_a_sym, token_b_sym), "Pair A/USDC should be permanently banned after logging");
    }


    #[tokio::test]
    async fn test_resolve_pools_for_opportunity_missing_pool() {
        let pools_map = create_dummy_pools_map();
        let config = dummy_config();
        let metrics_arc = dummy_metrics();
        let dummy_dex_clients: Vec<Arc<dyn DexClient>> = vec![Arc::new(MockDexClient)];

        let engine = ArbitrageEngine::new(
            pools_map.clone(), None, config, metrics_arc, None, None, dummy_dex_clients
        );

        let existing_pool_arc = pools_map.read().await.values().next().unwrap().clone();
        let missing_pool_address = Pubkey::new_unique();

        let opportunity_with_missing_pool = MultiHopArbOpportunity {
            id: "test_opp_missing".to_string(),
            hops: vec![
                ArbHop { dex: existing_pool_arc.dex_type.clone(), pool: existing_pool_arc.address, input_token: "X".into(), output_token: "Y".into(), input_amount: 1.0, expected_output: 1.0 },
                ArbHop { dex: DexType::Unknown("MissingDex".into()), pool: missing_pool_address, input_token: "Y".into(), output_token: "X".into(), input_amount: 1.0, expected_output: 1.0 },
            ],
            pool_path: vec![existing_pool_arc.address, missing_pool_address], 
            source_pool: existing_pool_arc.clone(), 
            // Target pool can be a clone of existing or a default if it's part of the missing path.
            // If the missing pool is the target, this needs careful thought.
            // For this test, assuming target_pool isn't strictly required for _resolve_pools_for_opportunity if path is used.
            target_pool: Arc::new(PoolInfo::default()), // Using a default for placeholder
            input_token_mint: Pubkey::new_unique(), // Placeholder
            output_token_mint: Pubkey::new_unique(), // Placeholder
            intermediate_token_mint: Some(Pubkey::new_unique()), // Placeholder
            ..MultiHopArbOpportunity::default() // Fill with other default values
        };

        let resolved_result = engine._resolve_pools_for_opportunity(&opportunity_with_missing_pool).await;
        assert!(resolved_result.is_err(), "resolve_pools_for_opportunity should return Err if any pool in pool_path is missing");
        
        if let Err(ArbError::PoolNotFound(addr_str)) = resolved_result {
            assert_eq!(addr_str, missing_pool_address.to_string(), "Error should specify the missing pool address");
            println!("Correctly identified missing pool: {}", addr_str);
        } else {
            panic!("Expected PoolNotFound error, got {:?}", resolved_result);
        }
    }

    #[tokio::test]
    async fn test_engine_initialization_and_threshold() {
        let pools_map = create_dummy_pools_map();
        let mut config_mut = Config::from_env(); // Assuming this loads defaults
        config_mut.min_profit_pct = 0.005; // Set to 0.5% (fractional)
        let config = Arc::new(config_mut);

        let metrics_arc = dummy_metrics();
        let dummy_dex_clients: Vec<Arc<dyn DexClient>> = vec![Arc::new(MockDexClient)];
        let dummy_sol_rpc_client = Some(Arc::new(SolanaRpcClient::new("http://dummy.rpc", vec![], 3, Duration::from_secs(1))));


        let engine = ArbitrageEngine::new(
            pools_map, 
            dummy_sol_rpc_client, 
            config.clone(), // Pass the Arc<Config>
            metrics_arc, 
            None, 
            None, 
            dummy_dex_clients
        );

        // Engine's detector stores threshold as percentage (e.g., 0.5 for 0.5%)
        let expected_threshold_pct = config.min_profit_pct * 100.0;
        assert_eq!(engine.get_min_profit_threshold_pct().await, expected_threshold_pct);
        
        // Test detector directly from engine
        let detector_guard = engine.detector.lock().await;
        assert_eq!(detector_guard.get_min_profit_threshold_pct(), expected_threshold_pct);
        drop(detector_guard); // Release lock

        let new_threshold_pct_val = 0.75; // New threshold as percentage
        engine.set_min_profit_threshold_pct(new_threshold_pct_val).await;
        assert_eq!(engine.get_min_profit_threshold_pct().await, new_threshold_pct_val);
        
        let detector_guard_after_set = engine.detector.lock().await;
        assert_eq!(detector_guard_after_set.get_min_profit_threshold_pct(), new_threshold_pct_val);
    }
}