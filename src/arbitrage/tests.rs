#[cfg(test)]
mod tests {
    use crate::arbitrage::engine::ArbitrageEngine;
    use crate::arbitrage::opportunity::{ArbHop, MultiHopArbOpportunity};
    use crate::utils::{DexType, PoolInfo, PoolToken};
    use solana_sdk::pubkey::Pubkey;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::{Mutex, RwLock};
    use crate::config::settings::Config; // For creating a dummy Config
    use crate::metrics::Metrics;
    use crate::error::ArbError;
    use crate::dex::DexClient; // For the dex_api_clients vector
    use crate::solana::rpc::SolanaRpcClient; // For Option<Arc<SolanaRpcClient>>
    use std::time::Duration; // For Duration

    // Helper to create a dummy Arc<Config>
    fn dummy_config() -> Arc<Config> {
        Arc::new(Config::from_env()) // Relies on default values or env vars if set
    }

    // Helper to create a dummy Arc<Mutex<Metrics>>
    fn dummy_metrics() -> Arc<Mutex<Metrics>> {
        Arc::new(Mutex::new(Metrics::new(100.0, None))) // sol_price_usd, log_path
    }

    // Helper to create dummy_pools
    fn create_dummy_pools_map() -> Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>> {
        let token_a_mint = Pubkey::new_unique();
        let token_b_mint = Pubkey::new_unique();
        let usdc_mint = Pubkey::new_unique();

        let pool1 = PoolInfo {
            address: Pubkey::new_unique(),
            name: "A/USDC-Orca".to_string(),
            token_a: PoolToken { mint: token_a_mint, symbol: "A".to_string(), decimals: 6, reserve: 1_000_000_000 },
            token_b: PoolToken { mint: usdc_mint, symbol: "USDC".to_string(), decimals: 6, reserve: 2_000_000_000 },
            fee_numerator: 30, fee_denominator: 10000, last_update_timestamp: 0, dex_type: DexType::Orca,
        };
        let pool2 = PoolInfo {
            address: Pubkey::new_unique(),
            name: "USDC/B-Raydium".to_string(),
            token_a: PoolToken { mint: usdc_mint, symbol: "USDC".to_string(), decimals: 6, reserve: 2_000_000_000 },
            token_b: PoolToken { mint: token_b_mint, symbol: "B".to_string(), decimals: 6, reserve: 1_000_000_000 },
            fee_numerator: 25, fee_denominator: 10000, last_update_timestamp: 0, dex_type: DexType::Raydium,
        };
        // Pool for cyclic A/USDC -> USDC/A
         let pool3 = PoolInfo {
            address: Pubkey::new_unique(),
            name: "USDC/A-Raydium".to_string(),
            token_a: PoolToken { mint: usdc_mint, symbol: "USDC".to_string(), decimals: 6, reserve: 2_100_000_000 }, // Slightly different price
            token_b: PoolToken { mint: token_a_mint, symbol: "A".to_string(), decimals: 6, reserve: 950_000_000 },
            fee_numerator: 25, fee_denominator: 10000, last_update_timestamp: 0, dex_type: DexType::Raydium,
        };

        let mut pools = HashMap::new();
        pools.insert(pool1.address, Arc::new(pool1));
        pools.insert(pool2.address, Arc::new(pool2));
        pools.insert(pool3.address, Arc::new(pool3));
        Arc::new(RwLock::new(pools))
    }
    
    // Mock DexClient for testing ArbitrageEngine initialization
    struct MockDexClient;
    #[async_trait::async_trait]
    impl DexClient for MockDexClient {
        async fn get_best_swap_quote(&self, _input_token: &str, _output_token: &str, _amount: u64) -> anyhow::Result<crate::dex::Quote> {
            unimplemented!()
        }
        fn get_supported_pairs(&self) -> Vec<(String, String)> { vec![] }
        fn get_name(&self) -> &str { "MockDex" }
    }


