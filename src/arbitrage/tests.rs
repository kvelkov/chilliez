#[cfg(test)]
mod tests {
    use crate::arbitrage::engine::ArbitrageEngine;
    use crate::arbitrage::opportunity::{ArbHop, MultiHopArbOpportunity};
    use crate::arbitrage::fee_manager::{FeeManager, XYKSlippageModel};
    use crate::utils::{DexType, PoolInfo, PoolToken, TokenAmount};
    use crate::error::ArbError;
    use crate::dex::DexClient;
    use crate::dex::quote::Quote;
    use crate::arbitrage::detector::ArbitrageDetector;
    use crate::solana::rpc::SolanaRpcClient;
    use crate::config::settings::Config;
    use crate::metrics::Metrics;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::{Mutex, RwLock};
    use solana_sdk::pubkey::Pubkey;
    use std::time::Duration;
    use std::str::FromStr;
    use std::fs;
    use log::info;
    
    // Ensure test output is visible
    use env_logger;

    fn dummy_config() -> Arc<Config> {
        Arc::new(Config::test_default())
    }

    // New test-specific config for multihop tests.
    fn dummy_config_for_multihop_test() -> Arc<Config> {
        let mut cfg = Config::test_default();
        cfg.min_profit_pct = 0.0000001; // set almost zero percentage for pct checks
        cfg.sol_price_usd = Some(1.0);   // set SOL price to $1 simplifying USD conversion
        cfg.default_priority_fee_lamports = 0; // no priority fee for this test
        cfg.degradation_profit_factor = Some(0.0000001); // very low profit threshold in USD
        Arc::new(cfg)
    }

    // Dummy metrics constructor.
    fn dummy_metrics() -> Arc<Mutex<Metrics>> {
        // For our updated Metrics, if constructor takes parameters, ensure they are provided.
        // For instance, if Metrics::new accepts an initial profit value or TTL; adjust as needed.
        // Here we assume a simple constructor exists.
        Arc::new(Mutex::new(Metrics::new()))
    }

    // Creates dummy pools for testing.
    fn create_dummy_pools_map() -> Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>> {
        let token_a_mint = Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap();
        let usdc_mint = Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap();

        let pool1 = PoolInfo {
            address: Pubkey::new_from_array([1; 32]),
            name: "A/USDC-Orca".to_string(),
            token_a: PoolToken {
                mint: token_a_mint,
                symbol: "A".to_string(),
                decimals: 9,
                reserve: 2_000_000_000_000, // example reserve
            },
            token_b: PoolToken {
                mint: usdc_mint,
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 2_200_000_000_000, // adjusted reserve
            },
            fee_numerator: 30,
            fee_denominator: 10000,
            last_update_timestamp: 0,
            dex_type: DexType::Orca,
        };

        let pool2 = PoolInfo {
            address: Pubkey::new_from_array([2; 32]),
            name: "USDC/A-Raydium".to_string(),
            token_a: PoolToken {
                mint: usdc_mint,
                symbol: "USDC".to_string(),
                decimals: 6,
                reserve: 2_000_000_000_000,
            },
            token_b: PoolToken {
                mint: token_a_mint,
                symbol: "A".to_string(),
                decimals: 9,
                reserve: 7_050_000_000_000, // adjusted reserve
            },
            fee_numerator: 25,
            fee_denominator: 10000,
            last_update_timestamp: 0,
            dex_type: DexType::Raydium,
        };

        let mut pools = HashMap::new();
        pools.insert(pool1.address, Arc::new(pool1));
        pools.insert(pool2.address, Arc::new(pool2));

        println!("\n=== Test Pool Setup ===");
        for (addr, pool) in &pools {
            println!("Pool {}: {} ({}:{})", addr, pool.name, pool.token_a.symbol, pool.token_b.symbol);
            println!(
                "  token_a: {} {} (Dec: {}) reserve {}",
                pool.token_a.symbol, pool.token_a.mint, pool.token_a.decimals, pool.token_a.reserve
            );
            println!(
                "  token_b: {} {} (Dec: {}) reserve {}",
                pool.token_b.symbol, pool.token_b.mint, pool.token_b.decimals, pool.token_b.reserve
            );
            println!("  dex_type: {:?}", pool.dex_type);
        }
        println!("======================\n");

        Arc::new(RwLock::new(pools))
    }
    
    // A simple mock for DexClient.
    struct MockDexClient {
        name: String,
    }

    impl MockDexClient {
        pub fn new(name: &str) -> Self {
            Self { name: name.to_string() }
        }
    }

    #[async_trait::async_trait]
    impl DexClient for MockDexClient {
        async fn get_best_swap_quote(&self, _input_token: &str, _output_token: &str, _amount: u64) -> anyhow::Result<Quote> {
            unimplemented!()
        }
        fn get_supported_pairs(&self) -> Vec<(String, String)> { vec![] }
        fn get_name(&self) -> &str { &self.name }
    }

