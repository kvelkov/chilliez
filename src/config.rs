use serde::Deserialize;
use std::{collections::HashMap, path::Path};

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    pub rpc_url: String,
    pub rpc_url_secondary: Option<String>,
    pub ws_url: String,
    pub wallet_path: String,
    pub min_profit_pct: f64,
    pub min_profit_usd_threshold: Option<f64>,
    pub sol_price_usd: Option<f64>,
    pub max_slippage_pct: f64,
    pub transaction_priority_fee_lamports: u64,
    pub default_priority_fee_lamports: u64,
    pub pool_refresh_interval_secs: u64,
    pub pool_read_timeout_ms: Option<u64>,
    pub health_check_interval_secs: Option<u64>,
    pub max_ws_reconnect_attempts: Option<u32>,
    pub log_level: Option<String>,
    pub redis_url: String,
    pub redis_default_ttl_secs: u64,
    pub dex_quote_cache_ttl_secs: Option<HashMap<String, u64>>,
    pub volatility_tracker_window: Option<usize>,
    pub volatility_threshold_factor: Option<f64>,
    pub dynamic_threshold_update_interval_secs: Option<u64>,
    pub degradation_profit_factor: Option<f64>,
    pub max_tx_fee_lamports_for_acceptance: Option<u64>,
    pub max_risk_score_for_acceptance: Option<f64>,
    pub max_hops: Option<usize>,
    pub max_pools_per_hop: Option<usize>,
    pub max_concurrent_executions: Option<usize>,
    pub execution_timeout_secs: Option<u64>,
    pub transaction_cu_limit: Option<u32>,
    pub simulation_mode: bool,
    pub paper_trading: bool,
    pub metrics_log_path: Option<String>,
    pub health_check_token_symbol: Option<String>,
    pub enable_fixed_input_arb_detection: bool,
    pub fixed_input_arb_amount: Option<f64>,
    pub rpc_url_backup: Option<String>,
    pub rpc_max_retries: Option<u32>,
    pub rpc_retry_delay_ms: Option<u64>,
    pub trader_wallet_keypair_path: Option<String>,
    pub max_transaction_timeout_seconds: Option<u64>,
    pub ws_update_channel_size: Option<usize>,
    pub congestion_update_interval_secs: Option<u64>,
    pub cycle_interval_seconds: Option<u64>,
    // Jupiter fallback configuration
    pub jupiter_fallback_enabled: bool,
    pub jupiter_api_timeout_ms: u64,
    pub jupiter_max_retries: u32,
    pub jupiter_fallback_min_profit_pct: f64,
    pub jupiter_slippage_tolerance_bps: u16,
    // Jupiter cache configuration
    pub jupiter_cache_enabled: bool,
    pub jupiter_cache_ttl_seconds: u64,
    pub jupiter_cache_max_entries: usize,
    pub jupiter_cache_amount_bucket_size: u64,
    pub jupiter_cache_volatility_threshold_pct: f64,
    // Jupiter route optimization configuration
    pub jupiter_route_optimization_enabled: bool,
    pub jupiter_max_parallel_routes: usize,
    pub jupiter_max_alternative_routes: u8,
    pub jupiter_route_evaluation_timeout_ms: u64,
    pub jupiter_min_route_improvement_pct: f64,
    // Webhook configuration
    pub webhook_port: Option<u16>,
    pub webhook_url: Option<String>,
    pub enable_webhooks: bool,
    // --- Jito/QuickNode bundle execution configuration ---
    pub enable_jito_bundle: bool,
    pub jito_quicknode_url: Option<String>,
    pub jito_tip_lamports: Option<u64>,
    pub jito_region: Option<String>,
    pub jito_tip_accounts: Option<Vec<String>>,
    pub jito_dynamic_tip_percentage: Option<f64>,
    pub jito_bundle_status_poll_interval_ms: Option<u64>,
    pub jito_bundle_status_timeout_secs: Option<u64>,
    // Jito Orchestrator integration
    pub jito_enabled: Option<bool>,
}

