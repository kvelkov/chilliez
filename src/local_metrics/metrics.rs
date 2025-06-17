use log::info;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

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
            execution_count: AtomicU64::new(0),
            total_execution_ms: AtomicU64::new(0),
            dynamic_threshold_updates: AtomicU64::new(0),
            total_profit: Mutex::new(0.0),
        }
    }
    /// Increments the `pools_new` counter.
    pub fn increment_pools_new(&self) {
        self.pools_new.fetch_add(1, Ordering::SeqCst);
    }

    /// Increments the `pools_updated` counter.
    pub fn increment_pools_updated(&self) {
        self.pools_updated.fetch_add(1, Ordering::SeqCst);
    }

    /// Increments the `total_pools` counter.
    pub fn increment_total_pools(&self) {
        self.total_pools.fetch_add(1, Ordering::SeqCst);
    }

    /// Increments the `opportunities_detected` counter.
    pub fn increment_opportunities_detected(&self) {
        self.opportunities_detected.fetch_add(1, Ordering::SeqCst);
    }

    /// Increments the `opportunities_executed_success` counter.
    pub fn increment_opportunities_executed_success(&self) {
        self.opportunities_executed_success
            .fetch_add(1, Ordering::SeqCst);
    }

    /// Increments the `opportunities_executed_failure` counter.
    pub fn increment_opportunities_executed_failure(&self) {
        self.opportunities_executed_failure
            .fetch_add(1, Ordering::SeqCst);
    }

    /// Increments the `execution_count` counter.
    pub fn increment_execution_count(&self) {
        self.execution_count.fetch_add(1, Ordering::SeqCst);
    }

    /// Adds to the `total_execution_ms` counter.
    pub fn add_to_total_execution_ms(&self, ms: u64) {
        self.total_execution_ms.fetch_add(ms, Ordering::SeqCst);
    }

    /// Increments the `dynamic_threshold_updates` counter.
    pub fn increment_dynamic_threshold_updates(&self) {
        self.dynamic_threshold_updates
            .fetch_add(1, Ordering::SeqCst);
    }

    /// Adds to the `total_profit` value.
    pub fn add_to_total_profit(&self, profit: f64) {
        let mut total_profit = self.total_profit.lock().unwrap();
        *total_profit += profit;
    }

    /// Records execution time and increments execution count.
    pub fn record_execution_time(&self, duration: std::time::Duration) {
        let ms = duration.as_millis() as u64;
        self.add_to_total_execution_ms(ms);
        self.increment_execution_count();
    }

    /// Logs a successful opportunity execution.
    pub fn log_opportunity_executed_success(&self) {
        self.increment_opportunities_executed_success();
    }

    /// Logs a failed opportunity execution.
    pub fn log_opportunity_executed_failure(&self) {
        self.increment_opportunities_executed_failure();
    }

    /// Logs the current metrics values.
    pub fn log_metrics(&self) {
        info!("Current Metrics:");
        info!("  New Pools: {}", self.pools_new.load(Ordering::SeqCst));
        info!(
            "  Updated Pools: {}",
            self.pools_updated.load(Ordering::SeqCst)
        );
        info!("  Total Pools: {}", self.total_pools.load(Ordering::SeqCst));
        info!(
            "  Opportunities Detected: {}",
            self.opportunities_detected.load(Ordering::SeqCst)
        );
        info!(
            "  Opportunities Executed (Success): {}",
            self.opportunities_executed_success.load(Ordering::SeqCst)
        );
        info!(
            "  Opportunities Executed (Failure): {}",
            self.opportunities_executed_failure.load(Ordering::SeqCst)
        );
        info!(
            "  Execution Count: {}",
            self.execution_count.load(Ordering::SeqCst)
        );
        info!(
            "  Total Execution Time (ms): {}",
            self.total_execution_ms.load(Ordering::SeqCst)
        );
        info!(
            "  Dynamic Threshold Updates: {}",
            self.dynamic_threshold_updates.load(Ordering::SeqCst)
        );
        let total_profit = self.total_profit.lock().unwrap();
        info!("  Total Profit: {}", *total_profit);
    }
}
