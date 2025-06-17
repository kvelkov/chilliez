use serde::Deserialize;
use std::{collections::HashMap, env};
use std::path::Path;
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
    pub transaction_cu_limit: Option<u32>, // Added field for Compute Unit Limit
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
}

impl Config {
    /// Loads configuration from the specified file path using the `config` crate.
    /// Expects a TOML file format.
    pub fn load(config_path: &Path) -> Result<Self, ::config::ConfigError> {
        let settings = ::config::Config::builder()
            // Add configuration source from the specified file path
            .add_source(::config::File::from(config_path))
            // Example: Optionally, add environment variable overrides
            // .add_source(::config::Environment::with_prefix("APP").separator("__"))
            .build()?;

        settings.try_deserialize()
    }

    pub fn from_env() -> Self {
        dotenv::dotenv().ok();
        Self::from_env_without_loading()
    }

    /// Create Config from environment variables without loading .env file
    /// This allows external control of which environment file to load
    pub fn from_env_without_loading() -> Self {
        Config {
            rpc_url: env::var("RPC_URL").expect("RPC_URL must be set"),
            rpc_url_secondary: env::var("RPC_URL_SECONDARY").ok(),
            ws_url: env::var("WS_URL").expect("WS_URL must be set"),
            wallet_path: env::var("WALLET_PATH").unwrap_or_else(|_| "".to_string()), // Make optional for paper trading
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
            transaction_cu_limit: env::var("TRANSACTION_CU_LIMIT").ok().and_then(|s| s.parse().ok()), // Load from env
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
            
            // Jupiter fallback configuration
            jupiter_fallback_enabled: env::var("JUPITER_FALLBACK_ENABLED").unwrap_or_else(|_| "false".to_string()).parse().unwrap_or(false),
            jupiter_api_timeout_ms: env::var("JUPITER_API_TIMEOUT_MS").unwrap_or_else(|_| "5000".to_string()).parse().unwrap_or(5000),
            jupiter_max_retries: env::var("JUPITER_MAX_RETRIES").unwrap_or_else(|_| "3".to_string()).parse().unwrap_or(3),
            jupiter_fallback_min_profit_pct: env::var("JUPITER_FALLBACK_MIN_PROFIT_PCT").unwrap_or_else(|_| "0.001".to_string()).parse().unwrap_or(0.001),
            jupiter_slippage_tolerance_bps: env::var("JUPITER_SLIPPAGE_TOLERANCE_BPS").unwrap_or_else(|_| "50".to_string()).parse().unwrap_or(50),
            
            // Jupiter cache configuration
            jupiter_cache_enabled: env::var("JUPITER_CACHE_ENABLED").unwrap_or_else(|_| "true".to_string()).parse().unwrap_or(true),
            jupiter_cache_ttl_seconds: env::var("JUPITER_CACHE_TTL_SECONDS").unwrap_or_else(|_| "5".to_string()).parse().unwrap_or(5),
            jupiter_cache_max_entries: env::var("JUPITER_CACHE_MAX_ENTRIES").unwrap_or_else(|_| "1000".to_string()).parse().unwrap_or(1000),
            jupiter_cache_amount_bucket_size: env::var("JUPITER_CACHE_AMOUNT_BUCKET_SIZE").unwrap_or_else(|_| "1000000".to_string()).parse().unwrap_or(1_000_000),
            jupiter_cache_volatility_threshold_pct: env::var("JUPITER_CACHE_VOLATILITY_THRESHOLD_PCT").unwrap_or_else(|_| "2.0".to_string()).parse().unwrap_or(2.0),
            
            // Jupiter route optimization configuration
            jupiter_route_optimization_enabled: env::var("JUPITER_ROUTE_OPTIMIZATION_ENABLED").unwrap_or_else(|_| "true".to_string()).parse().unwrap_or(true),
            jupiter_max_parallel_routes: env::var("JUPITER_MAX_PARALLEL_ROUTES").unwrap_or_else(|_| "5".to_string()).parse().unwrap_or(5),
            jupiter_max_alternative_routes: env::var("JUPITER_MAX_ALTERNATIVE_ROUTES").unwrap_or_else(|_| "10".to_string()).parse().unwrap_or(10),
            jupiter_route_evaluation_timeout_ms: env::var("JUPITER_ROUTE_EVALUATION_TIMEOUT_MS").unwrap_or_else(|_| "2000".to_string()).parse().unwrap_or(2000),
            jupiter_min_route_improvement_pct: env::var("JUPITER_MIN_ROUTE_IMPROVEMENT_PCT").unwrap_or_else(|_| "0.1".to_string()).parse().unwrap_or(0.1),
            
            // Webhook configuration
            webhook_port: env::var("WEBHOOK_PORT").ok().and_then(|s| s.parse().ok()),
            webhook_url: env::var("WEBHOOK_URL").ok(),
            enable_webhooks: env::var("ENABLE_WEBHOOKS").unwrap_or_else(|_| "false".to_string()).parse().unwrap_or(false),
        }
    }

