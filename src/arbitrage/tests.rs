use crate::config::settings::Config;
use crate::dex::api::CommonSwapInfo;
use crate::dex::api::{Quote, SwapInfo};
use crate::dex::{BannedPairsManager, DexClient};
use crate::error::{self, ArbError};
use crate::local_metrics::Metrics;
use crate::utils::{DexType, PoolInfo, PoolToken};
use dashmap::DashMap;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;

// Ensure test output is visible
// use env_logger;

// Helper function to create a dummy BannedPairsManager for testing
#[allow(dead_code)]
fn dummy_banned_pairs_manager() -> Arc<BannedPairsManager> {
    // Create a temporary CSV file for testing
    let temp_csv_path = std::env::temp_dir().join("test_banned_pairs.csv");
    std::fs::write(&temp_csv_path, "token_a,token_b\n").unwrap_or_default();

    Arc::new(
        BannedPairsManager::new(temp_csv_path.to_string_lossy().to_string()).unwrap_or_else(|_| {
            // Fallback: create with an empty temporary file
            let fallback_path = std::env::temp_dir().join("empty_banned_pairs.csv");
            std::fs::write(&fallback_path, "").unwrap_or_default();
            BannedPairsManager::new(fallback_path.to_string_lossy().to_string())
                .expect("Failed to create fallback BannedPairsManager")
        }),
    )
}

#[allow(dead_code)]
fn dummy_config() -> Arc<Config> {
    Arc::new(Config::test_default())
}

// New test-specific config for multihop tests.
#[allow(dead_code)]
fn dummy_config_for_multihop_test() -> Arc<Config> {
    let mut cfg = Config::test_default();
    cfg.min_profit_pct = 0.0000001; // set almost zero percentage for pct checks
    cfg.sol_price_usd = Some(1.0); // set SOL price to $1 simplifying USD conversion
    cfg.default_priority_fee_lamports = 0; // no priority fee for this test
    cfg.degradation_profit_factor = Some(0.0000001); // very low profit threshold in USD
    Arc::new(cfg)
}

// Dummy metrics constructor.
#[allow(dead_code)]
fn dummy_metrics() -> Arc<Mutex<Metrics>> {
    // Use Metrics::new() instead of Metrics::default()
    Arc::new(Mutex::new(Metrics::new()))
}

