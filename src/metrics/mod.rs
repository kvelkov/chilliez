use log::{debug, info, warn, error}; // Ensure log levels are imported, added debug
use std::collections::HashSet;
use std::fs::OpenOptions; // For file logging, if kept
use std::io::Write;      // For file logging, if kept
use std::sync::Arc;      // If any part becomes async and shared
use tokio::sync::Mutex;  // If any part becomes async and shared
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::{Utc, TimeZone}; // For better timestamp handling

// If you intend to log to a file as in the prometheus version:
// use serde_json::json;
// use crate::error::ArbError; // If ArbError is used for file logging errors
// use crate::arbitrage::opportunity::ArbOpportunity as MainArbOpportunity; // Assuming this is the canonical one

/// Simple tuple struct for representing a trading pair, public for cross-module use.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct TradingPair(pub String, pub String); // (TokenInSymbol, TokenOutSymbol)

#[derive(Debug, Clone)] // Added Debug
pub struct OpportunityDetail {
    input_token: String,
    output_token: String,
    profit_percentage: f64,
    input_amount: f64,
    expected_output: f64,
    // Add other relevant details like DEX path if it's a multi-hop
    dex_path: Option<Vec<String>>, // Example: vec!["Orca", "Raydium"]
    timestamp: chrono::DateTime<chrono::Utc>, // Changed from Instant to DateTime<Utc>
}

#[derive(Debug)] // Added Debug
pub struct ExecutionRecord {
    opportunity_id: String, // Some way to link to the detected opportunity
    success: bool,
    execution_time_ms: u64,
    actual_profit_usd: Option<f64>, // Assuming profit is converted to a common currency
    transaction_signature: Option<String>,
    error_message: Option<String>,
    timestamp: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)] // Added derives for TradeOutcome
pub enum TradeOutcome {
    Success,
    Failure,
    Skipped,
}


pub struct Metrics {
    pub launch_time: u64,
    pub pools_fetched: usize,
    pub trading_pairs_monitored: usize, // Renamed for clarity

    pub opportunities_detected_count: u64,
    // pub recent_opportunities: VecDeque<OpportunityDetail>, // To store last N opportunities

    pub trades_attempted_count: u64,
    // pub trades_succeeded_count: u64, // Renamed to successful_trades_count
    // pub trades_failed_count: u64,    // Renamed to failed_trades_count
    pub successful_trades_count: u64, // Corrected name
    pub failed_trades_count: u64,     // Corrected name
    pub skipped_trades_count: u64, // Added for consistency

    pub total_profit_usd: f64, // Standardize to a common currency like USD
    pub total_fees_paid_usd: f64, // Fees in USD
    pub total_tx_cost_sol: f64, // Transaction costs in SOL (or lamports)

    // For Prometheus-like gauges, if not using the actual Prometheus crate yet:
    pub active_pools_gauge: usize,
    pub current_balance_sol: f64, // Example gauge

    // Added fields from main.rs usage
    sol_price_usd: f64,
    log_path: Option<String>,
    launch_time_utc: Option<chrono::DateTime<chrono::Utc>>,
    main_cycles_executed: u64,
    last_opportunity_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    last_successful_trade_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    current_min_profit_threshold: f64,
    system_healthy: bool,
    log_file: Option<Arc<tokio::sync::Mutex<tokio::fs::File>>>, // For async file logging
}