    /// Returns a default config for testing purposes.
    pub fn test_default() -> Self {
        Self {
            min_profit_pct: 0.1,
            dynamic_threshold_update_interval_secs: Some(60),
            rpc_url: "http://localhost:8899".to_string(),
            rpc_url_secondary: None,
            ws_url: "ws://localhost:8900".to_string(),
            wallet_path: "test_wallet.json".to_string(),
            trader_wallet_keypair_path: Some("test_keypair.json".to_string()),
            default_priority_fee_lamports: 5000,
            health_check_interval_secs: Some(60),
            health_check_token_symbol: Some("SOL/USDC".to_string()),
            degradation_profit_factor: Some(1.5),
            max_ws_reconnect_attempts: Some(5),
            enable_fixed_input_arb_detection: false,
            fixed_input_arb_amount: None,
            sol_price_usd: Some(150.0),
            min_profit_usd_threshold: Some(0.05),
            max_slippage_pct: 0.005,
            max_tx_fee_lamports_for_acceptance: Some(100000),
            transaction_priority_fee_lamports: 10000,
            pool_refresh_interval_secs: 10,
            redis_url: "redis://127.0.0.1/".to_string(),
            redis_default_ttl_secs: 3600,
            dex_quote_cache_ttl_secs: Some(std::collections::HashMap::new()),
            volatility_tracker_window: Some(20),
            volatility_threshold_factor: Some(0.5),
            max_risk_score_for_acceptance: Some(0.75),
            max_hops: Some(3),
            max_pools_per_hop: Some(5),
            max_concurrent_executions: Some(10),
            execution_timeout_secs: Some(30),
            transaction_cu_limit: Some(400_000), // Add default for test config
            simulation_mode: false,
            paper_trading: false,
            metrics_log_path: None,
            rpc_url_backup: None,
            rpc_max_retries: Some(3),
            rpc_retry_delay_ms: Some(1000),
            max_transaction_timeout_seconds: Some(120),
            ws_update_channel_size: Some(1024),
            congestion_update_interval_secs: Some(15),
            cycle_interval_seconds: Some(5),
            pool_read_timeout_ms: Some(1000),
            log_level: Some("info".to_string()),
            
            // Webhook Configuration (test defaults)
            webhook_port: Some(8080),
            webhook_url: Some("http://localhost:8080/webhook".to_string()),
            enable_webhooks: false,

            // Jupiter fallback configuration (test defaults)
            jupiter_fallback_enabled: false,
            jupiter_api_timeout_ms: 5000,
            jupiter_max_retries: 3,
            jupiter_fallback_min_profit_pct: 0.001,
            jupiter_slippage_tolerance_bps: 50,
            
            // Jupiter cache configuration (test defaults)
            jupiter_cache_enabled: true,
            jupiter_cache_ttl_seconds: 5,
            jupiter_cache_max_entries: 1000,
            jupiter_cache_amount_bucket_size: 1_000_000,
            jupiter_cache_volatility_threshold_pct: 2.0,
            
            // Jupiter route optimization configuration (test defaults)
            jupiter_route_optimization_enabled: true,
            jupiter_max_parallel_routes: 5,
            jupiter_max_alternative_routes: 10,
            jupiter_route_evaluation_timeout_ms: 2000,
            jupiter_min_route_improvement_pct: 0.1,
        }
    }