    #[tokio::test]
    async fn test_multihop_opportunity_detection_and_ban_logic() {
        let pools_map = create_dummy_pools_map();
        let config = dummy_config();
        let metrics_arc = dummy_metrics();
        let dummy_dex_clients: Vec<Arc<dyn DexClient>> = vec![Arc::new(MockDexClient)];

        // Initialize ArbitrageEngine with all required parameters
        let engine = ArbitrageEngine::new(
            pools_map.clone(),
            None, // Option<Arc<SolanaRpcClient>>
            config.clone(),
            metrics_arc.clone(),
            None, // Option<Arc<dyn CryptoDataProvider + Send + Sync>>
            None, // Option<Arc<Mutex<SolanaWebsocketManager>>>
            dummy_dex_clients, // Vec<Arc<dyn DexClient>>
        );
        
        // Modify min_profit_threshold on the detector through the engine's access if needed, or set on detector directly for test
        engine.detector.lock().await.set_min_profit_threshold(0.01 * 100.0); // 0.01% for test

        // Notification for each pool connection (conceptual, actual connection is elsewhere)
        for pool_arc in pools_map.read().await.values() {
            println!("Notification: Using pool {} ({})", pool_arc.name, pool_arc.address);
        }

        // Test multi-hop opportunity detection
        // Assuming discover_multihop_opportunities now finds 3-hop or more complex paths
        // The create_dummy_pools_map might not create a 3-hop by default.
        // For this test, let's focus on discover_direct_opportunities (2-hop cycles)
        let opps_result = engine.discover_direct_opportunities_refactored().await; // Use refactored version
        assert!(opps_result.is_ok(), "Opportunity detection failed: {:?}", opps_result.err());
        let opps = opps_result.unwrap();

        // With pool1 (A/USDC) and pool3 (USDC/A), a 2-hop cycle A -> USDC -> A should be found
        assert!(!opps.is_empty(), "No 2-hop cyclic opportunities detected when at least one should exist");
        println!("Detected {} direct (2-hop cyclic) opportunities.", opps.len());
        for opp in &opps {
            opp.log_summary();
        }


        // Test ban logic (directly on ArbitrageDetector for simplicity in test)
        let detector_arc = engine.detector.clone(); // Get Arc<Mutex<ArbitrageDetector>>
        
        let token_a_sym = "A";
        let token_b_sym = "USDC";

        assert!(!ArbitrageDetector::is_permanently_banned(token_a_sym, token_b_sym), "Pair A/USDC should not be banned by default");
        println!("Ban check before banning A/USDC: permanent: {}, temp: {}", 
            ArbitrageDetector::is_permanently_banned(token_a_sym, token_b_sym),
            ArbitrageDetector::is_temporarily_banned(token_a_sym, token_b_sym)
        );

        ArbitrageDetector::log_banned_pair(token_a_sym, token_b_sym, "permanent", "test permanent ban");
        assert!(ArbitrageDetector::is_permanently_banned(token_a_sym, token_b_sym), "Pair A/USDC should be permanently banned after logging");
        println!("Ban check after permanent banning A/USDC: {}", ArbitrageDetector::is_permanently_banned(token_a_sym, token_b_sym));
        
        // Clean up for other tests by removing the ban log or using a test-specific file
        // For simplicity, we're not cleaning up here, subsequent tests might see this ban.
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

        // Construct a MultiHopArbOpportunity with one existing and one missing pool in its path
        let opportunity_with_missing_pool = MultiHopArbOpportunity {
            id: "test_opp_missing".to_string(),
            hops: vec![
                ArbHop { dex: existing_pool_arc.dex_type.clone(), pool: existing_pool_arc.address, input_token: "X".into(), output_token: "Y".into(), input_amount: 1.0, expected_output: 1.0 },
                ArbHop { dex: DexType::Unknown("MissingDex".into()), pool: missing_pool_address, input_token: "Y".into(), output_token: "X".into(), input_amount: 1.0, expected_output: 1.0 },
            ],
            total_profit: 0.0, profit_pct: 0.0,
            input_token: "X".to_string(), output_token: "X".to_string(),
            input_amount: 1.0, expected_output: 1.0,
            dex_path: vec![existing_pool_arc.dex_type.clone(), DexType::Unknown("MissingDex".into())],
            pool_path: vec![existing_pool_arc.address, missing_pool_address], // Critical field for resolve_pools
            risk_score: None, notes: None,
            estimated_profit_usd: None, input_amount_usd: None, output_amount_usd: None,
            intermediate_tokens: vec!["Y".to_string()],
            source_pool: existing_pool_arc.clone(), // Placeholder
            target_pool: existing_pool_arc.clone(), // Placeholder
            input_token_mint: Pubkey::new_unique(), output_token_mint: Pubkey::new_unique(),
            intermediate_token_mint: Some(Pubkey::new_unique()),
        };

        let resolved_result = engine.resolve_pools_for_opportunity(&opportunity_with_missing_pool).await;
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
        let mut config_mut = Config::from_env();
        config_mut.min_profit_pct = 0.0025; // Set to 0.25% (fractional)
        let config = Arc::new(config_mut);

        let metrics_arc = dummy_metrics();
        let dummy_dex_clients: Vec<Arc<dyn DexClient>> = vec![Arc::new(MockDexClient)];

        let engine = ArbitrageEngine::new(
            pools_map, Some(Arc::new(SolanaRpcClient::new("http://dummy.rpc", vec![], 3, Duration::from_secs(1)))), 
            config.clone(), metrics_arc, None, None, dummy_dex_clients
        );

        // Engine stores threshold as percentage, config.min_profit_pct is fractional.
        assert_eq!(engine.get_min_profit_threshold().await, config.min_profit_pct * 100.0);
        
        let new_threshold_pct = 0.5; // 0.5%
        engine.set_min_profit_threshold(new_threshold_pct).await;
        assert_eq!(engine.get_min_profit_threshold().await, new_threshold_pct);
        // Check detector's threshold as well
        assert_eq!(engine.detector.lock().await.get_min_profit_threshold(), new_threshold_pct);
    }
}