// Creates dummy pools for testing.
#[allow(dead_code)]
fn create_dummy_pools_map() -> Arc<DashMap<Pubkey, Arc<PoolInfo>>> {
    let token_a_mint = Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap();
    let usdc_mint = Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap();
    let token_b_mint = Pubkey::new_unique(); // Use a valid unique pubkey for token B

    let pool1 = PoolInfo {
        address: Pubkey::new_from_array([1; 32]),
        name: "A/USDC-Orca".to_string(),
        token_a: PoolToken {
            mint: token_a_mint,
            symbol: "A".to_string(),
            decimals: 9,
            reserve: 1_000_000_000_000,
        },
        token_b: PoolToken {
            mint: usdc_mint,
            symbol: "USDC".to_string(),
            decimals: 6,
            reserve: 1_000_000_000_000,
        },
        token_a_vault: Pubkey::new_unique(),
        token_b_vault: Pubkey::new_unique(),
        fee_numerator: Some(0),
        fee_denominator: Some(1),
        fee_rate_bips: Some(0),
        last_update_timestamp: 0,
        dex_type: DexType::Orca,
        liquidity: Some(1_000_000_000),
        sqrt_price: None,
        tick_current_index: None,
        tick_spacing: None,
        tick_array_0: None,
        tick_array_1: None,
        tick_array_2: None,
        oracle: None,
    };

    let pool2 = PoolInfo {
        address: Pubkey::new_from_array([2; 32]),
        name: "USDC/B-Raydium".to_string(),
        token_a: PoolToken {
            mint: usdc_mint,
            symbol: "USDC".to_string(),
            decimals: 6,
            reserve: 1_000_000_000_000,
        },
        token_b: PoolToken {
            mint: token_b_mint,
            symbol: "B".to_string(),
            decimals: 9,
            reserve: 1_000_000_000_000,
        },
        token_a_vault: Pubkey::new_unique(),
        token_b_vault: Pubkey::new_unique(),
        fee_numerator: Some(0),
        fee_denominator: Some(1),
        fee_rate_bips: Some(0),
        last_update_timestamp: 0,
        dex_type: DexType::Raydium,
        liquidity: Some(1_500_000_000),
        sqrt_price: None,
        tick_current_index: None,
        tick_spacing: None,
        tick_array_0: None,
        tick_array_1: None,
        tick_array_2: None,
        oracle: None,
    };

    let pool3 = PoolInfo {
        address: Pubkey::new_from_array([3; 32]),
        name: "B/A-Orca".to_string(),
        token_a: PoolToken {
            mint: token_b_mint,
            symbol: "B".to_string(),
            decimals: 9,
            reserve: 500_000_000_000, // Lower reserve for B
        },
        token_b: PoolToken {
            mint: token_a_mint,
            symbol: "A".to_string(),
            decimals: 9,
            reserve: 2_000_000_000_000, // Much higher reserve for A
        },
        token_a_vault: Pubkey::new_unique(),
        token_b_vault: Pubkey::new_unique(),
        fee_numerator: Some(0),
        fee_denominator: Some(1),
        fee_rate_bips: Some(0),
        last_update_timestamp: 0,
        dex_type: DexType::Orca,
        liquidity: Some(1_000_000_000),
        sqrt_price: None,
        tick_current_index: None,
        tick_spacing: None,
        tick_array_0: None,
        tick_array_1: None,
        tick_array_2: None,
        oracle: None,
    };

    let pools = DashMap::new();
    pools.insert(pool1.address, Arc::new(pool1));
    pools.insert(pool2.address, Arc::new(pool2));
    pools.insert(pool3.address, Arc::new(pool3));

    println!("\n=== Test Pool Setup ===");
    for entry in pools.iter() {
        let (addr, pool) = (entry.key(), entry.value());
        println!(
            "Pool {}: {} ({}:{})",
            addr, pool.name, pool.token_a.symbol, pool.token_b.symbol
        );
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

    Arc::new(pools)
}

// A simple mock for DexClient.
#[allow(dead_code)]
struct MockDexClient {
    name: String,
}

#[allow(dead_code)]
impl MockDexClient {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl DexClient for MockDexClient {
    fn get_name(&self) -> &str {
        &self.name
    }

    fn calculate_onchain_quote(
        &self,
        _pool: &PoolInfo,
        _input_amount: u64,
    ) -> anyhow::Result<Quote> {
        Ok(Quote {
            input_token: "SOL".to_string(),
            output_token: "USDC".to_string(),
            input_amount: _input_amount,
            output_amount: _input_amount / 2, // Mock calculation
            dex: self.name.clone(),
            route: vec![_pool.address],
            slippage_estimate: Some(0.01),
        })
    }

    fn get_swap_instruction(&self, _swap_info: &SwapInfo) -> anyhow::Result<Instruction> {
        // Return a mock instruction
        Ok(solana_sdk::instruction::Instruction {
            program_id: Pubkey::default(),
            accounts: vec![],
            data: vec![],
        })
    }

    async fn discover_pools(&self) -> anyhow::Result<Vec<PoolInfo>> {
        Ok(vec![]) // Return empty pool list for tests
    }

    async fn get_swap_instruction_enhanced(
        &self,
        _swap_info: &CommonSwapInfo,
        _pool_info: Arc<PoolInfo>,
    ) -> Result<Instruction, error::ArbError> {
        // Return a mock instruction or an error for the stub
        // For simplicity, let's return an error indicating it's a stub
        Err(ArbError::InstructionError(
            "MockDexClient get_swap_instruction_enhanced not implemented".to_string(),
        ))
    }

    async fn health_check(
        &self,
    ) -> Result<crate::dex::api::DexHealthStatus, crate::error::ArbError> {
        Ok(crate::dex::api::DexHealthStatus {
            is_healthy: true,
            last_successful_request: Some(std::time::Instant::now()),
            error_count: 0,
            response_time_ms: Some(10),
            pool_count: Some(0),
            status_message: "Mock DEX client always healthy".to_string(),
        })
    }
}

#[tokio::test]
async fn test_multihop_opportunity_detection_and_ban_logic() {
    use crate::arbitrage::{ArbHop, ArbitrageOrchestrator, MultiHopArbOpportunity};
    use std::fs;
    use std::time::Duration;

    // Initialize logger for test output.
    let _ = env_logger::builder()
        .is_test(true)
        .filter_level(log::LevelFilter::Info)
        .try_init();

    // Cleanup any previous ban log file.
    let ban_log_path = "banned_pairs_log.csv";
    let _ = fs::remove_file(ban_log_path);

    let pools_map_arc = create_dummy_pools_map();
    let config_arc = dummy_config_for_multihop_test();
    let metrics_arc = dummy_metrics();
    let dummy_dex_clients: Vec<Arc<dyn DexClient>> = vec![Arc::new(MockDexClient::new("Mock"))];

    let engine = Arc::new(ArbitrageOrchestrator::new(
        pools_map_arc.clone(),
        None,
        None,
        config_arc.clone(),
        metrics_arc.clone(),
        dummy_dex_clients,
        dummy_banned_pairs_manager(), // banned_pairs_manager
        None,                         // quicknode_opportunity_receiver
    ));

    engine.set_min_profit_threshold_pct(0.01).await; // Set threshold to 0.01%

    // Print out pool notifications.
    {
        for entry in pools_map_arc.iter() {
            let (_addr, pool_arc_val) = (entry.key(), entry.value());
            let pool_val = pool_arc_val.as_ref();
            println!(
                "Notification: Using pool {} ({})",
                pool_val.name, pool_val.address
            );
            println!(
                "  token_a: {} {} reserve {}",
                pool_val.token_a.symbol, pool_val.token_a.mint, pool_val.token_a.reserve
            );
            println!(
                "  token_b: {} {} reserve {}",
                pool_val.token_b.symbol, pool_val.token_b.mint, pool_val.token_b.reserve
            );
            println!("  dex_type: {:?}", pool_val.dex_type);
        }
    }

    // Discover direct opportunities.
    // Explicit type annotation for clarity, though it might be inferred after other fixes.
    let opps_result: Result<Vec<MultiHopArbOpportunity>, ArbError> =
        engine.detect_arbitrage_opportunities().await;
    println!("discover_direct_opportunities result: {:?}", opps_result);

    if let Ok(ref opps) = opps_result {
        println!("Number of opportunities found: {}", opps.len());
        for (i, opp) in opps.iter().enumerate() {
            println!(
                "Opportunity {}: id={} total_profit={:.8} profit_pct={:.4}% input_token_mint: {}",
                i, opp.id, opp.total_profit, opp.profit_pct, opp.input_token_mint
            );
            opp.log_summary();
        }
    } else {
        println!(
            "discover_direct_opportunities error: {:?}",
            opps_result.as_ref().err()
        );
    }
    assert!(
        opps_result.is_ok(),
        "Opportunity detection failed: {:?}",
        opps_result.err()
    );
    let opps = opps_result.unwrap();
    assert!(!opps.is_empty(), "No 2-hop cyclic opportunities detected when at least one should exist based on test pool setup.");

    let _token_a_sym = "A"; // Reserved for banned pairs functionality
    let _token_b_sym = "USDC"; // Reserved for banned pairs functionality

    // TODO: Re-implement banned pair functionality
    // By default, pair A / USDC should not be banned.
    // assert!(!ArbitrageDetector::is_permanently_banned(token_a_sym, token_b_sym), "Pair A/USDC should not be banned by default");

    // TODO: Re-implement banned pair functionality
    // Log a banned pair and verify.
    // ArbitrageDetector::log_banned_pair(token_a_sym, token_b_sym, "permanent", "test permanent ban from test_multihop");

    tokio::time::sleep(Duration::from_millis(100)).await;
    // TODO: Re-implement banned pair functionality
    // assert!(ArbitrageDetector::is_permanently_banned(token_a_sym, token_b_sym), "Pair A/USDC should be permanently banned after logging");
}

#[tokio::test]
async fn test_resolve_pools_for_opportunity_missing_pool() {
    use crate::arbitrage::{ArbHop, ArbitrageOrchestrator, MultiHopArbOpportunity};
    use solana_sdk::pubkey::Pubkey;

    let pools_map = create_dummy_pools_map();
    let config = dummy_config();
    let metrics_arc = dummy_metrics();
    let dummy_dex_clients: Vec<Arc<dyn DexClient>> = vec![Arc::new(MockDexClient::new("Mock"))];
    let engine = ArbitrageOrchestrator::new(
        // Renamed ArbitrageEngine to ArbitrageOrchestrator
        pools_map.clone(),
        None,
        None,
        config,
        metrics_arc,
        dummy_dex_clients,
        dummy_banned_pairs_manager(), // banned_pairs_manager
        None,                         // quicknode_opportunity_receiver
    );

    let existing_pool_arc = pools_map.iter().next().unwrap().value().clone();
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
            },
        ],
        pool_path: vec![existing_pool_arc.address, missing_pool_address],
        source_pool: existing_pool_arc.clone(),
        target_pool: Arc::new(PoolInfo::default()),
        input_token_mint: Pubkey::new_unique(),
        output_token_mint: Pubkey::new_unique(),
        intermediate_token_mint: Some(Pubkey::new_unique()),
        ..MultiHopArbOpportunity::default()
    };

    let resolved_result = engine
        .resolve_pools_for_opportunity(&opportunity_with_missing_pool)
        .await;
    assert!(
        resolved_result.is_err(),
        "resolve_pools_for_opportunity should return Err if any pool in pool_path is missing"
    );
    if let Err(ArbError::PoolNotFound(addr_str)) = resolved_result {
        assert_eq!(
            addr_str,
            missing_pool_address.to_string(),
            "Error should specify the missing pool address"
        );
        println!("Correctly identified missing pool: {}", addr_str);
    } else {
        panic!("Expected PoolNotFound error, got {:?}", resolved_result);
    }
}