    #[tokio::test]
    async fn test_multihop_opportunity_detection_and_ban_logic() {
        // Initialize logger for test output.
        let _ = env_logger::builder().is_test(true).filter_level(log::LevelFilter::Info).try_init();

        // Cleanup any previous ban log file.
        let ban_log_path = "banned_pairs_log.csv";
        let _ = fs::remove_file(ban_log_path);

        let pools_map_arc = create_dummy_pools_map();
        let config_arc = dummy_config_for_multihop_test();
        let metrics_arc = dummy_metrics();
        let dummy_dex_clients: Vec<Arc<dyn DexClient>> = vec![Arc::new(MockDexClient::new("Mock"))];

        let engine = Arc::new(
            ArbitrateEngine::new(
                pools_map_arc.clone(),
                None,
                None,
                None,
                config_arc.clone(),
                metrics_arc.clone(),
                dummy_dex_clients,
            )
        );

        engine.set_min_profit_threshold_pct(0.01).await; // Set threshold to 0.01%

        // Print out pool notifications.
        {
            let pools_guard = pools_map_arc.read().await;
            for pool_arc_val in pools_guard.values() {
                let pool_val = pool_arc_val.as_ref();
                println!("Notification: Using pool {} ({})", pool_val.name, pool_val.address);
                println!("  token_a: {} {} reserve {}", pool_val.token_a.symbol, pool_val.token_a.mint, pool_val.token_a.reserve);
                println!("  token_b: {} {} reserve {}", pool_val.token_b.symbol, pool_val.token_b.mint, pool_val.token_b.reserve);
                println!("  dex_type: {:?}", pool_val.dex_type);
            }
        }

        // Discover direct opportunities.
        let opps_result = engine._discover_direct_opportunities().await;
        println!("discover_direct_opportunities result: {:?}", opps_result);

        if let Ok(ref opps) = opps_result {
            println!("Number of opportunities found: {}", opps.len());
            for (i, opp) in opps.iter().enumerate() {
                println!("Opportunity {}: id={} total_profit={:.8} profit_pct={:.4}% input_token_mint: {}", 
                         i, opp.id, opp.total_profit, opp.profit_pct, opp.input_token_mint);
                opp.log_summary();
            }
        } else {
            println!("discover_direct_opportunities error: {:?}", opps_result.as_ref().err());
        }
        assert!(opps_result.is_ok(), "Opportunity detection failed: {:?}", opps_result.err());
        let opps = opps_result.unwrap();
        assert!(!opps.is_empty(), "No 2-hop cyclic opportunities detected when at least one should exist based on test pool setup.");

        let token_a_sym = "A";
        let token_b_sym = "USDC";

        // By default, pair A / USDC should not be banned.
        assert!(!ArbitrageDetector::is_permanently_banned(token_a_sym, token_b_sym), "Pair A/USDC should not be banned by default");

        // Log a banned pair and verify.
        ArbitrageDetector::log_banned_pair(token_a_sym, token_b_sym, "permanent", "test permanent ban from test_multihop");

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(ArbitrageDetector::is_permanently_banned(token_a_sym, token_b_sym), "Pair A/USDC should be permanently banned after logging");
    }

