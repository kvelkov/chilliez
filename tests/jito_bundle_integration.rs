use dashmap::DashMap;
use solana_arb_bot::{
    arbitrage::{
        execution::HftExecutor,
        opportunity::{ArbHop, MultiHopArbOpportunity},
    },
    config::settings::Config,
    dex::{api::DexClient, clients::{OrcaClient, RaydiumClient}},
    utils::{DexType, PoolInfo, PoolToken},
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{pubkey::Pubkey, signature::Keypair, system_program};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

// Helper to create a default config for testing Jito integration.
fn create_jito_test_config() -> Config {
    // A dummy keypair for testing purposes.
    let dummy_wallet_path = "/tmp/dummy_wallet.json";
    let keypair = Keypair::new();
    let keypair_bytes = keypair.to_bytes();
    std::fs::write(
        dummy_wallet_path,
        format!(
            "[{}]",
            keypair_bytes
                .iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ),
    )
    .unwrap();

    Config {
        rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
        rpc_url_secondary: None,
        ws_url: "wss://api.mainnet-beta.solana.com".to_string(),
        wallet_path: dummy_wallet_path.to_string(),
        min_profit_pct: 0.1,
        min_profit_usd_threshold: Some(1.0),
        sol_price_usd: Some(150.0),
        max_slippage_pct: 1.0,
        transaction_priority_fee_lamports: 10000,
        default_priority_fee_lamports: 5000,
        pool_refresh_interval_secs: 10,
        pool_read_timeout_ms: Some(5000),
        health_check_interval_secs: Some(60),
        max_ws_reconnect_attempts: Some(5),
        log_level: Some("info".to_string()),
        redis_url: "redis://127.0.0.1/".to_string(),
        redis_default_ttl_secs: 60,
        dex_quote_cache_ttl_secs: None,
        volatility_tracker_window: None,
        volatility_threshold_factor: None,
        dynamic_threshold_update_interval_secs: None,
        degradation_profit_factor: None,
        max_tx_fee_lamports_for_acceptance: None,
        max_risk_score_for_acceptance: None,
        max_hops: Some(3),
        max_pools_per_hop: Some(3),
        max_concurrent_executions: Some(10),
        execution_timeout_secs: Some(30),
        transaction_cu_limit: Some(600_000),
        simulation_mode: false,
        paper_trading: false,
        metrics_log_path: None,
        health_check_token_symbol: None,
        enable_fixed_input_arb_detection: false,
        fixed_input_arb_amount: None,
        rpc_url_backup: None,
        rpc_max_retries: Some(3),
        rpc_retry_delay_ms: Some(500),
        trader_wallet_keypair_path: Some(dummy_wallet_path.to_string()),
        max_transaction_timeout_seconds: Some(60),
        ws_update_channel_size: Some(1024),
        congestion_update_interval_secs: Some(30),
        cycle_interval_seconds: Some(5),
        jupiter_fallback_enabled: false,
        jupiter_api_timeout_ms: 10000,
        jupiter_max_retries: 3,
        jupiter_fallback_min_profit_pct: 0.1,
        jupiter_slippage_tolerance_bps: 50,
        jupiter_cache_enabled: false,
        jupiter_cache_ttl_seconds: 30,
        jupiter_cache_max_entries: 10000,
        jupiter_cache_amount_bucket_size: 1000000,
        jupiter_cache_volatility_threshold_pct: 1.0,
        jupiter_route_optimization_enabled: false,
        jupiter_max_parallel_routes: 5,
        jupiter_max_alternative_routes: 3,
        jupiter_route_evaluation_timeout_ms: 5000,
        jupiter_min_route_improvement_pct: 0.1,
        webhook_port: None,
        webhook_url: None,
        enable_webhooks: false,
        enable_jito_bundle: true,
        jito_quicknode_url: Some("http://127.0.0.1:8899".to_string()), // Mock URL
        jito_tip_lamports: Some(10000),
        jito_region: Some("mainnet".to_string()),
        jito_tip_accounts: Some(vec![
            "96gYZGLnZtraKkCsyrJBA6SV9vbmGDvrpq5hGk91ck8u".to_string(), // Jito tip account
        ]),
        jito_dynamic_tip_percentage: Some(0.1),
        jito_bundle_status_poll_interval_ms: Some(500),
        jito_bundle_status_timeout_secs: Some(5),
    }
}

// Helper to create a mock opportunity
fn create_mock_opportunity() -> MultiHopArbOpportunity {
    MultiHopArbOpportunity {
        id: "test_jito_opp".to_string(),
        hops: vec![
            ArbHop {
                dex: DexType::Orca,
                pool: Pubkey::new_unique(),
                input_token: "SOL".to_string(),
                output_token: "USDC".to_string(),
                input_amount: 1.0,
                expected_output: 150.0,
            },
            ArbHop {
                dex: DexType::Raydium,
                pool: Pubkey::new_unique(),
                input_token: "USDC".to_string(),
                output_token: "SOL".to_string(),
                input_amount: 150.0,
                expected_output: 1.01,
            },
        ],
        input_token: "SOL".to_string(),
        input_amount: 1.0,
        expected_output: 1.01,
        total_profit: 0.01,
        profit_pct: 1.0,
        estimated_profit_usd: Some(1.5),
        ..Default::default()
    }
}

#[tokio::test]
#[ignore] // This test makes real network calls and requires a valid QuickNode endpoint.
async fn test_jito_bundle_execution_flow() {
    let config = Arc::new(create_jito_test_config());
    let wallet = Arc::new(Keypair::new());
    let rpc_client = Arc::new(RpcClient::new(config.rpc_url.clone()));
    let metrics = Arc::new(Mutex::new(solana_arb_bot::local_metrics::Metrics::new()));
    let hot_cache = Arc::new(DashMap::new());

    // Populate hot_cache with dummy pool data for instruction building
    let sol_info = PoolToken {
        mint: system_program::id(), // Using system_program id as a stand-in for SOL mint
        symbol: "SOL".to_string(),
        decimals: 9,
        reserve: 0,
    };
    let usdc_info = PoolToken {
        mint: Pubkey::new_unique(), // Dummy USDC mint
        symbol: "USDC".to_string(),
        decimals: 6,
        reserve: 0,
    };

    let opportunity = create_mock_opportunity();

    let pool1 = PoolInfo {
        address: opportunity.hops[0].pool,
        dex_type: DexType::Orca,
        token_a: sol_info.clone(),
        token_b: usdc_info.clone(),
        ..Default::default()
    };
    let pool2 = PoolInfo {
        address: opportunity.hops[1].pool,
        dex_type: DexType::Raydium,
        token_a: usdc_info.clone(),
        token_b: sol_info.clone(),
        ..Default::default()
    };
    hot_cache.insert(pool1.address, Arc::new(pool1));
    hot_cache.insert(pool2.address, Arc::new(pool2));

    let mut executor = HftExecutor::new(
        wallet,
        rpc_client,
        None,
        config.clone(),
        metrics,
        hot_cache,
    );

    // Mock DEX clients
    let mut dex_clients: HashMap<DexType, Box<dyn DexClient>> = HashMap::new();
    dex_clients.insert(DexType::Orca, Box::new(OrcaClient::new()));
    dex_clients.insert(DexType::Raydium, Box::new(RaydiumClient::new()));
    executor.set_dex_clients(Arc::new(Mutex::new(dex_clients)));

    // This will attempt to send a bundle to the configured jito_quicknode_url.
    // In a real CI/CD environment, this URL should be a mock server.
    // The test is expected to fail if the endpoint is not reachable or rejects the bundle.
    let result = executor.execute_opportunity_atomic_jito(&opportunity).await;

    // We expect an error here because we are not providing real, executable transactions.
    // The goal is to verify that the flow reaches the point of submission and polling.
    assert!(
        result.is_err(),
        "Expected Jito execution to fail without valid transactions and a real endpoint."
    );

    // More specific assertions can be added if a mock server is used.
    // For example, checking if the error message indicates a timeout or a specific API error.
    let error_message = result.unwrap_err();
    println!("Jito execution failed as expected: {}", error_message);
    assert!(error_message.contains("Error polling for bundle"));
}
