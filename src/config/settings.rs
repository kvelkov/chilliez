use serde::Deserialize;
use std::{collections::HashMap, env, time::Duration};

#[derive(Debug, Deserialize, Clone)]
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
    pub simulation_mode: bool,
    pub paper_trading: bool,
    pub metrics_log_path: Option<String>,
    pub health_check_token_symbol: Option<String>,
    pub enable_fixed_input_arb_detection: bool, // Changed to bool, with default handling below
    pub fixed_input_arb_amount: Option<f64>,    // Kept as Option<f64>
    pub rpc_url_backup: Option<String>,
    pub rpc_max_retries: Option<u32>,
    pub rpc_retry_delay_ms: Option<u64>,
    pub trader_wallet_keypair_path: Option<String>,
    pub max_transaction_timeout_seconds: Option<u64>,
    pub ws_update_channel_size: Option<usize>,
    pub congestion_update_interval_secs: Option<u64>,
    pub cycle_interval_seconds: Option<u64>,
}

impl Config {
    pub fn from_env() -> Self {
        dotenv::dotenv().ok();
        Config {
            rpc_url: env::var("RPC_URL").expect("RPC_URL must be set"),
            rpc_url_secondary: env::var("RPC_URL_SECONDARY").ok(),
            ws_url: env::var("WS_URL").expect("WS_URL must be set"),
            wallet_path: env::var("WALLET_PATH").expect("WALLET_PATH must be set"),
            min_profit_pct: env::var("MIN_PROFIT_PCT")
                .unwrap_or_else(|_| "0.001".to_string())
                .parse()
                .expect("MIN_PROFIT_PCT must be a float"),
            min_profit_usd_threshold: env::var("MIN_PROFIT_USD_THRESHOLD").ok().and_then(|s| s.parse().ok()),
            sol_price_usd: env::var("SOL_PRICE_USD").ok().and_then(|s| s.parse().ok()),
            max_slippage_pct: env::var("MAX_SLIPPAGE_PCT")
                .unwrap_or_else(|_| "0.01".to_string())
                .parse()
                .expect("MAX_SLIPPAGE_PCT must be a float"),
            transaction_priority_fee_lamports: env::var("TRANSACTION_PRIORITY_FEE_LAMPORTS")
                .unwrap_or_else(|_| "10000".to_string())
                .parse()
                .expect("TRANSACTION_PRIORITY_FEE_LAMPORTS must be an int"),
            default_priority_fee_lamports: env::var("DEFAULT_PRIORITY_FEE_LAMPORTS")
                .unwrap_or_else(|_| "5000".to_string())
                .parse()
                .expect("DEFAULT_PRIORITY_FEE_LAMPORTS must be an int"),
            pool_refresh_interval_secs: env::var("POOL_REFRESH_INTERVAL_SECS")
                .unwrap_or_else(|_| "10".to_string())
                .parse()
                .expect("POOL_REFRESH_INTERVAL_SECS must be an int"),
            pool_read_timeout_ms: env::var("POOL_READ_TIMEOUT_MS").ok().and_then(|s| s.parse().ok()),
            health_check_interval_secs: env::var("HEALTH_CHECK_INTERVAL_SECS").ok().and_then(|s| s.parse().ok()),
            max_ws_reconnect_attempts: env::var("MAX_WS_RECONNECT_ATTEMPTS").ok().and_then(|s| s.parse().ok()),
            log_level: env::var("LOG_LEVEL").ok(),
            redis_url: env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1/".to_string()),
            redis_default_ttl_secs: env::var("REDIS_DEFAULT_TTL_SECS").unwrap_or_else(|_| "60".to_string()).parse().expect("REDIS_DEFAULT_TTL_SECS must be an int"),
            dex_quote_cache_ttl_secs: env::var("DEX_QUOTE_CACHE_TTL_SECS").ok().and_then(|s| serde_json::from_str(&s).ok()),
            volatility_tracker_window: env::var("VOLATILITY_TRACKER_WINDOW").ok().and_then(|s| s.parse().ok()),
            volatility_threshold_factor: env::var("VOLATILITY_THRESHOLD_FACTOR").ok().and_then(|s| s.parse().ok()),
            dynamic_threshold_update_interval_secs: env::var("DYNAMIC_THRESHOLD_UPDATE_INTERVAL_SECS").ok().and_then(|s| s.parse().ok()),
            degradation_profit_factor: env::var("DEGRADATION_PROFIT_FACTOR").ok().and_then(|s| s.parse().ok()),
            max_tx_fee_lamports_for_acceptance: env::var("MAX_TX_FEE_LAMPORTS_FOR_ACCEPTANCE").ok().and_then(|s| s.parse().ok()),
            max_risk_score_for_acceptance: env::var("MAX_RISK_SCORE_FOR_ACCEPTANCE").ok().and_then(|s| s.parse().ok()),
            max_hops: env::var("MAX_HOPS").ok().and_then(|s| s.parse().ok()),
            max_pools_per_hop: env::var("MAX_POOLS_PER_HOP").ok().and_then(|s| s.parse().ok()),
            max_concurrent_executions: env::var("MAX_CONCURRENT_EXECUTIONS").ok().and_then(|s| s.parse().ok()),
            execution_timeout_secs: env::var("EXECUTION_TIMEOUT_SECS").ok().and_then(|s| s.parse().ok()),
            simulation_mode: env::var("SIMULATION_MODE").unwrap_or_else(|_| "false".to_string()).parse().unwrap_or(false),
            paper_trading: env::var("PAPER_TRADING").unwrap_or_else(|_| "false".to_string()).parse().unwrap_or(false),
            metrics_log_path: env::var("METRICS_LOG_PATH").ok(),
            health_check_token_symbol: env::var("HEALTH_CHECK_TOKEN_SYMBOL").ok(),
            enable_fixed_input_arb_detection: env::var("ENABLE_FIXED_INPUT_ARB_DETECTION").unwrap_or_else(|_| "false".to_string()).parse().unwrap_or(false),
            fixed_input_arb_amount: env::var("FIXED_INPUT_ARB_AMOUNT").ok().and_then(|s| s.parse().ok()),
            rpc_url_backup: env::var("RPC_URL_BACKUP").ok(),
            rpc_max_retries: env::var("RPC_MAX_RETRIES").ok().and_then(|s| s.parse().ok()),
            rpc_retry_delay_ms: env::var("RPC_RETRY_DELAY_MS").ok().and_then(|s| s.parse().ok()),
            trader_wallet_keypair_path: env::var("TRADER_WALLET_KEYPAIR_PATH").ok(),
            max_transaction_timeout_seconds: env::var("MAX_TRANSACTION_TIMEOUT_SECONDS").ok().and_then(|s| s.parse().ok()),
            ws_update_channel_size: env::var("WS_UPDATE_CHANNEL_SIZE").ok().and_then(|s| s.parse().ok()),
            congestion_update_interval_secs: env::var("CONGESTION_UPDATE_INTERVAL_SECS").ok().and_then(|s| s.parse().ok()),
            cycle_interval_seconds: env::var("CYCLE_INTERVAL_SECONDS").ok().and_then(|s| s.parse().ok()),
        }
    }