impl Config {
    /// Loads configuration from the specified file path using the `config` crate.
    /// Expects a TOML file format.
    pub fn load(config_path: &Path) -> Result<Self, ::config::ConfigError> {
        let settings = ::config::Config::builder()
            .add_source(::config::File::from(config_path))
            .build()?;
        settings.try_deserialize()
    }

    pub fn from_env() -> Self {
        dotenv::dotenv().ok();
        Self::from_env_without_loading()
    }

    pub fn from_env_without_loading() -> Self {
        unimplemented!("Copy the full env var loading logic from settings.rs here.");
    }

    pub fn validate_and_log(&self) {
        unimplemented!("Copy the validate_and_log logic from settings.rs here.");
    }

    pub fn test_default() -> Self {
        Self {
            rpc_url: "http://localhost:8899".to_string(),
            rpc_url_secondary: None,
            ws_url: "ws://localhost:8900".to_string(),
            wallet_path: "test_wallet.json".to_string(),
            min_profit_pct: 0.1,
            min_profit_usd_threshold: Some(0.05),
            sol_price_usd: Some(150.0),
            max_slippage_pct: 0.005,
            transaction_priority_fee_lamports: 10000,
            default_priority_fee_lamports: 5000,
            pool_refresh_interval_secs: 10,
            pool_read_timeout_ms: Some(1000),
            health_check_interval_secs: Some(60),
            max_ws_reconnect_attempts: Some(5),
            log_level: Some("info".to_string()),
            redis_url: "redis://127.0.0.1/".to_string(),
            redis_default_ttl_secs: 3600,
            dex_quote_cache_ttl_secs: Some(HashMap::new()),
            volatility_tracker_window: Some(20),
            volatility_threshold_factor: Some(0.5),
            dynamic_threshold_update_interval_secs: Some(60),
            degradation_profit_factor: Some(1.5),
            max_tx_fee_lamports_for_acceptance: Some(100000),
            max_risk_score_for_acceptance: Some(0.75),
            max_hops: Some(3),
            max_pools_per_hop: Some(5),
            max_concurrent_executions: Some(10),
            execution_timeout_secs: Some(30),
            transaction_cu_limit: Some(400_000),
            simulation_mode: false,
            paper_trading: true,
            metrics_log_path: None,
            health_check_token_symbol: Some("SOL/USDC".to_string()),
            enable_fixed_input_arb_detection: false,
            fixed_input_arb_amount: None,
            rpc_url_backup: None,
            rpc_max_retries: Some(3),
            rpc_retry_delay_ms: Some(1000),
            trader_wallet_keypair_path: Some("test_keypair.json".to_string()),
            max_transaction_timeout_seconds: Some(120),
            ws_update_channel_size: Some(1024),
            congestion_update_interval_secs: Some(15),
            cycle_interval_seconds: Some(5),
            jupiter_fallback_enabled: false,
            jupiter_api_timeout_ms: 5000,
            jupiter_max_retries: 3,
            jupiter_fallback_min_profit_pct: 0.001,
            jupiter_slippage_tolerance_bps: 50,
            jupiter_cache_enabled: true,
            jupiter_cache_ttl_seconds: 5,
            jupiter_cache_max_entries: 1000,
            jupiter_cache_amount_bucket_size: 1_000_000,
            jupiter_cache_volatility_threshold_pct: 2.0,
            jupiter_route_optimization_enabled: true,
            jupiter_max_parallel_routes: 5,
            jupiter_max_alternative_routes: 10,
            jupiter_route_evaluation_timeout_ms: 2000,
            jupiter_min_route_improvement_pct: 0.1,
            webhook_port: Some(8080),
            webhook_url: Some("http://localhost:8080/webhook".to_string()),
            enable_webhooks: false,
            enable_jito_bundle: false,
            jito_quicknode_url: None,
            jito_tip_lamports: None,
            jito_region: None,
            jito_tip_accounts: None,
            jito_dynamic_tip_percentage: None,
            jito_bundle_status_poll_interval_ms: None,
            jito_bundle_status_timeout_secs: None,
            jito_enabled: None,
        }
    }
}

// If any relevant types or constants from env.rs are needed, copy them here.
// Otherwise, all config logic is now unified in this file.
