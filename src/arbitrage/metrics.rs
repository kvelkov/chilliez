use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::Duration,
};

/// Metrics collects various runtime statistics used for monitoring the arbitrage bot’s performance and health.
/// It is designed to be thread-safe and efficient for high-frequency updates.
pub struct Metrics {
    // Pool-related metrics.
    pools_new: AtomicU64,
    pools_updated: AtomicU64,
    total_pools: AtomicU64,

    // Opportunity metrics.
    opportunities_detected: AtomicU64,
    opportunities_executed_success: AtomicU64,
    opportunities_executed_failure: AtomicU64,

    // Profit metrics (stored in a mutex since f64 is not atomic).
    total_profit: Mutex<f64>,

    // Execution time metrics: record the count and the total execution time in milliseconds.
    execution_count: AtomicU64,
    total_execution_ms: AtomicU64,

    // Dynamic threshold updates count.
    dynamic_threshold_updates: AtomicU64,
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

    /// Call this method immediately after a successful execution. (Method Added)
    pub fn log_opportunity_executed_success(&self) {
        self.opportunities_executed_success.fetch_add(1, Ordering::Relaxed);
    }

    /// Call this method after a failed execution attempt. (Method Added)
    pub fn log_opportunity_executed_failure(&self) {
        self.opportunities_executed_failure.fetch_add(1, Ordering::Relaxed);
    }

    /// Update the total profit by adding the profit value.
    /// The profit can be a positive number (earning) or negative (loss).
    pub fn update_profit(&self, profit: f64) {
        let mut total = self.total_profit.lock().unwrap();
        *total += profit;
    }

    /// Record the execution time (duration) for an operation.
    pub fn record_execution_time(&self, duration: Duration) {
        self.execution_count.fetch_add(1, Ordering::Relaxed);
        self.total_execution_ms
            .fetch_add(duration.as_millis() as u64, Ordering::Relaxed);
    }

    /// Logs whenever dynamic thresholds are updated.
    /// - `new_threshold`: The new threshold value (expressed as a fraction; e.g., 0.005 represents 0.5%).
    pub fn log_dynamic_threshold_update(&self, new_threshold: f64) {
        self.dynamic_threshold_updates.fetch_add(1, Ordering::Relaxed);
        log::info!("Dynamic Threshold updated to: {:.4}%", new_threshold * 100.0);
    }

    /// Generates a formatted report summarizing all current metrics.
    pub fn report(&self) -> String {
        let total_profit = *self.total_profit.lock().unwrap();
        let exec_count = self.execution_count.load(Ordering::Relaxed);
        let avg_exec_ms = if exec_count > 0 {
            self.total_execution_ms.load(Ordering::Relaxed) as f64 / exec_count as f64
        } else {
            0.0
        };

        format!(
            "Pools - New: {}, Updated: {}, Total: {}. \
             Opportunities - Detected: {}, Executed Success: {}, Executed Failure: {}. \
             Profit - Total Profit: {:.2}. \
             Execution - Count: {}, Average Time: {:.2} ms. \
             DynamicThreshold Updates: {}.",
            self.pools_new.load(Ordering::Relaxed),
            self.pools_updated.load(Ordering::Relaxed),
            self.total_pools.load(Ordering::Relaxed),
            self.opportunities_detected.load(Ordering::Relaxed),
            self.opportunities_executed_success.load(Ordering::Relaxed),
            self.opportunities_executed_failure.load(Ordering::Relaxed),
            total_profit,
            exec_count,
            avg_exec_ms,
            self.dynamic_threshold_updates.load(Ordering::Relaxed)
        )
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}