    pub fn test_default() -> Self {
        Self {
            rpc_url: "http://localhost:8899".to_string(),
            rpc_url_secondary: None,
            ws_url: "ws://localhost:8900".to_string(),
            wallet_path: "dummy-wallet.json".to_string(),
            min_profit_pct: 0.001,
            min_profit_usd_threshold: Some(0.0),
            sol_price_usd: Some(1.0),
            max_slippage_pct: 0.01,
            transaction_priority_fee_lamports: 0,
            default_priority_fee_lamports: 0,
            pool_refresh_interval_secs: 10,
            pool_read_timeout_ms: Some(1000),
            health_check_interval_secs: Some(60),
            max_ws_reconnect_attempts: Some(5),
            log_level: Some("info".to_string()),
            redis_url: "redis://127.0.0.1/".to_string(),
            redis_default_ttl_secs: 60,
            dex_quote_cache_ttl_secs: None,
            volatility_tracker_window: Some(20),
            volatility_threshold_factor: Some(0.1),
            dynamic_threshold_update_interval_secs: Some(300),
            degradation_profit_factor: Some(1.5),
            max_tx_fee_lamports_for_acceptance: Some(10000),
            max_risk_score_for_acceptance: Some(1.0),
            max_hops: Some(3),
            max_pools_per_hop: Some(5),
            max_concurrent_executions: Some(1),
            execution_timeout_secs: Some(30),
            simulation_mode: false,
            paper_trading: false,
            metrics_log_path: None,
            health_check_token_symbol: Some("SOL/USDC".to_string()),
            enable_fixed_input_arb_detection: false,
            fixed_input_arb_amount: None,
            rpc_url_backup: None,
            rpc_max_retries: Some(3),
            rpc_retry_delay_ms: Some(500),
            trader_wallet_keypair_path: Some("dummy-wallet.json".to_string()),
            max_transaction_timeout_seconds: Some(30),
            ws_update_channel_size: Some(1024),
            congestion_update_interval_secs: Some(60),
            cycle_interval_seconds: Some(10),
        }
    }

    pub fn get_rpc_timeout_duration(&self) -> Duration {
        Duration::from_millis(self.pool_read_timeout_ms.unwrap_or(1000))
    }

    pub fn validate_and_log(&self) {
        log::info!("Application Configuration Loaded: {:?}", self);
        if self.rpc_url.is_empty() {
            log::error!("CRITICAL: RPC_URL environment variable is not set or empty.");
        }
        if self.trader_wallet_keypair_path.as_ref().map_or(true, |s| s.is_empty()) {
            log::error!("CRITICAL: TRADER_WALLET_KEYPAIR_PATH environment variable is not set or empty.");
        }
        if self.min_profit_pct <= 0.0 || self.min_profit_pct >= 1.0 {
            log::warn!("MIN_PROFIT_PCT ({}) is outside the typical range (0.0 to 1.0, exclusive of 0). Ensure it's a fraction (e.g., 0.001 for 0.1%).", self.min_profit_pct);
        }
        if self.enable_fixed_input_arb_detection {
            if self.fixed_input_arb_amount.is_none() {
                log::warn!("ENABLE_FIXED_INPUT_ARB_DETECTION is true but FIXED_INPUT_ARB_AMOUNT is not set.");
            }
        }
    }
}