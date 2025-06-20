//! Integration tests for Jupiter fallback functionality
//!
//! These tests verify that the Jupiter fallback integration works correctly
//! with the arbitrage orchestrator and price aggregator.

use dashmap::DashMap;
use solana_arb_bot::{
    arbitrage::{
        orchestrator::core::OrchestratorDeps,
        orchestrator::ArbitrageOrchestrator,
        price_aggregator::{PriceAggregator, QuoteSource},
    },
    config::Config,
    dex::{api::Quote, BannedPairsManager, DexClient},
    monitoring::LocalMetrics as Metrics,
    utils::PoolInfo,
};
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Mock DEX client for testing
struct MockDexClient {
    name: String,
    should_fail: bool,
}

impl MockDexClient {
    fn new(name: &str, should_fail: bool) -> Self {
        Self {
            name: name.to_string(),
            should_fail,
        }
    }
}

#[async_trait::async_trait]
impl DexClient for MockDexClient {
    fn get_name(&self) -> &str {
        &self.name
    }

    fn calculate_onchain_quote(&self, pool: &PoolInfo, input_amount: u64) -> anyhow::Result<Quote> {
        if self.should_fail {
            return Err(anyhow::anyhow!("Mock DEX client failure"));
        }

        Ok(Quote {
            input_token: pool.token_a.symbol.clone(),
            output_token: pool.token_b.symbol.clone(),
            input_amount,
            output_amount: input_amount / 2, // Mock 2:1 conversion
            dex: self.name.clone(),
            route: vec![pool.address],
            slippage_estimate: Some(0.01),
        })
    }

    fn get_swap_instruction(
        &self,
        _swap_info: &solana_arb_bot::dex::api::SwapInfo,
    ) -> anyhow::Result<solana_sdk::instruction::Instruction> {
        Ok(solana_sdk::instruction::Instruction {
            program_id: Pubkey::default(),
            accounts: vec![],
            data: vec![],
        })
    }

    async fn get_swap_instruction_enhanced(
        &self,
        _swap_info: &solana_arb_bot::dex::api::CommonSwapInfo,
        _pool_info: Arc<PoolInfo>,
    ) -> Result<solana_sdk::instruction::Instruction, solana_arb_bot::error::ArbError> {
        Ok(solana_sdk::instruction::Instruction {
            program_id: Pubkey::default(),
            accounts: vec![],
            data: vec![],
        })
    }

    async fn discover_pools(&self) -> anyhow::Result<Vec<PoolInfo>> {
        Ok(vec![])
    }

    async fn health_check(
        &self,
    ) -> Result<solana_arb_bot::dex::api::DexHealthStatus, solana_arb_bot::error::ArbError> {
        Ok(solana_arb_bot::dex::api::DexHealthStatus {
            is_healthy: true,
            last_successful_request: Some(std::time::Instant::now()),
            error_count: 0,
            response_time_ms: Some(100),
            pool_count: Some(0),
            status_message: "Mock DEX is healthy".to_string(),
        })
    }
}

#[tokio::test]
async fn test_price_aggregator_with_failing_primary_sources() {
    // Create test config with Jupiter fallback enabled
    let mut config = Config::test_default();
    config.jupiter_fallback_enabled = true;
    config.jupiter_fallback_min_profit_pct = 0.001;

    let metrics = Arc::new(Mutex::new(Metrics::new()));

    // Create mock DEX clients that fail
    let failing_dex_clients: Vec<Arc<dyn DexClient>> = vec![
        Arc::new(MockDexClient::new("MockDEX1", true)),
        Arc::new(MockDexClient::new("MockDEX2", true)),
    ];

    // Create price aggregator without Jupiter client (simulating failure)
    let price_aggregator = PriceAggregator::new(
        failing_dex_clients,
        None, // No Jupiter client for this test
        &config,
        Arc::clone(&metrics),
    );

    // Create test pool
    let pool = PoolInfo::default();

    // Test that aggregator fails when all sources fail
    let result = price_aggregator.get_best_quote(&pool, 1000000).await;
    assert!(
        result.is_err(),
        "Expected aggregator to fail when all sources fail"
    );
}

