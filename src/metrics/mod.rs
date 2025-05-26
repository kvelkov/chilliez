// src/metrics/mod.rs
use log::{debug, info, error}; // Removed warn as it was unused
// Removed: use std::collections::HashSet; // Unused
use std::fs::OpenOptions;
// Removed: use std::io::Write; // Unused
use std::sync::Arc;
// Removed: use tokio::sync::Mutex as TokioMutex; // Using std Mutex for log_file, or switch to tokio::fs::File
use std::time::{SystemTime, UNIX_EPOCH, Instant}; // Added Instant
use chrono::{Utc, TimeZone}; // Removed TimeZone as it's not directly used, Utc handles it

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

#[derive(Debug)] // Added derive
pub struct ExecutionRecord { // This struct seems unused currently
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
    Attempted, // Added this variant for logging initial attempt
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
    sol_price_usd: f64,
    #[allow(dead_code)] log_path: Option<String>, // Mark unused if file logging is placeholder
    launch_time_utc: Option<chrono::DateTime<chrono::Utc>>,
    main_cycles_executed: u64,
    last_opportunity_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    last_successful_trade_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    current_min_profit_threshold: f64, // This is fractional (e.g. 0.001 for 0.1%)
    system_healthy: bool,
    // Using tokio::fs::File for async operations, requires async methods to write
    log_file: Option<Arc<tokio::sync::Mutex<tokio::fs::File>>>,
}

impl Metrics {
    pub fn new(sol_price_usd: f64, log_path: Option<String>) -> Self {
        // Async file opening should be done in an async context or a blocking task.
        // For simplicity in `new`, we'll prepare the path, and actual file opening
        // can happen in an async method if/when first write occurs, or `new` becomes async.
        // For now, log_file remains None if initialized sync.
        info!("Metrics initialized. Log path: {:?}", log_path); // Log path for now
        Self {
            launch_time: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
            pools_fetched: 0, trading_pairs_monitored: 0,
            opportunities_detected_count: 0, trades_attempted_count: 0,
            successful_trades_count: 0, failed_trades_count: 0, skipped_trades_count: 0,
            total_profit_usd: 0.0, total_fees_paid_usd: 0.0, total_tx_cost_sol: 0.0,
            active_pools_gauge: 0, current_balance_sol: 0.0,
            sol_price_usd, log_path, launch_time_utc: None, main_cycles_executed: 0,
            last_opportunity_timestamp: None, last_successful_trade_timestamp: None,
            current_min_profit_threshold: 0.0, system_healthy: true,
            log_file: None, // Initialize as None; async setup needed for tokio::fs::File
        }
    }
    
    // Example async method to setup log_file if needed later
    // pub async fn setup_log_file(&mut self) {
    //     if let Some(path) = &self.log_path {
    //         match tokio::fs::OpenOptions::new().create(true).append(true).open(path).await {
    //             Ok(file) => self.log_file = Some(Arc::new(tokio::sync::Mutex::new(file))),
    //             Err(e) => error!("Failed to open metrics log file async at {}: {}", path, e),
    //         }
    //     }
    // }


    pub fn log_launch(&mut self) {
        self.launch_time_utc = Some(Utc::now());
        info!("Application launched at: {:?}", self.launch_time_utc.unwrap());
    }

    pub fn log_pools_fetched(&mut self, count: usize) {
        self.pools_fetched = count;
        info!("{} pools fetched/initialized.", count);
    }

    pub fn log_trading_pairs(&mut self, pairs_count: usize) {
        self.trading_pairs_monitored = pairs_count;
        info!("📊 Monitoring {} unique trading pairs", pairs_count);
    }
    