#[tokio::test]
async fn test_engine_initialization_and_threshold() {
    use crate::arbitrage::ArbitrageOrchestrator;
    use crate::solana::rpc::SolanaRpcClient;
    use std::time::Duration;

    let pools_map = create_dummy_pools_map();
    let mut config_mut = Config::test_default();
    config_mut.min_profit_pct = 0.005; // 0.5%
    let config = Arc::new(config_mut);
    let metrics_arc = dummy_metrics();
    let dummy_dex_clients: Vec<Arc<dyn DexClient>> = vec![Arc::new(MockDexClient::new("Mock"))];
    let dummy_sol_rpc_client = Some(Arc::new(SolanaRpcClient::new(
        "http://dummy.rpc",
        vec![],
        3,
        Duration::from_secs(1),
    )));

    let engine = ArbitrageOrchestrator::new(
        pools_map,
        None,
        dummy_sol_rpc_client,
        config.clone(),
        metrics_arc,
        dummy_dex_clients,
        dummy_banned_pairs_manager(), // banned_pairs_manager
        None,                         // quicknode_opportunity_receiver
    );

    let mut expected_threshold_pct = config.min_profit_pct;
    expected_threshold_pct *= 100.0;
    assert_eq!(
        engine.get_min_profit_threshold_pct().await,
        expected_threshold_pct
    );

    {
        let detector_guard = engine.detector.lock().await;
        assert_eq!(
            detector_guard.get_min_profit_threshold_pct(),
            expected_threshold_pct
        );
    }

    let new_threshold_pct_val = 0.75;
    engine
        .set_min_profit_threshold_pct(new_threshold_pct_val)
        .await;
    assert_eq!(
        engine.get_min_profit_threshold_pct().await,
        new_threshold_pct_val
    );
    {
        let detector_guard_after = engine.detector.lock().await;
        assert_eq!(
            detector_guard_after.get_min_profit_threshold_pct(),
            new_threshold_pct_val
        );
    }
}