#[tokio::test]
async fn test_price_aggregator_with_working_primary_sources() {
    // Create test config
    let config = Config::test_default();
    let metrics = Arc::new(Mutex::new(Metrics::new()));

    // Create mock DEX clients that work
    let working_dex_clients: Vec<Arc<dyn DexClient>> = vec![
        Arc::new(MockDexClient::new("MockDEX1", false)),
        Arc::new(MockDexClient::new("MockDEX2", false)),
    ];

    // Create price aggregator
    let price_aggregator = PriceAggregator::new(
        working_dex_clients,
        None, // No Jupiter client needed for this test
        &config,
        Arc::clone(&metrics),
    );

    // Create test pool
    let pool = PoolInfo::default();

    // Test that aggregator succeeds with working sources
    let result = price_aggregator.get_best_quote(&pool, 1000000).await;
    assert!(
        result.is_ok(),
        "Expected aggregator to succeed with working sources"
    );

    let aggregated_quote = result.unwrap();
    assert_eq!(aggregated_quote.quote.input_amount, 1000000);
    assert_eq!(aggregated_quote.quote.output_amount, 500000); // Mock 2:1 conversion

    // Verify it used a primary source
    match aggregated_quote.source {
        QuoteSource::Primary(_) => { /* Expected */ }
        _ => panic!("Expected primary source to be used"),
    }
}

#[tokio::test]
async fn test_orchestrator_integration_with_price_aggregator() {
    // Create test config
    let mut config = Config::test_default();
    config.jupiter_fallback_enabled = true;
    let config = Arc::new(config);

    // Create test components
    let hot_cache = Arc::new(DashMap::new());
    let metrics = Arc::new(Mutex::new(Metrics::new()));
    let banned_pairs_manager =
        Arc::new(BannedPairsManager::new("test_banned_pairs.csv".to_string()).unwrap());

    // Create mock DEX clients
    let dex_clients: Vec<Arc<dyn DexClient>> = vec![Arc::new(MockDexClient::new("MockDEX", false))];

    // Create orchestrator
    let orchestrator = ArbitrageOrchestrator::new(
        hot_cache,
        OrchestratorDeps {
            ws_manager: None,
            rpc_client: None,
            metrics,
            dex_providers: dex_clients,
            banned_pairs_manager,
        },
        config,
        None,
    );

    // Create test pool and add to cache
    let test_pool = PoolInfo::default();
    let pool_address = test_pool.address;
    orchestrator
        .hot_cache
        .insert(pool_address, Arc::new(test_pool.clone()));

    // Test get_aggregated_quote method
    let result = orchestrator.get_aggregated_quote(&test_pool, 1000000).await;
    assert!(
        result.is_ok(),
        "Expected orchestrator to provide aggregated quote"
    );

    let aggregated_quote = result.unwrap();
    assert_eq!(aggregated_quote.quote.input_amount, 1000000);
}

#[tokio::test]
async fn test_orchestrator_fallback_to_traditional_quotes() {
    // Create test config with Jupiter fallback disabled
    let mut config = Config::test_default();
    config.jupiter_fallback_enabled = false;
    let config = Arc::new(config);

    // Create test components
    let hot_cache = Arc::new(DashMap::new());
    let metrics = Arc::new(Mutex::new(Metrics::new()));
    let banned_pairs_manager =
        Arc::new(BannedPairsManager::new("test_banned_pairs.csv".to_string()).unwrap());

    // Create mock DEX clients
    let dex_clients: Vec<Arc<dyn DexClient>> = vec![
        Arc::new(MockDexClient::new("MockDEX1", false)),
        Arc::new(MockDexClient::new("MockDEX2", false)),
    ];

    // Create orchestrator
    let orchestrator = ArbitrageOrchestrator::new(
        hot_cache,
        OrchestratorDeps {
            ws_manager: None,
            rpc_client: None,
            metrics,
            dex_providers: dex_clients,
            banned_pairs_manager,
        },
        config,
        None,
    );

    // Create test pool
    let test_pool = PoolInfo::default();

    // Test get_aggregated_quote method falls back to traditional method
    let result = orchestrator.get_aggregated_quote(&test_pool, 1000000).await;
    assert!(
        result.is_ok(),
        "Expected orchestrator to provide traditional quote"
    );

    let aggregated_quote = result.unwrap();
    assert_eq!(aggregated_quote.quote.input_amount, 1000000);
}