impl Metrics {
    pub fn new(sol_price_usd: f64, log_path: Option<String>) -> Self { // Updated signature
        let file_logger = if let Some(path) = &log_path { // Use &log_path
            // This part needs to be async or run in a blocking task if file opening is slow
            // For simplicity, keeping it sync for now, but be mindful of blocking in async context
            match OpenOptions::new().create(true).append(true).open(path) {
                Ok(file) => {
                    // Convert std::fs::File to tokio::fs::File if needed for async writes
                    // This example assumes you might wrap it or use a sync file writer initially
                    // For true async, you'd use tokio::fs::File::create/open
                    // Some(Arc::new(tokio::sync::Mutex::new(tokio::fs::File::from_std(file).unwrap())))
                    // Using std::fs::File directly with a Mutex for now, assuming writes are quick or handled carefully.
                    // If this becomes a bottleneck, switch to full async file I/O.
                    None // Placeholder: Actual async file handling is more complex
                }
                Err(e) => {
                    error!("Failed to open metrics log file at {}: {}", path, e);
                    None
                }
            }
        } else {
            None
        };

        Self {
            launch_time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            pools_fetched: 0,
            trading_pairs_monitored: 0,
            opportunities_detected_count: 0,
            trades_attempted_count: 0,
            // trades_succeeded_count: 0,
            // trades_failed_count: 0,
            successful_trades_count: 0, // Initialize
            failed_trades_count: 0,     // Initialize
            skipped_trades_count: 0,    // Initialize
            total_profit_usd: 0.0,
            total_fees_paid_usd: 0.0,
            total_tx_cost_sol: 0.0,
            active_pools_gauge: 0,
            current_balance_sol: 0.0,
            log_file: file_logger,
            // Initialize new fields
            sol_price_usd,
            log_path,
            launch_time_utc: None, 
            main_cycles_executed: 0,
            last_opportunity_timestamp: None,
            last_successful_trade_timestamp: None,
            current_min_profit_threshold: 0.0, // Default, will be updated
            system_healthy: true, // Assume healthy on start
        }
    }

    // Synchronous version of log_launch
    pub fn log_launch(&mut self) {
        self.launch_time_utc = Some(Utc::now()); // Corrected: use launch_time_utc
        info!("Application launched at: {:?}", self.launch_time_utc.unwrap());
        // If file logging is enabled, write to it here
    }

    // Synchronous version of log_pools_fetched
    pub fn log_pools_fetched(&mut self, count: usize) {
        self.pools_fetched = count;
        info!("{} pools fetched/initialized.", count);
    }

    pub fn log_trading_pairs(&mut self, pairs_count: usize) {
        self.trading_pairs_monitored = pairs_count;
        info!("📊 Monitoring {} unique trading pairs", pairs_count);
    }

    /// Called when an arbitrage opportunity is identified by the detector.
    pub fn record_opportunity_detected(
        &mut self,
        input_token_symbol: &str,
        output_token_symbol: &str, // Changed from intermediate_token_symbol for clarity with 3-hop
        profit_percentage: f64,
        estimated_profit_usd: Option<f64>,
        input_amount_usd: Option<f64>,
        dex_path: Vec<String>, // Changed from Vec<&str> to Vec<String>
    ) -> Result<(), String> { // Return a Result
        self.opportunities_detected_count += 1;
        let timestamp_utc = Utc::now(); // Use Utc::now() for DateTime<Utc>
        let detail = OpportunityDetail {
            input_token: input_token_symbol.to_string(),
            output_token: output_token_symbol.to_string(), // Use output_token_symbol
            profit_percentage,
            input_amount: input_amount_usd.unwrap_or(0.0), // Corrected: was estimated_profit_usd
            expected_output: estimated_profit_usd.unwrap_or(0.0), // Corrected: was input_amount_usd
            dex_path: Some(dex_path), // Wrap in Some()
            timestamp: timestamp_utc, // Use DateTime<Utc>
        };

        // Log to console
        info!(
            "💡 Arb Opportunity Detected (#{}): {:.2}% | {} -> {} | Input: {:.4} | Expected Output: {:.4} | Path: {:?}",
            self.opportunities_detected_count,
            detail.profit_percentage,
            detail.input_token,
            detail.output_token,
            detail.input_amount,
            detail.expected_output,
            detail.dex_path.as_ref().map_or_else(|| "N/A".to_string(), |p| p.join(" -> ")) // Corrected logging for Option<Vec<String>>
        );

        // TODO: Add to recent_opportunities deque if using
        // TODO: Log to file if self.log_file is Some(...)

        // Ensure the CSV format matches the headers, especially if intermediate_token was removed/renamed
        let log_entry_str = format!(
            "{}Z,{},{},{:.6},{:.2},{:.2},{}\
", // Adjusted for potentially removed intermediate token
            detail.timestamp.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            detail.input_token, // Corrected: use detail.input_token
            detail.output_token, // Corrected: use detail.output_token
            detail.profit_percentage,
            detail.expected_output, // Corrected: use detail.expected_output (was estimated_profit_usd)
            detail.input_amount, // Corrected: use detail.input_amount (was input_amount_usd)
            detail.dex_path.as_ref().map_or_else(|| "".to_string(), |p| p.join("->")) // Corrected logging for Option<Vec<String>>
        );

        // If file logging is enabled, write the log_entry_str to the file

        Ok(())
    }


