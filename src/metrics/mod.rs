// src/metrics/mod.rs
use log::{debug, info}; // Removed error, warn as unused by compiler in this specific file context
// use std::fs::OpenOptions; // No longer directly used here after log_file changes
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH, Instant};
use chrono::Utc; // Removed TimeZone as Utc is sufficient

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct TradingPair(pub String, pub String);

#[derive(Debug, Clone)]
pub struct OpportunityDetail {
    input_token: String,
    output_token: String,
    profit_percentage: f64,
    input_amount: f64,
    expected_output: f64,
    dex_path: Option<Vec<String>>,
    timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug)]
pub struct ExecutionRecord {
    #[allow(dead_code)] opportunity_id: String,
    #[allow(dead_code)] success: bool,
    #[allow(dead_code)] execution_time_ms: u64,
    #[allow(dead_code)] actual_profit_usd: Option<f64>,
    #[allow(dead_code)] transaction_signature: Option<String>,
    #[allow(dead_code)] error_message: Option<String>,
    #[allow(dead_code)] timestamp: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TradeOutcome {
    Attempted,
    Success,
    Failure,
    Skipped,
}

#[derive(Debug)]
pub struct Metrics {
    pub launch_time: u64,
    pub pools_fetched: usize,
    pub trading_pairs_monitored: usize,
    pub opportunities_detected_count: u64,
    pub trades_attempted_count: u64,
    pub successful_trades_count: u64,
    pub failed_trades_count: u64,
    pub skipped_trades_count: u64,
    pub total_profit_usd: f64,
    pub total_fees_paid_usd: f64,
    pub total_tx_cost_sol: f64,
    pub active_pools_gauge: usize,
    pub current_balance_sol: f64,
    #[allow(dead_code)] sol_price_usd: f64, // Mark unused if only for initialization
    #[allow(dead_code)] log_path: Option<String>,
    launch_time_utc: Option<chrono::DateTime<chrono::Utc>>,
    main_cycles_executed: u64,
    last_opportunity_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    last_successful_trade_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    current_min_profit_threshold: f64,
    system_healthy: bool,
    log_file: Option<Arc<tokio::sync::Mutex<tokio::fs::File>>>,
}

impl Metrics {
    pub fn new(sol_price_usd: f64, log_path: Option<String>) -> Self {
        info!("Metrics initialized. Log path: {:?}", log_path);
        Self {
            launch_time: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
            pools_fetched: 0, trading_pairs_monitored: 0,
            opportunities_detected_count: 0, trades_attempted_count: 0,
            successful_trades_count: 0, failed_trades_count: 0, skipped_trades_count: 0,
            total_profit_usd: 0.0, total_fees_paid_usd: 0.0, total_tx_cost_sol: 0.0,
            active_pools_gauge: 0, current_balance_sol: 0.0, sol_price_usd, log_path,
            launch_time_utc: None, main_cycles_executed: 0,
            last_opportunity_timestamp: None, last_successful_trade_timestamp: None,
            current_min_profit_threshold: 0.0, system_healthy: true, log_file: None,
        }
    }

    pub fn log_launch(&mut self) { /* ... same ... */ }
    pub fn log_pools_fetched(&mut self, count: usize) { /* ... same ... */ }
    pub fn log_trading_pairs(&mut self, pairs_count: usize) { /* ... same ... */ }
    
    pub fn record_opportunity_detected(
        &mut self,
        input_token_symbol: &str,
        intermediate_token_symbol: &str,
        profit_percentage: f64,
        estimated_profit_usd: Option<f64>, // Now used in logging
        input_amount_usd: Option<f64>,
        dex_path: Vec<String>,
    ) -> Result<(), String> {
        self.opportunities_detected_count += 1;
        self.last_opportunity_timestamp = Some(Utc::now());

        // Using intermediate_token_symbol as output_token for this simplified detail logging
        let detail = OpportunityDetail {
            input_token: input_token_symbol.to_string(),
            output_token: intermediate_token_symbol.to_string(),
            profit_percentage,
            input_amount: input_amount_usd.unwrap_or(0.0),
            expected_output: input_amount_usd.unwrap_or(0.0) * (1.0 + profit_percentage / 100.0),
            dex_path: Some(dex_path.clone()),
            timestamp: Utc::now(),
        };

        info!( // Using estimated_profit_usd here
            "💡 Arb Opportunity Detected (#{}): {:.2}% | {} -> {} (via path) | Input USD: {:.2} | Est. Profit USD: {:.2} | Path: {}",
            self.opportunities_detected_count,
            detail.profit_percentage,
            detail.input_token,
            detail.output_token, 
            detail.input_amount, 
            estimated_profit_usd.unwrap_or(0.0), // Using the parameter
            dex_path.join(" -> ")
        );
        Ok(())
    }

    pub fn record_trade_attempt( /* ... same as previous correct version, ensures Attempted is handled by early return ... */ ) { /* ... */ }
    pub fn get_log_file(&self) -> Option<&Arc<tokio::sync::Mutex<tokio::fs::File>>> { self.log_file.as_ref() }
    pub fn summary(&self) { /* ... same ... */ }
    pub fn increment_main_cycles(&mut self) { /* ... same ... */ }
    pub fn record_main_cycle_duration(&mut self, duration_ms: u64) { /* ... same ... */ }
    pub fn log_dynamic_threshold_update(&mut self, new_threshold_fractional: f64) { /* ... same ... */ }
    pub fn set_system_health(&mut self, healthy: bool) { /* ... same ... */ }
    pub fn log_pools_updated(&mut self, new_count: usize, updated_count: usize, total_count: usize) { /* ... same ... */ }
    pub fn log_degradation_mode_change(&mut self, entered_degradation: bool, new_threshold_fractional: Option<f64>) { /* ... same ... */ }
}

impl Default for Metrics { fn default() -> Self { Metrics::new(100.0, None) } }