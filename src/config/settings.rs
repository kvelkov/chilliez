use std::collections::HashMap;
use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub redis_url: String,
    pub redis_default_ttl_secs: u64,
    pub dex_quote_cache_ttl_secs: Option<HashMap<String, u64>>,
    pub volatility_tracker_window: Option<usize>,
    pub dynamic_threshold_update_interval_secs: Option<u64>,
    pub volatility_threshold_factor: Option<f64>,
    pub congestion_update_interval_secs: Option<u64>,
    pub execution_chunk_size: Option<u8>,
    pub sol_price_usd: Option<f64>,
    pub degradation_profit_factor: Option<f64>,
    pub degradation_slippage_factor: Option<f64>,
    pub pool_read_timeout_ms: Option<u64>,
    pub health_check_token_symbol: Option<String>,
    pub rpc_url: String,
    pub rpc_url_backup: Option<Vec<String>>,
    pub rpc_max_retries: Option<usize>,
    pub rpc_retry_delay_ms: Option<u64>,
    pub trader_wallet_keypair_path: String,
    pub default_priority_fee_lamports: u64,
    pub max_transaction_timeout_seconds: u64,
    pub paper_trading: bool,
    pub ws_url: String,
    pub ws_update_channel_size: Option<usize>,
    pub min_profit_pct: f64,
    pub max_slippage_pct: f64,
    pub cycle_interval_seconds: u64,
    pub health_check_interval_secs: Option<u64>,
    pub max_ws_reconnect_attempts: Option<u8>,
    pub metrics_log_path: Option<String>,
}

impl Config {
    pub fn from_env() -> Self {
        Config {
            redis_url: env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost".to_string()),
            redis_default_ttl_secs: env::var("REDIS_DEFAULT_TTL_SECS")
                .unwrap_or_else(|_| "60".to_string())
                .parse()
                .unwrap_or(60),
            dex_quote_cache_ttl_secs: env::var("DEX_QUOTE_CACHE_TTL_SECS")
                .ok()
                .map(|s| {
                    s.split(',')
                        .filter_map(|part| {
                            let mut kv = part.split(':');
                            let key = kv.next()?.trim().to_string();
                            let value = kv.next()?.trim().parse::<u64>().ok()?;
                            Some((key, value))
                        })
                        .collect()
                }),
            volatility_tracker_window: env::var("VOLATILITY_TRACKER_WINDOW")
                .ok()
                .and_then(|v| v.parse().ok()),
            dynamic_threshold_update_interval_secs: env::var(
                "DYNAMIC_THRESHOLD_UPDATE_INTERVAL_SECS",
            )
            .ok()
            .and_then(|v| v.parse().ok()),
            volatility_threshold_factor: env::var("VOLATILITY_THRESHOLD_FACTOR")
                .ok()
                .and_then(|v| v.parse().ok()),
            congestion_update_interval_secs: env::var("CONGESTION_UPDATE_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok()),
            execution_chunk_size: env::var("EXECUTION_CHUNK_SIZE")
                .ok()
                .and_then(|v| v.parse().ok()),
            sol_price_usd: env::var("SOL_PRICE_USD").ok().and_then(|v| v.parse().ok()),
            degradation_profit_factor: env::var("DEGRADATION_PROFIT_FACTOR")
                .ok()
                .and_then(|v| v.parse().ok()),
            degradation_slippage_factor: env::var("DEGRADATION_SLIPPAGE_FACTOR")
                .ok()
                .and_then(|v| v.parse().ok()),
            pool_read_timeout_ms: env::var("POOL_READ_TIMEOUT_MS")
                .ok()
                .and_then(|v| v.parse().ok()),
            health_check_token_symbol: env::var("HEALTH_CHECK_TOKEN_SYMBOL").ok(),
            rpc_url: env::var("RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8899".to_string()),
            rpc_url_backup: env::var("RPC_URL_BACKUP")
                .ok()
                .map(|s| s.split(',').map(String::from).collect()),
            rpc_max_retries: env::var("RPC_MAX_RETRIES").ok().and_then(|v| v.parse().ok()),
            rpc_retry_delay_ms: env::var("RPC_RETRY_DELAY_MS")
                .ok()
                .and_then(|v| v.parse().ok()),
            trader_wallet_keypair_path: env::var("TRADER_WALLET_KEYPAIR_PATH")
                .unwrap_or_else(|_| ".config/solana/id.json".to_string()),
            default_priority_fee_lamports: env::var("DEFAULT_PRIORITY_FEE_LAMPORTS")
                .unwrap_or_else(|_| "10000".to_string())
                .parse()
                .unwrap_or(10000),
            max_transaction_timeout_seconds: env::var("MAX_TRANSACTION_TIMEOUT_SECONDS")
                .unwrap_or_else(|_| "60".to_string())
                .parse()
                .unwrap_or(60),
            paper_trading: env::var("PAPER_TRADING")
                .unwrap_or_else(|_| "false".to_string())
                .parse()
                .unwrap_or(false),
            ws_url: env::var("WS_URL").unwrap_or_else(|_| "ws://127.0.0.1:8900".to_string()),
            ws_update_channel_size: env::var("WS_UPDATE_CHANNEL_SIZE")
                .ok()
                .and_then(|v| v.parse().ok()),
            min_profit_pct: env::var("MIN_PROFIT_PCT")
                .unwrap_or_else(|_| "0.001".to_string())
                .parse()
                .unwrap_or(0.001),
            max_slippage_pct: env::var("MAX_SLIPPAGE_PCT")
                .unwrap_or_else(|_| "0.005".to_string())
                .parse()
                .unwrap_or(0.005),
            cycle_interval_seconds: env::var("CYCLE_INTERVAL_SECONDS")
                .unwrap_or_else(|_| "5".to_string())
                .parse()
                .unwrap_or(5),
            health_check_interval_secs: env::var("HEALTH_CHECK_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok()),
            max_ws_reconnect_attempts: env::var("MAX_WS_RECONNECT_ATTEMPTS")
                .ok()
                .and_then(|v| v.parse().ok()),
            metrics_log_path: env::var("METRICS_LOG_PATH").ok(),
        }
    }

    pub fn validate_and_log(&self) {
        // Simple log of the loaded config. Add more validation logic if needed.
        log::info!("Application Configuration Loaded: {:?}", self);
        if self.rpc_url.is_empty() {
            log::error!("RPC_URL cannot be empty.");
            // Consider panicking or returning a Result from from_env if critical configs are invalid
        }
        // Add other critical validations here
    }
}