    pub fn validate_and_log(&self) {
        log::info!("╔════════════════════════════════════════════════════════════════════════════╗");
        log::info!("║                      🤖 SOLANA ARBITRAGE BOT CONFIGURATION                   ║");
        log::info!("╚════════════════════════════════════════════════════════════════════════════╝");
        
        // Trading Mode
        if self.paper_trading {
            log::info!("🎯 TRADING MODE: 📄 Paper Trading (Virtual Funds)");
        } else {
            log::info!("🎯 TRADING MODE: 💰 Real Trading (Live Funds)");
        }
        
        // Connection Configuration
        log::info!("🌐 NETWORK CONFIGURATION:");
        log::info!("   • Primary RPC: {}", self.rpc_url);
        if let Some(secondary) = &self.rpc_url_secondary {
            log::info!("   • Secondary RPC: {}", secondary);
        } else {
            log::info!("   • Secondary RPC: Not configured");
        }
        log::info!("   • WebSocket: {}", self.ws_url);
        
        // Wallet Configuration
        log::info!("💼 WALLET CONFIGURATION:");
        if let Some(wallet_path) = &self.trader_wallet_keypair_path {
            if !wallet_path.is_empty() {
                log::info!("   • Trader Wallet: ✅ Configured ({})", wallet_path);
            } else {
                if self.paper_trading {
                    log::info!("   • Trader Wallet: ➖ Not required (Paper Trading)");
                } else {
                    log::error!("   • Trader Wallet: ❌ MISSING - Required for real trading!");
                }
            }
        } else {
            if self.paper_trading {
                log::info!("   • Trader Wallet: ➖ Not required (Paper Trading)");
            } else {
                log::error!("   • Trader Wallet: ❌ MISSING - Required for real trading!");
            }
        }
        
        // Trading Parameters
        log::info!("📊 TRADING PARAMETERS:");
        log::info!("   • Min Profit Threshold: {:.4}% ({:.1} bps)", 
                  self.min_profit_pct * 100.0, self.min_profit_pct * 10000.0);
        if let Some(usd_threshold) = self.min_profit_usd_threshold {
            log::info!("   • Min Profit (USD): ${:.4}", usd_threshold);
        }
        log::info!("   • Max Slippage: {:.2}% ({:.0} bps)", 
                  self.max_slippage_pct * 100.0, self.max_slippage_pct * 10000.0);
        log::info!("   • Priority Fee: {} lamports ({:.6} SOL)", 
                  self.transaction_priority_fee_lamports, 
                  self.transaction_priority_fee_lamports as f64 / 1e9);
        
        // Performance Settings
        log::info!("⚡ PERFORMANCE SETTINGS:");
        log::info!("   • Pool Refresh: {} seconds", self.pool_refresh_interval_secs);
        if let Some(concurrent) = self.max_concurrent_executions {
            log::info!("   • Max Concurrent: {} operations", concurrent);
        }
        if let Some(timeout) = self.execution_timeout_secs {
            log::info!("   • Execution Timeout: {} seconds", timeout);
        }
        if let Some(cu_limit) = self.transaction_cu_limit {
            log::info!("   • Compute Units: {} CU limit", cu_limit);
        }
        
        // Webhook Configuration
        if self.enable_webhooks {
            log::info!("🔗 WEBHOOK INTEGRATION:");
            if let Some(port) = self.webhook_port {
                log::info!("   • Status: ✅ Enabled (Port: {})", port);
            } else {
                log::info!("   • Status: ✅ Enabled");
            }
            if let Some(url) = &self.webhook_url {
                log::info!("   • Endpoint: {}", url);
            }
        } else {
            log::info!("🔗 WEBHOOK INTEGRATION: ➖ Disabled");
        }
        
        // Jupiter Integration
        if self.jupiter_fallback_enabled {
            log::info!("🪐 JUPITER INTEGRATION:");
            log::info!("   • Status: ✅ Enabled");
            log::info!("   • API Timeout: {}ms", self.jupiter_api_timeout_ms);
            log::info!("   • Max Retries: {}", self.jupiter_max_retries);
            log::info!("   • Min Profit: {:.4}%", self.jupiter_fallback_min_profit_pct * 100.0);
            if self.jupiter_cache_enabled {
                log::info!("   • Cache: ✅ Enabled (TTL: {}s, Max: {} entries)", 
                          self.jupiter_cache_ttl_seconds, self.jupiter_cache_max_entries);
            }
        } else {
            log::info!("🪐 JUPITER INTEGRATION: ➖ Disabled");
        }
        
        // Validation Warnings
        if self.min_profit_pct <= 0.0 || self.min_profit_pct >= 1.0 {
            log::warn!("⚠️  MIN_PROFIT_PCT ({:.4}%) is outside typical range (0.001% - 99.999%)", 
                      self.min_profit_pct * 100.0);
        }
        
        if self.transaction_priority_fee_lamports == 0 {
            log::warn!("⚠️  Priority fee is 0 - transactions may be slow");
        }
        
        log::info!("════════════════════════════════════════════════════════════════════════════");
    }
}

// Example of how you might load it in your main.rs or setup
// fn main() -> Result<(), Box<dyn std::error::Error>> {
//     // Ensure you have the `config` crate in your Cargo.toml
//     let config = Config::load(Path::new("config.toml"))?;
//     // Now use config...
//     Ok(())
// }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_is_creatable_and_has_expected_values() {
        let config = Config::test_default();
        // Add assertions here to check specific default values.
        // This not only uses the function but also verifies its behavior.
        assert_eq!(config.min_profit_pct, 0.1, "Default min_profit_pct should be 0.1");
        assert_eq!(config.rpc_url, "http://localhost:8899", "Default rpc_url should be http://localhost:8899");
        assert!(!config.simulation_mode, "Default simulation_mode should be false");
    }
}