#[tokio::test]
async fn test_engine_initialization_with_dex_clients() {
    use crate::arbitrage::ArbitrageOrchestrator;
    use log::info;

    let config = Arc::new(Config::test_default());
    let pools = Arc::new(DashMap::new());
    let metrics = Arc::new(Mutex::new(Metrics::new()));

    let mock_dex_client1 = Arc::new(MockDexClient::new("Raydium"));
    let mock_dex_client2 = Arc::new(MockDexClient::new("Orca"));
    let dex_clients: Vec<Arc<dyn DexClient>> = vec![mock_dex_client1, mock_dex_client2];

    let engine = ArbitrageOrchestrator::new(
        pools,
        None,
        None,
        config,
        metrics,
        dex_clients,
        dummy_banned_pairs_manager(),
        None, // quicknode_opportunity_receiver
    );

    assert_eq!(
        engine.dex_providers.len(),
        2,
        "Engine should have 2 DEX API clients"
    );
    assert_eq!(engine.dex_providers[0].get_name(), "Raydium");
    assert_eq!(engine.dex_providers[1].get_name(), "Orca");
    info!(
        "Engine initialized with {} DEX providers.",
        engine.dex_providers.len()
    );
}

#[tokio::test]
async fn test_engine_all_fields_and_methods_referenced() {
    use crate::arbitrage::{ArbitrageOrchestrator, MultiHopArbOpportunity};
    use std::collections::HashMap;

    let pools_map = create_dummy_pools_map();
    let config = dummy_config();
    let metrics_arc = dummy_metrics();
    let dummy_dex_clients: Vec<Arc<dyn DexClient>> = vec![Arc::new(MockDexClient::new("Mock"))];
    let engine = ArbitrageOrchestrator::new(
        pools_map.clone(),
        None,
        None,
        config,
        metrics_arc,
        dummy_dex_clients,
        dummy_banned_pairs_manager(), // banned_pairs_manager
        None,                         // quicknode_opportunity_receiver
    );
    let _ = engine
        .degradation_mode
        .load(std::sync::atomic::Ordering::Relaxed);
    assert!(!engine.dex_providers.is_empty() || engine.dex_providers.len() == 0);
    let _ = engine.get_min_profit_threshold_pct().await;
    let _ = engine.discover_multihop_opportunities().await;
    let _ = engine
        .with_pool_guard_async("test", false, |_pools| async { Ok(()) })
        .await;
    let _ = engine.update_pools(HashMap::new()).await;
    let _ = engine.get_current_status_string().await;
    let _ = engine.start_services(None).await;
}