    /// Called before attempting to execute a trade.
    pub fn record_trade_attempt(
        &mut self,
        opportunity_id: &str,
        dex_path: &[String], // Changed to slice of String
        input_token: &str,
        output_token: &str,
        input_amount: f64,
        expected_output_amount: f64,
        estimated_profit_pct: f64,
        estimated_profit_usd: Option<f64>,
        outcome: TradeOutcome, // Use defined TradeOutcome
        actual_profit_usd: Option<f64>, // Added field
        execution_time_ms: u128, // Added field
        tx_cost_sol: f64, // Added field
        fees_paid_usd: Option<f64>, // Added field
        error_message: Option<String>,
        transaction_id: Option<String>,
    ) {
        self.trades_attempted_count += 1;
        let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let dex_path_str = dex_path.join(" -> ");

        let log_entry_str = match outcome {
            TradeOutcome::Success => {
                self.successful_trades_count += 1;
                if let Some(profit) = actual_profit_usd {
                    self.total_profit_usd += profit;
                }
                format!(
                    "{}Z,SUCCESS,{},{},{},{},{:.6},{:.6},{:.6},{:.2},{:.2},{},{:.8},{:.4},{}",
                    timestamp, opportunity_id, dex_path_str, input_token, output_token,
                    input_amount, expected_output_amount, actual_profit_usd.unwrap_or(0.0),
                    execution_time_ms, estimated_profit_pct, transaction_id.as_deref().unwrap_or("N/A"),
                    tx_cost_sol, fees_paid_usd.unwrap_or(0.0), error_message.as_deref().unwrap_or("")
                )
            }
            TradeOutcome::Failure => {
                self.failed_trades_count += 1;
                format!(
                    "{}Z,FAILURE,{},{},{},{},{:.6},{:.6},{:.2},{},{:.8},{:.4},{}",
                    timestamp, opportunity_id, dex_path_str, input_token, output_token,
                    input_amount, expected_output_amount, execution_time_ms, 
                    transaction_id.as_deref().unwrap_or("N/A"),
                    tx_cost_sol, fees_paid_usd.unwrap_or(0.0), error_message.as_deref().unwrap_or("ERROR")
                )
            }
            TradeOutcome::Skipped => { // Added match arm for Skipped
                self.skipped_trades_count += 1; // Assuming a counter for skipped trades
                format!(
                    "{}Z,SKIPPED,{},{},{},{},{:.6},{:.6},{:.2},{},{},{}",
                    timestamp, opportunity_id, dex_path_str, input_token, output_token,
                    input_amount, expected_output_amount, estimated_profit_pct,
                    error_message.as_deref().unwrap_or("SKIPPED"), transaction_id.as_deref().unwrap_or("N/A"),
                    fees_paid_usd.unwrap_or(0.0) // Example, adjust as needed
                )
            }
        };
        self.total_tx_cost_sol += tx_cost_sol;
        if let Some(fees) = fees_paid_usd {
            self.total_fees_paid_usd += fees;
        }

        info!("Trade Attempt: {}", log_entry_str);
        // Further logging to file or other systems can be done here if needed
        // Example: self.log_to_file(&log_entry_str);
    }

