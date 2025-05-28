// src/metrics/mod.rs
use log::{info}; // Removed error, warn as unused by compiler in this specific file context
// use std::fs::OpenOptions; // No longer directly used here after log_file changes
use std::sync::{
    atomic::{AtomicU64, Ordering}, // Added AtomicU64 and Ordering
    Arc,
};
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
    opportunity_id: String,
    success: bool,
    execution_time_ms: u64,
    actual_profit_usd: Option<f64>,
    transaction_signature: Option<String>,
    error_message: Option<String>,
    timestamp: Instant,
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
    sol_price_usd: f64, // Mark unused if only for initialization
    log_path: Option<String>,
    launch_time_utc: Option<chrono::DateTime<chrono::Utc>>,
    main_cycles_executed: u64,
    last_opportunity_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    last_successful_trade_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    current_min_profit_threshold: f64,
    system_healthy: bool,
    // Added missing fields for execution metrics
    opportunities_executed_success: AtomicU64,
    opportunities_executed_failure: AtomicU64,
    execution_count: AtomicU64,
    total_execution_ms: AtomicU64,
    dynamic_threshold_updates: AtomicU64, // Assuming this was also intended if not already present
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
            last_opportunity_timestamp: None, 
            last_successful_trade_timestamp: None,
            opportunities_executed_success: AtomicU64::new(0), // Initialize new fields
            opportunities_executed_failure: AtomicU64::new(0),
            execution_count: AtomicU64::new(0),
            total_execution_ms: AtomicU64::new(0),
            dynamic_threshold_updates: AtomicU64::new(0),
            current_min_profit_threshold: 0.0, system_healthy: true, log_file: None,
        }
    }

    pub fn log_launch(&mut self) { /* ... same ... */ }
    pub fn log_pools_fetched(&mut self, _count: usize) { /* ... same ... */ }
    pub fn log_trading_pairs(&mut self, _pairs_count: usize) { /* ... same ... */ }
    
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

    /// Call this method immediately after a successful execution.
    pub fn log_opportunity_executed_success(&self) {
        self.opportunities_executed_success.fetch_add(1, Ordering::Relaxed);
    }

    /// Call this method after a failed execution attempt.
    pub fn log_opportunity_executed_failure(&self) {
        self.opportunities_executed_failure.fetch_add(1, Ordering::Relaxed);
    }

    /// Record the execution time (duration) for an operation.
    pub fn record_execution_time(&self, duration: std::time::Duration) { // Changed from u64 to Duration
        self.execution_count.fetch_add(1, Ordering::Relaxed);
        self.total_execution_ms
            .fetch_add(duration.as_millis() as u64, Ordering::Relaxed);
    }

    pub fn record_trade_attempt( /* ... same as previous correct version, ensures Attempted is handled by early return ... */ ) { /* ... */ }
    pub fn get_log_file(&self) -> Option<&Arc<tokio::sync::Mutex<tokio::fs::File>>> { self.log_file.as_ref() }
    pub fn summary(&self) { /* ... same ... */ }
    pub fn increment_main_cycles(&mut self) { /* ... same ... */ }
    pub fn record_main_cycle_duration(&mut self, _duration_ms: u64) { /* ... same ... */ }
    pub fn log_dynamic_threshold_update(&mut self, new_threshold_fractional: f64) { 
        self.dynamic_threshold_updates.fetch_add(1, Ordering::Relaxed);
        log::info!("Dynamic Threshold updated to: {:.4}%", new_threshold_fractional * 100.0);
    }
    pub fn set_system_health(&mut self, _healthy: bool) { /* ... same ... */ }
    pub fn log_pools_updated(&mut self, _new_count: usize, _updated_count: usize, _total_count: usize) { /* ... same ... */ }
    pub fn log_degradation_mode_change(&mut self, _entered_degradation: bool, _new_threshold_fractional: Option<f64>) { /* ... same ... */ }
}

impl Default for Metrics { fn default() -> Self { Metrics::new(100.0, None) } }