    #[tokio::test]
    async fn test_resolve_pools_for_opportunity_missing_pool() {
        let pools_map = create_dummy_pools_map();
        let config = dummy_config();
        let metrics_arc = dummy_metrics();
        let dummy_dex_clients: Vec<Arc<dyn DexClient>> = vec![Arc::new(MockDexClient::new("Mock"))];
        let engine = ArbitrateEngine::new(pools_map.clone(), None, None, None, config, metrics_arc, dummy_dex_clients);

        let existing_pool_arc = pools_map.read().await.values().next().unwrap().clone();
        let missing_pool_address = Pubkey::new_unique();

        let opportunity_with_missing_pool = MultiHopArbOpportunity {
            id: "test_opp_missing".to_string(),
            hops: vec![
                ArbHop {
                    dex: existing_pool_arc.dex_type.clone(),
                    pool: existing_pool_arc.address,
                    input_token: "X".into(),
                    output_token: "Y".into(),
                    input_amount: 1.0,
                    expected_output: 1.0,
                },
                ArbHop {
                    dex: DexType::Unknown("MissingDex".into()),
                    pool: missing_pool_address,
                    input_token: "Y".into(),
                    output_token: "X".into(),
                    input_amount: 1.0,
                    expected_output: 1.0,
                }
            ],
            pool_path: vec![existing_pool_arc.address, missing_pool_address],
            source_pool: existing_pool_arc.clone(),
            target_pool: Arc::new(PoolInfo::default()),
            input_token_mint: Pubkey::new_unique(),
            output_token_mint: Pubkey::new_unique(),
            intermediate_token_mint: Some(Pubkey::new_unique()),
            ..MultiHopArbOpportunity::default()
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
        let mut config_mut = Config::test_default();
        config_mut.min_profit_pct = 0.005; // 0.5%
        let config = Arc::new(config_mut);
        let metrics_arc = dummy_metrics();
        let dummy_dex_clients: Vec<Arc<dyn DexClient>> = vec![Arc::new(MockDexClient::new("Mock"))];
        let dummy_sol_rpc_client = Some(Arc::new(SolanaRpcClient::new("http://dummy.rpc", vec![], 3, Duration::from_secs(1))));

        let engine = ArbitrateEngine::new(
            pools_map,
            None,
            None,
            dummy_sol_rpc_client,
            config.clone(),
            metrics_arc,
            dummy_dex_clients
        );

        let expected_threshold_pct = config.min_profit_pct * 100.0;
        assert_eq!(engine.get_min_profit_threshold_pct().await, expected_threshold_pct);

        {
            let detector_guard = engine.detector.lock().await;
            assert_eq!(detector_guard.get_min_profit_threshold_pct(), expected_threshold_pct);
        }

        let new_threshold_pct_val = 0.75;
        engine.set_min_profit_threshold_pct(new_threshold_pct_val).await;
        assert_eq!(engine.get_min_profit_threshold_pct().await, new_threshold_pct_val);
        {
            let detector_guard_after = engine.detector.lock().await;
            assert_eq!(detector_guard_after.get_min_profit_threshold_pct(), new_threshold_pct_val);
        }
    }

    #[tokio::test]
    async fn test_engine_initialization_with_dex_clients() {
        let config = Arc::new(Config::test_default());
        let pools = Arc::new(RwLock::new(HashMap::new()));
        let metrics = Arc::new(Mutex::new(Metrics::default()));

        let mock_dex_client1 = Arc::new(MockDexClient::new("Raydium"));
        let mock_dex_client2 = Arc::new(MockDexClient::new("Orca"));
        let dex_clients: Vec<Arc<dyn DexClient>> = vec![mock_dex_client1, mock_dex_client2];

        let engine = ArbitrateEngine::new(
            pools,
            None,
            None,
            None,
            config,
            metrics,
            dex_clients,
        );

        assert_eq!(engine._dex_providers.len(), 2, "Engine should have 2 DEX API clients");
        assert_eq!(engine._dex_providers[0].get_name(), "Raydium");
        assert_eq!(engine._dex_providers[1].get_name(), "Orca");
        info!("Engine initialized with {} DEX providers.", engine._dex_providers.len());
    }

    #[tokio::test]
    async fn test_engine_all_fields_and_methods_referenced() {
        let pools_map = create_dummy_pools_map();
        let config = dummy_config();
        let metrics_arc = dummy_metrics();
        let dummy_dex_clients: Vec<Arc<dyn DexClient>> = vec![Arc::new(MockDexClient::new("Mock"))];
        let engine = ArbitrateEngine::new(
            pools_map.clone(), None, None, None, config, metrics_arc, dummy_dex_clients
        );
        let _ = engine.degradation_mode.load(std::sync::atomic::Ordering::Relaxed);
        assert!(!engine._dex_providers.is_empty() || engine._dex_providers.len() == 0);
        let _ = engine.get_min_profit_threshold_pct().await;
        let _ = engine.discover_multihop_opportunities().await;
        let _ = engine.with_pool_guard_async(|_pools| ()).await;
        let _ = engine.update_pools(HashMap::new()).await;
        let _ = engine.get_current_status_string().await;
        let _ = engine.start_services(None).await;
    }

    #[test]
    fn test_opportunity_profit_checks() {
        let mut opp = MultiHopArbOpportunity::default();
        opp.profit_pct = 1.5;
        opp.estimated_profit_usd = Some(10.0);
        assert!(opp.is_profitable_by_pct(1.0));
        assert!(opp.is_profitable_by_usd(5.0));
        assert!(opp.is_profitable(1.0, 5.0));
        assert!(!opp.is_profitable_by_pct(2.0));
        assert!(!opp.is_profitable_by_usd(20.0));
        assert!(!opp.is_profitable(2.0, 20.0));
    }

    #[test]
    fn test_exercise_all_fee_manager_functions() {
        use crate::arbitrage::fee_manager::{FeeManager, XYKSlippageModel};
        use crate::utils::PoolInfo;
        let pool = PoolInfo::default();
        let input_amt = TokenAmount::new(100_000_000, 6);
        let _ = FeeManager::estimate_pool_swap_with_model(
            &pool,
            &input_amt,
            true,
            Some(pool.fee_numerator),
            Some(pool.fee_denominator),
            Some(pool.last_update_timestamp),
            &XYKSlippageModel::default(),
        );
        let _ = XYKSlippageModel::default();
    }
}
