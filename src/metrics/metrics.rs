use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use log::info;

pub struct Metrics {
    // Atomic counters for thread-safe incrementing
    pools_new: AtomicU64,
    pools_updated: AtomicU64,
    total_pools: AtomicU64,
    opportunities_detected: AtomicU64,
    opportunities_executed_success: AtomicU64,
    opportunities_executed_failure: AtomicU64,
    execution_count: AtomicU64,
    total_execution_ms: AtomicU64,
    dynamic_threshold_updates: AtomicU64,
    // Mutex-protected values (no atomic f64 in Rust)
    total_profit: Mutex<f64>,
}

impl Metrics {
    /// Creates a new Metrics instance with all counters initialized.
    pub fn new() -> Self {
        Self {
            pools_new: AtomicU64::new(0),
            pools_updated: AtomicU64::new(0),
            total_pools: AtomicU64::new(0),
            opportunities_detected: AtomicU64::new(0),
            opportunities_executed_success: AtomicU64::new(0),
            opportunities_executed_failure: AtomicU64::new(0),
            total_profit: Mutex::new(0.0),
            execution_count: AtomicU64::new(0),
            total_execution_ms: AtomicU64::new(0),
            dynamic_threshold_updates: AtomicU64::new(0),
        }
    }

    /// Log pool updates.
    ///
    /// - `new`: The number of new pools added.
    /// - `updated`: The number of existing pools updated.
    /// - `total`: The total number of pools in the system.
    pub fn log_pools_updated(&self, new: u64, updated: u64, total: usize) {
        self.pools_new.fetch_add(new, Ordering::Relaxed);
        self.pools_updated.fetch_add(updated, Ordering::Relaxed);
        self.total_pools.store(total as u64, Ordering::Relaxed);
    }

    /// Logs the number of opportunities detected during a detection scan.
    pub fn log_opportunities_detected(&self, count: u64) {
        self.opportunities_detected.fetch_add(count, Ordering::Relaxed);
    }

    /// Call this method immediately after a successful execution.
    pub fn log_opportunity_executed_success(&self) {
        self.opportunities_executed_success.fetch_add(1, Ordering::Relaxed);
        info!("Metrics: Successful execution recorded");
    }

    /// Call this method immediately after a failed execution.
    pub fn log_opportunity_executed_failure(&self) {
        self.opportunities_executed_failure.fetch_add(1, Ordering::Relaxed);
        info!("Metrics: Failed execution recorded");
    }

    /// Updates the total profit, handling both positive and negative values.
    pub fn update_profit(&self, profit: f64) {
        let mut total = self.total_profit.lock().unwrap();
        *total += profit;
    }

    /// Records the execution time of an operation.
    pub fn record_execution_time(&self, duration_ms: u64) {
        self.execution_count.fetch_add(1, Ordering::Relaxed);
        self.total_execution_ms.fetch_add(duration_ms, Ordering::Relaxed);
    }

    /// Logs dynamic threshold updates.
    pub fn log_dynamic_threshold_update(&self, new_threshold: f64) {
        self.dynamic_threshold_updates.fetch_add(1, Ordering::Relaxed);
        info!("Dynamic threshold updated to: {:.4}%", new_threshold);
    }

    /// Records the duration of a main cycle.
    pub fn record_main_cycle_duration(&self, duration_ms: u64) {
        self.record_execution_time(duration_ms);
    }

    /// Increments the main cycles counter.
    pub fn increment_main_cycles(&self) {
        self.execution_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Logs the launch of the application.
    pub fn log_launch(&self) {
        info!("Application launched. Metrics tracking started.");
    }

    /// Logs the number of pools fetched.
    pub fn log_pools_fetched(&self, count: usize) {
        self.total_pools.store(count as u64, Ordering::Relaxed);
        info!("Fetched {} pools", count);
    }

    /// Generates a report of all metrics.
    pub fn summary(&self) -> String {
        let report = format!(
            "Metrics Summary:\n\
             - Pool Statistics: {} new, {} updated, {} total\n\
             - Opportunity Statistics: {} detected, {} executed successfully, {} failed\n\
             - Total Profit: ${:.2}\n\
             - Execution Statistics: {} operations, {:.2}ms average execution time\n\
             - Dynamic Threshold Updates: {}",
            self.pools_new.load(Ordering::Relaxed),
            self.pools_updated.load(Ordering::Relaxed),
            self.total_pools.load(Ordering::Relaxed),
            self.opportunities_detected.load(Ordering::Relaxed),
            self.opportunities_executed_success.load(Ordering::Relaxed),
            self.opportunities_executed_failure.load(Ordering::Relaxed),
            *self.total_profit.lock().unwrap(),
            self.execution_count.load(Ordering::Relaxed),
            if self.execution_count.load(Ordering::Relaxed) > 0 {
                self.total_execution_ms.load(Ordering::Relaxed) as f64
                    / self.execution_count.load(Ordering::Relaxed) as f64
            } else {
                0.0
            },
            self.dynamic_threshold_updates.load(Ordering::Relaxed)
        );
        info!("{}", report);
        report
    }

    /// Records an opportunity detection.
    pub fn record_opportunity_detected(
        &self,
        input_token: &str,
        intermediate_token: &str,
        profit_pct: f64,
        estimated_profit_usd: Option<f64>,
        input_amount_usd: Option<f64>,
        dex_path: Vec<String>,
    ) -> Result<(), String> {
        self.opportunities_detected.fetch_add(1, Ordering::Relaxed);
        info!(
            "Detected opportunity: {} -> {} -> {}, Profit: {:.4}%, Est. USD: {:?}, Input USD: {:?}, Path: {:?}",
            input_token, intermediate_token, input_token, profit_pct, estimated_profit_usd, input_amount_usd, dex_path
        );
        Ok(())
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}