    pub fn record_opportunity_detected(
        &mut self,
        input_token: &str,
        intermediate_token: &str, // Assuming this is the last intermediate before cycling back
        profit_pct: f64,
        estimated_profit_usd: Option<f64>,
        input_amount_usd: Option<f64>,
        dex_path: Vec<String>, // Changed to Vec<String>
    ) -> Result<(), String> { // Return Result
        self.opportunities_detected_count += 1;
        let timestamp_utc = chrono::Utc::now(); // Use Utc::now() for DateTime<Utc>
        let dex_path_str = dex_path.join(" -> ");

        let log_entry = format!(
            "{}Z,OPP_DETECTED,{},{},{:.4}%,{:.2},{:.2},{}",
            timestamp_utc.to_rfc3339_opts(chrono::SecondsFormat::Millis, true), // Use timestamp_utc
            input_token,
            intermediate_token,
            profit_pct,
            estimated_profit_usd.unwrap_or(0.0),
            input_amount_usd.unwrap_or(0.0),
            dex_path_str
        );
        info!("{}", log_entry);
        // Example: self.log_to_file(&log_entry_str)?;
        Ok(())
    }


    pub fn get_log_file(&self) -> Option<&Arc<tokio::sync::Mutex<tokio::fs::File>>> { // Getter for log_file
        self.log_file.as_ref()
    }

    // Synchronous version of summary
    pub fn summary(&self) {
        info!("-------------------- Metrics Summary --------------------");
        if let Some(lt) = self.launch_time_utc { // Corrected: use launch_time_utc
            let uptime = Utc::now().signed_duration_since(lt);
            info!("Uptime: {}h {}m {}s", uptime.num_hours(), uptime.num_minutes() % 60, uptime.num_seconds() % 60);
        }
        info!("Pools Fetched: {}", self.pools_fetched);
        info!("Trading Pairs Monitored: {}", self.trading_pairs_monitored);
        info!("Main Cycles Executed: {}", self.main_cycles_executed);
        info!("Opportunities Detected: {}", self.opportunities_detected_count);
        info!("Trades Attempted: {}", self.trades_attempted_count);
        info!("Successful Trades: {}", self.successful_trades_count);
        info!("Failed Trades: {}", self.failed_trades_count);
        info!("Total Profit (USD): {:.2}", self.total_profit_usd);
        info!("Average Profit per Successful Trade (USD): {:.2}", if self.successful_trades_count > 0 { self.total_profit_usd / self.successful_trades_count as f64 } else { 0.0 });
        if let Some(last_update) = self.last_opportunity_timestamp {
            info!("Last Opportunity Detected At: {}Z", last_update.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        }
        if let Some(last_trade) = self.last_successful_trade_timestamp {
            info!("Last Successful Trade At: {}Z", last_trade.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        }
        info!("Current Dynamic Min Profit Threshold: {:.4}%", self.current_min_profit_threshold * 100.0);
        info!("System Health Status: {}", if self.system_healthy { "Healthy" } else { "Unhealthy" });
        info!("-------------------------------------------------------");

        // If logging to a file, this might be where a summary is written too.
    }

    // Synchronous version of increment_main_cycles
    pub fn increment_main_cycles(&mut self) {
        self.main_cycles_executed += 1;
    }

    // Synchronous version of record_main_cycle_duration
    pub fn record_main_cycle_duration(&mut self, duration_ms: u64) {
        // Could store this for averaging or just log it
        debug!("Main cycle duration: {}ms", duration_ms);
    }

    // Synchronous version of log_dynamic_threshold_update
    pub fn log_dynamic_threshold_update(&mut self, new_threshold: f64) {
        self.current_min_profit_threshold = new_threshold;
        info!("Dynamic minimum profit threshold updated to: {:.4}%", new_threshold * 100.0);
    }

    // Synchronous version of set_system_health
    pub fn set_system_health(&mut self, healthy: bool) {
        self.system_healthy = healthy;
        info!("System health status set to: {}", if healthy { "Healthy" } else { "Unhealthy" });
    }
}

// Default implementation for convenience
impl Default for Metrics {
    fn default() -> Self {
        Metrics::new(100.0, None) // Provide default values for new parameters
    }
}