    // First version of record_opportunity_detected from original file (simpler args)
    pub fn record_opportunity_detected(
        &mut self,
        input_token_symbol: &str,
        intermediate_token_symbol: &str, // For 3-hop, this is the *first* intermediate
        profit_percentage: f64, // This is actual percentage e.g. 1.5 for 1.5%
        estimated_profit_usd: Option<f64>,
        input_amount_usd: Option<f64>,
        dex_path: Vec<String>,
    ) -> Result<(), String> { // Return Result for error handling
        self.opportunities_detected_count += 1;
        self.last_opportunity_timestamp = Some(Utc::now());

        let detail = OpportunityDetail {
            input_token: input_token_symbol.to_string(),
            output_token: intermediate_token_symbol.to_string(), // In 3-hop this is misleading, should be final output
            profit_percentage, // Already in percent
            input_amount: input_amount_usd.unwrap_or(0.0),
            expected_output: input_amount_usd.unwrap_or(0.0) * (1.0 + profit_percentage / 100.0), // Rough estimate
            dex_path: Some(dex_path.clone()),
            timestamp: Utc::now(),
        };

        info!(
            "💡 Arb Opportunity Detected (#{}): {:.2}% | {} -> {} (via path) | Input USD: {:.2} | Est. Profit USD: {:.2} | Path: {}",
            self.opportunities_detected_count,
            detail.profit_percentage,
            detail.input_token,
            detail.output_token, // This is intermediate for 3-hop log. Clarify logging intent.
            detail.input_amount, // This is input_amount_usd
            detail.expected_output - detail.input_amount, // This is profit_usd if input is usd
            dex_path.join(" -> ")
        );
        
        // CSV Logging part can be added here if log_file is setup and method is async
        // For sync method, direct file I/O can block.
        // Example structure if it were async:
        // if let Some(log_file_mutex) = &self.log_file {
        //     let mut file_guard = log_file_mutex.lock().await;
        //     let log_entry_str = format!(...);
        //     if let Err(e) = file_guard.write_all(log_entry_str.as_bytes()).await {
        //         error!("Failed to write opportunity to metrics log file: {}", e);
        //         return Err(format!("Failed to write to log: {}", e));
        //     }
        // }
        Ok(())
    }

    // record_trade_attempt now handles different outcomes including initial attempt logging.
    pub fn record_trade_attempt(
        &mut self,
        opportunity_id: &str,
        dex_path: &[String],
        input_token: &str,
        output_token: &str,
        input_amount: f64, // Human-readable amount
        expected_output_amount: f64, // Human-readable amount
        estimated_profit_pct: f64, // Percentage, e.g., 1.5 for 1.5%
        estimated_profit_usd: Option<f64>,
        outcome: TradeOutcome,
        actual_profit_usd: Option<f64>,
        execution_time_ms: u128, // Changed from u64 due to .as_millis()
        tx_cost_sol: f64,
        fees_paid_usd: Option<f64>,
        error_message: Option<String>,
        transaction_id: Option<String>,
    ) {
        if outcome == TradeOutcome::Attempted {
            self.trades_attempted_count += 1;
            info!(
                "🚀 Trade Attempt: ID {} | Path: {} | {} {:.4} -> {} {:.4} | Expected Profit: {:.2}% / ${:.2}",
                opportunity_id, dex_path.join(" -> "), input_token, input_amount, output_token, expected_output_amount,
                estimated_profit_pct, estimated_profit_usd.unwrap_or(0.0)
            );
            return; // Just log attempt, further processing for Success/Failure/Skipped
        }

        // For Success, Failure, Skipped
        let outcome_str = match outcome {
            TradeOutcome::Success => {
                self.successful_trades_count += 1;
                self.last_successful_trade_timestamp = Some(Utc::now());
                if let Some(profit) = actual_profit_usd { self.total_profit_usd += profit; }
                "SUCCESS"
            }
            TradeOutcome::Failure => {
                self.failed_trades_count += 1;
                "FAILURE"
            }
            TradeOutcome::Skipped => {
                self.skipped_trades_count += 1;
                "SKIPPED"
            }
            TradeOutcome::Attempted => unreachable!(), // Already handled
        };
        
        self.total_tx_cost_sol += tx_cost_sol;
        if let Some(fees) = fees_paid_usd { self.total_fees_paid_usd += fees; }

        info!(
            "TRADE {}: ID {} | Path: {} | {} {:.4} -> {} {:.4} | Profit Pct: {:.2}% | Actual USD Profit: {:?} | Exec Time: {}ms | Tx Cost SOL: {:.8} | Fees USD: {:?} | TxID: {} | Error: {}",
            outcome_str, opportunity_id, dex_path.join(" -> "), input_token, input_amount, output_token, expected_output_amount,
            estimated_profit_pct, actual_profit_usd, execution_time_ms, tx_cost_sol, fees_paid_usd,
            transaction_id.as_deref().unwrap_or("N/A"), error_message.as_deref().unwrap_or("None")
        );
    }
    