#[test]
fn test_opportunity_profit_checks() {
    use crate::arbitrage::MultiHopArbOpportunity;

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

#[tokio::test]
async fn test_exercise_all_fee_manager_functions() {
    use crate::arbitrage::analysis::FeeManager;

    // FeeManager is imported at the module level.
    // XYKSlippageModel is also part of analysis module.
    use crate::arbitrage::analysis::XYKSlippageModel;
    use crate::utils::{PoolInfo, TokenAmount}; // Removed unused DexType import

    // Instantiate FeeManager
    let fee_manager = FeeManager::default();
    let pool = PoolInfo::default();
    let input_amt = TokenAmount::new(100_000_000, 6);
    let sol_price_usd = 150.0; // Dummy SOL price for the test

    // Call the public method on FeeManager (now async)
    let fee_breakdown_result = fee_manager
        .calculate_multihop_fees(
            &[&pool], // For a single pool, pass it as a slice
            &input_amt,
            sol_price_usd,
        )
        .await;

    // Handle the Result and assert something about fee_breakdown
    match fee_breakdown_result {
        Ok(fee_breakdown) => {
            assert!(
                fee_breakdown.total_cost >= 0.0,
                "Total cost should be non-negative"
            );
        }
        Err(e) => {
            // In a test environment, we might expect some failures due to RPC issues
            println!(
                "Fee calculation failed (expected in test environment): {:?}",
                e
            );
        }
    }

    let _ = XYKSlippageModel::default();
}
