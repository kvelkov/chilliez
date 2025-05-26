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
    use crate::dex::DexClient; 
    use crate::dex::quote::Quote; // Added import for Quote
    use crate::arbitrage::detector::ArbitrageDetector; // Added import for ArbitrageDetector
    use crate::solana::rpc::SolanaRpcClient; 
    use std::time::Duration; 
    use std::str::FromStr; // Added import for FromStr

    fn dummy_config() -> Arc<Config> {
        Arc::new(Config::from_env()) 
    }

    fn dummy_metrics() -> Arc<Mutex<Metrics>> {
        Arc::new(Mutex::new(Metrics::new(100.0, None))) 
    }

    fn create_dummy_pools_map() -> Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>> {
        // Use valid 32-char base58 mints for A and USDC so they match across pools
        let token_a_mint = Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap(); // SOL as dummy A
        let usdc_mint = Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap(); // USDC

        // Pool 1: A/USDC (Orca) -- A is CHEAP here (buy A with USDC)
        let pool1 = PoolInfo {
            address: Pubkey::new_unique(),
            name: "A/USDC-Orca (A cheap)".to_string(),
            token_a: PoolToken { mint: token_a_mint, symbol: "A".to_string(), decimals: 6, reserve: 2_000_000_000 }, // 2B A
            token_b: PoolToken { mint: usdc_mint, symbol: "USDC".to_string(), decimals: 6, reserve: 1_000_000_000 }, // 1B USDC
            fee_numerator: 30, fee_denominator: 10000, last_update_timestamp: 0, dex_type: DexType::Orca,
        };
        // Pool 2: USDC/A (Raydium) -- A is EXPENSIVE here (sell A for USDC)
        let pool2 = PoolInfo {
            address: Pubkey::new_unique(),
            name: "USDC/A-Raydium (A expensive)".to_string(),
            token_a: PoolToken { mint: usdc_mint, symbol: "USDC".to_string(), decimals: 6, reserve: 2_000_000_000 }, // 2B USDC
            token_b: PoolToken { mint: token_a_mint, symbol: "A".to_string(), decimals: 6, reserve: 500_000_000 }, // 500M A
            fee_numerator: 25, fee_denominator: 10000, last_update_timestamp: 0, dex_type: DexType::Raydium,
        };
        let mut pools = HashMap::new();
        pools.insert(pool1.address, Arc::new(pool1));
        pools.insert(pool2.address, Arc::new(pool2));
        // Debug print all pools and their tokens
        println!("\n=== Test Pool Setup ===");
        for (addr, pool) in &pools {
            println!("Pool {}: {} ({}:{})", addr, pool.name, pool.token_a.symbol, pool.token_b.symbol);
            println!("  token_a: {} {} reserve {}", pool.token_a.symbol, pool.token_a.mint, pool.token_a.reserve);
            println!("  token_b: {} {} reserve {}", pool.token_b.symbol, pool.token_b.mint, pool.token_b.reserve);
            println!("  dex_type: {:?}", pool.dex_type);
        }
        println!("======================\n");
        Arc::new(RwLock::new(pools))
    }
    
    struct MockDexClient;
    #[async_trait::async_trait]
    impl DexClient for MockDexClient {
        async fn get_best_swap_quote(&self, _input_token: &str, _output_token: &str, _amount: u64) -> anyhow::Result<Quote> { // Changed crate::dex::Quote to Quote
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

        let engine = ArbitrageEngine::new(
            pools_map.clone(),
            None, 
            config.clone(),
            metrics_arc.clone(),
            None, 
            None, 
            dummy_dex_clients, 
        );
        
        engine.detector.lock().await.set_min_profit_threshold(0.01 * 100.0); 

        for pool_arc in pools_map.read().await.values() {
            println!("Notification: Using pool {} ({})", pool_arc.name, pool_arc.address);
            println!("  token_a: {} {} reserve {}", pool_arc.token_a.symbol, pool_arc.token_a.mint, pool_arc.token_a.reserve);
            println!("  token_b: {} {} reserve {}", pool_arc.token_b.symbol, pool_arc.token_b.mint, pool_arc.token_b.reserve);
            println!("  dex_type: {:?}", pool_arc.dex_type);
        }

        // Changed discover_direct_opportunities_refactored to discover_direct_opportunities
        let opps_result: Result<Vec<MultiHopArbOpportunity>, ArbError> = engine._discover_direct_opportunities().await;
        println!("discover_direct_opportunities result: {:?}", opps_result);
        if let Ok(ref opps) = opps_result {
            println!("Number of opportunities found: {}", opps.len());
            for (i, opp) in opps.iter().enumerate() {
                println!("Opportunity {}: id={} total_profit={} profit_pct={}", i, opp.id, opp.total_profit, opp.profit_pct);
                println!("  hops:");
                for (j, hop) in opp.hops.iter().enumerate() {
                    println!("    Hop {}: dex={:?} pool={} {}->{} input_amount={} expected_output={}", j, hop.dex, hop.pool, hop.input_token, hop.output_token, hop.input_amount, hop.expected_output);
                }
            }
        }
        assert!(opps_result.is_ok(), "Opportunity detection failed: {:?}", opps_result.err());
        let opps = opps_result.unwrap();

        assert!(!opps.is_empty(), "No 2-hop cyclic opportunities detected when at least one should exist");
        println!("Detected {} direct (2-hop cyclic) opportunities.", opps.len());
        for opp in &opps {
            opp.log_summary();
        }

        let _detector_arc = engine.detector.clone(); 
        
        let token_a_sym = "A";
        let token_b_sym = "USDC";

        // Using ArbitrageDetector directly for ban logic tests
        assert!(!ArbitrageDetector::is_permanently_banned(token_a_sym, token_b_sym), "Pair A/USDC should not be banned by default");
        println!("Ban check before banning A/USDC: permanent: {}, temp: {}", 
            ArbitrageDetector::is_permanently_banned(token_a_sym, token_b_sym),
            ArbitrageDetector::is_temporarily_banned(token_a_sym, token_b_sym)
        );

        ArbitrageDetector::log_banned_pair(token_a_sym, token_b_sym, "permanent", "test permanent ban");
        assert!(ArbitrageDetector::is_permanently_banned(token_a_sym, token_b_sym), "Pair A/USDC should be permanently banned after logging");
        println!("Ban check after permanent banning A/USDC: {}", ArbitrageDetector::is_permanently_banned(token_a_sym, token_b_sym));
        
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
            total_profit: 0.0, profit_pct: 0.0,
            input_token: "X".to_string(), output_token: "X".to_string(),
            input_amount: 1.0, expected_output: 1.0,
            dex_path: vec![existing_pool_arc.dex_type.clone(), DexType::Unknown("MissingDex".into())],
            pool_path: vec![existing_pool_arc.address, missing_pool_address], 
            risk_score: None, notes: None,
            estimated_profit_usd: None, input_amount_usd: None, output_amount_usd: None,
            intermediate_tokens: vec!["Y".to_string()],
            source_pool: existing_pool_arc.clone(), 
            target_pool: existing_pool_arc.clone(), 
            input_token_mint: Pubkey::new_unique(), output_token_mint: Pubkey::new_unique(),
            intermediate_token_mint: Some(Pubkey::new_unique()),
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
        let mut config_mut = Config::from_env();
        config_mut.min_profit_pct = 0.005; // Set to 0.5% so threshold is 0.5
        let config = Arc::new(config_mut);

        let metrics_arc = dummy_metrics();
        let dummy_dex_clients: Vec<Arc<dyn DexClient>> = vec![Arc::new(MockDexClient)];

        let engine = ArbitrageEngine::new(
            pools_map, Some(Arc::new(SolanaRpcClient::new("http://dummy.rpc", vec![], 3, Duration::from_secs(1)))), 
            config.clone(), metrics_arc, None, None, dummy_dex_clients
        );

        // The engine stores min_profit_threshold as percent (i.e., config.min_profit_pct * 100.0)
        assert_eq!(engine.get_min_profit_threshold().await, config.min_profit_pct * 100.0);
        
        let new_threshold_pct = 0.5; 
        engine.set_min_profit_threshold(new_threshold_pct).await;
        assert_eq!(engine.get_min_profit_threshold().await, new_threshold_pct);
        assert_eq!(engine.detector.lock().await.get_min_profit_threshold(), new_threshold_pct);
    }
}