    // Getter for log_file (remains unchanged)
    pub fn get_log_file(&self) -> Option<&Arc<tokio::sync::Mutex<tokio::fs::File>>> {
        self.log_file.as_ref()
    }

    pub fn summary(&self) {
        info!("-------------------- Metrics Summary --------------------");
        if let Some(lt) = self.launch_time_utc {
            let uptime = Utc::now().signed_duration_since(lt);
            info!("Uptime: {}h {}m {}s", uptime.num_hours(), uptime.num_minutes() % 60, uptime.num_seconds() % 60);
        }
        info!("Pools Fetched (current): {}", self.pools_fetched);
        info!("Trading Pairs Monitored: {}", self.trading_pairs_monitored);
        info!("Main Cycles Executed: {}", self.main_cycles_executed);
        info!("Opportunities Detected: {}", self.opportunities_detected_count);
        info!("Trades Attempted: {}", self.trades_attempted_count);
        info!("Successful Trades: {}", self.successful_trades_count);
        info!("Failed Trades: {}", self.failed_trades_count);
        info!("Skipped Trades: {}", self.skipped_trades_count);
        info!("Total Profit (USD): {:.2}", self.total_profit_usd);
        if self.successful_trades_count > 0 {
            info!("Average Profit per Successful Trade (USD): {:.2}", self.total_profit_usd / self.successful_trades_count as f64);
        }
        if let Some(last_update) = self.last_opportunity_timestamp {
            info!("Last Opportunity Detected At: {}Z", last_update.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        }
        if let Some(last_trade) = self.last_successful_trade_timestamp {
            info!("Last Successful Trade At: {}Z", last_trade.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        }
        info!("Current Dynamic Min Profit Threshold: {:.4}%", self.current_min_profit_threshold * 100.0); // Assuming this is fractional
        info!("System Health Status: {}", if self.system_healthy { "Healthy" } else { "Unhealthy" });
        info!("-------------------------------------------------------");
    }

    pub fn increment_main_cycles(&mut self) { self.main_cycles_executed += 1; }
    pub fn record_main_cycle_duration(&mut self, duration_ms: u64) { debug!("Main cycle duration: {}ms", duration_ms); }
    
    // current_min_profit_threshold is expected as fractional (e.g. 0.001 for 0.1%)
    pub fn log_dynamic_threshold_update(&mut self, new_threshold_fractional: f64) {
        self.current_min_profit_threshold = new_threshold_fractional;
        info!("Dynamic minimum profit threshold updated to: {:.4}%", new_threshold_fractional * 100.0);
    }
    pub fn set_system_health(&mut self, healthy: bool) {
        self.system_healthy = healthy;
        info!("System health status set to: {}", if healthy { "Healthy" } else { "Unhealthy" });
    }

    // Called from ArbitrageEngine when pools are updated
    pub fn log_pools_updated(&mut self, new_count: usize, updated_count: usize, total_count: usize) {
        self.pools_fetched = total_count; // Update current pool count
        info!("Pools updated - New: {}, Updated: {}, Total currently monitored: {}", new_count, updated_count, total_count);
    }
    
    // Placeholder if a separate method for degradation mode is needed, otherwise covered by set_system_health and threshold update
    pub fn log_degradation_mode_change(&mut self, entered_degradation: bool, new_threshold_fractional: Option<f64>) {
        if entered_degradation {
            info!("Entered degradation mode. New threshold: {:.4}%", new_threshold_fractional.unwrap_or(self.current_min_profit_threshold) * 100.0);
        } else {
            info!("Exited degradation mode. Threshold reset to: {:.4}%", new_threshold_fractional.unwrap_or(self.current_min_profit_threshold) * 100.0);
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Metrics::new(100.0, None)
    }
}