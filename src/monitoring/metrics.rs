//! Unified Metrics Collection and Analysis Module
//!
//! This module consolidates all metrics functionality from the previous
//! metrics, local_metrics, and performance modules into a single unified
//! monitoring system.

use anyhow::Result;
use log::{debug, info};
use metrics::{counter, gauge, histogram};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};
use sysinfo::System;

// =============================================================================
// Core Metrics Structure (merged from metrics and local_metrics)
// =============================================================================

/// Primary metrics collector for the arbitrage bot
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

    // =============================================================================
    // Pool Metrics (from both implementations)
    // =============================================================================

    /// Log pool updates with external metrics integration
    pub fn log_pools_updated(&self, new: u64, updated: u64, total: usize) {
        counter!("pools_new");
        counter!("pools_updated");
        gauge!("total_pools").set(total as f64);

        self.pools_new.fetch_add(new, Ordering::Relaxed);
        self.pools_updated.fetch_add(updated, Ordering::Relaxed);
        self.total_pools.store(total as u64, Ordering::Relaxed);
    }

    /// Increments the pools_new counter
    pub fn increment_pools_new(&self) {
        self.pools_new.fetch_add(1, Ordering::SeqCst);
    }

    /// Increments the pools_updated counter
    pub fn increment_pools_updated(&self) {
        self.pools_updated.fetch_add(1, Ordering::SeqCst);
    }

    /// Increments the total_pools counter
    pub fn increment_total_pools(&self) {
        self.total_pools.fetch_add(1, Ordering::SeqCst);
    }

    /// Logs the number of pools fetched
    pub fn log_pools_fetched(&self, count: usize) {
        gauge!("pools_fetched").set(count as f64);
        info!("Fetched {} pools", count);
    }

    // =============================================================================
    // Opportunity Metrics
    // =============================================================================

    /// Logs the number of opportunities detected during a detection scan
    pub fn log_opportunities_detected(&self, count: u64) {
        counter!("opportunities_detected");
        self.opportunities_detected.fetch_add(count, Ordering::Relaxed);
    }

    /// Increments the opportunities_detected counter
    pub fn increment_opportunities_detected(&self) {
        counter!("opportunities_detected");
        self.opportunities_detected.fetch_add(1, Ordering::Relaxed);
    }

    /// Log opportunity executed successfully 
    pub fn log_opportunity_executed_success(&self) {
        counter!("opportunities_executed_success");
        self.opportunities_executed_success.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the opportunities_executed_success counter
    pub fn increment_opportunities_executed_success(&self) {
        self.opportunities_executed_success.fetch_add(1, Ordering::SeqCst);
    }

    /// Call this method immediately after a failed execution
    pub fn log_opportunity_executed_failure(&self) {
        counter!("opportunities_executed_failure");
        self.opportunities_executed_failure.fetch_add(1, Ordering::SeqCst);
        info!("Metrics: Failed execution recorded");
    }

    /// Increments the opportunities_executed_failure counter
    pub fn increment_opportunities_executed_failure(&self) {
        self.opportunities_executed_failure.fetch_add(1, Ordering::SeqCst);
    }

    /// Records an opportunity detection with detailed information
    pub fn record_opportunity_detected(
        &self,
        input_token: &str,
        intermediate_token: &str,
        profit_pct: f64,
        estimated_profit_usd: Option<f64>,
        input_amount_usd: Option<f64>,
        dex_path: Vec<String>,
    ) -> Result<(), String> {
        counter!("opportunities_detected");
        self.increment_opportunities_detected();
        info!(
            "Detected opportunity: {} -> {} -> {}, Profit: {:.4}%, Est. USD: {:?}, Input USD: {:?}, Path: {:?}",
            input_token, intermediate_token, input_token, profit_pct, estimated_profit_usd, input_amount_usd, dex_path
        );
        Ok(())
    }

    // =============================================================================
    // Execution and Performance Metrics
    // =============================================================================

    /// Records the execution time of an operation
    pub fn record_execution_time(&self, duration_ms: u64) {
        histogram!("execution_time_ms").record(duration_ms as f64);
        counter!("execution_count");

        self.execution_count.fetch_add(1, Ordering::Relaxed);
        self.total_execution_ms.fetch_add(duration_ms, Ordering::Relaxed);
    }

    /// Records execution time using Duration
    pub fn record_execution_time_duration(&self, duration: Duration) {
        let ms = duration.as_millis() as u64;
        self.record_execution_time(ms);
    }

    /// Increments the execution_count counter
    pub fn increment_execution_count(&self) {
        self.execution_count.fetch_add(1, Ordering::SeqCst);
    }

    /// Adds to the total_execution_ms counter
    pub fn add_to_total_execution_ms(&self, latency_ms: u64) {
        self.total_execution_ms.fetch_add(latency_ms, Ordering::Relaxed);
        histogram!("execution_time_ms").record(latency_ms as f64);
    }

    /// Records the duration of a main cycle
    pub fn record_main_cycle_duration(&self, duration_ms: u64) {
        histogram!("main_cycle_duration_ms").record(duration_ms as f64);
        self.record_execution_time(duration_ms);
    }

    /// Increments the main cycles counter
    pub fn increment_main_cycles(&self) {
        counter!("main_cycles_executed");
        self.execution_count.fetch_add(1, Ordering::Relaxed);
    }

    // =============================================================================
    // Profit and Financial Metrics
    // =============================================================================

    /// Updates the total profit, handling both positive and negative values
    pub fn update_profit(&self, profit: f64) {
        gauge!("total_profit").set(profit);
        let mut total = self.total_profit.lock().unwrap();
        *total += profit;
    }

    /// Adds to the total_profit value
    pub fn add_to_total_profit(&self, profit: f64) {
        let mut total_profit = self.total_profit.lock().unwrap();
        *total_profit += profit;
        gauge!("total_profit").set(*total_profit);
    }

    // =============================================================================
    // Dynamic Threshold Metrics
    // =============================================================================

    /// Logs dynamic threshold updates
    pub fn log_dynamic_threshold_update(&self, new_threshold: f64) {
        counter!("dynamic_threshold_updates");
        gauge!("current_dynamic_threshold").set(new_threshold);
        self.dynamic_threshold_updates.fetch_add(1, Ordering::Relaxed);
        info!("Dynamic threshold updated to: {:.4}%", new_threshold);
    }

    /// Increments the dynamic_threshold_updates counter
    pub fn increment_dynamic_threshold_updates(&self) {
        self.dynamic_threshold_updates.fetch_add(1, Ordering::SeqCst);
    }

    // =============================================================================
    // Logging and Reporting
    // =============================================================================

    /// Logs the launch of the application
    pub fn log_launch(&self) {
        info!("Application launched. Metrics tracking started.");
    }

    /// Logs the current metrics values
    pub fn log_metrics(&self) {
        info!("Current Metrics:");
        info!("  New Pools: {}", self.pools_new.load(Ordering::SeqCst));
        info!("  Updated Pools: {}", self.pools_updated.load(Ordering::SeqCst));
        info!("  Total Pools: {}", self.total_pools.load(Ordering::SeqCst));
        info!("  Opportunities Detected: {}", self.opportunities_detected.load(Ordering::SeqCst));
        info!("  Opportunities Executed (Success): {}", self.opportunities_executed_success.load(Ordering::SeqCst));
        info!("  Opportunities Executed (Failure): {}", self.opportunities_executed_failure.load(Ordering::SeqCst));
        info!("  Execution Count: {}", self.execution_count.load(Ordering::SeqCst));
        info!("  Total Execution Time (ms): {}", self.total_execution_ms.load(Ordering::SeqCst));
        info!("  Dynamic Threshold Updates: {}", self.dynamic_threshold_updates.load(Ordering::SeqCst));
        let total_profit = self.total_profit.lock().unwrap();
        info!("  Total Profit: {}", *total_profit);
    }

    /// Generates a comprehensive report of all metrics
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
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Performance Metrics (from performance/metrics.rs)
// =============================================================================

/// Comprehensive metrics collector for performance analysis
pub struct MetricsCollector {
    operation_metrics: HashMap<String, OperationMetrics>,
    system_metrics: SystemMetrics,
    latency_tracker: LatencyTracker,
    throughput_tracker: ThroughputTracker,
    error_tracker: ErrorTracker,
    start_time: Instant,
    system_info: System,
}

/// Metrics for specific operations
#[derive(Debug, Clone, Default)]
pub struct OperationMetrics {
    pub total_operations: u64,
    pub successful_operations: u64,
    pub failed_operations: u64,
    pub total_duration: Duration,
    pub min_duration: Option<Duration>,
    pub max_duration: Option<Duration>,
    pub avg_duration: Duration,
    pub p95_duration: Duration,
    pub p99_duration: Duration,
    pub last_operation: Option<Instant>,
}

/// System-level metrics
#[derive(Debug, Clone, Default)]
pub struct SystemMetrics {
    pub cpu_usage: f32,
    pub memory_usage_mb: f64,
    pub memory_available_mb: f64,
    pub memory_total_mb: f64,
    pub disk_usage_mb: f64,
    pub network_rx_mb: f64,
    pub network_tx_mb: f64,
    pub open_file_descriptors: u32,
    pub process_count: u32,
    pub uptime_seconds: u64,
}

/// Latency tracking for performance analysis
#[derive(Debug, Clone)]
pub struct LatencyTracker {
    samples: VecDeque<Duration>,
    max_samples: usize,
}

/// Throughput tracking
#[derive(Debug, Clone)]
pub struct ThroughputTracker {
    operations: VecDeque<Instant>,
    window_duration: Duration,
}

/// Error tracking and analysis
#[derive(Debug, Clone, Default)]
pub struct ErrorTracker {
    total_errors: u64,
    error_types: HashMap<String, u64>,
    error_rate_window: VecDeque<(Instant, bool)>,
    window_duration: Duration,
}

/// Summary of all collected metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSummary {
    pub timestamp: u64,
    pub system_metrics: SystemMetricsSummary,
    pub operation_metrics: HashMap<String, OperationMetricsSummary>,
    pub latency_summary: LatencySummary,
    pub throughput_ops_per_second: f64,
    pub error_rate: f64,
    pub uptime_seconds: u64,
}

/// Serializable system metrics summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetricsSummary {
    pub cpu_usage: f32,
    pub memory_usage_mb: f64,
    pub memory_usage_percent: f64,
    pub disk_usage_mb: f64,
    pub network_rx_mb: f64,
    pub network_tx_mb: f64,
    pub open_file_descriptors: u32,
}

/// Serializable operation metrics summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationMetricsSummary {
    pub total_operations: u64,
    pub successful_operations: u64,
    pub failed_operations: u64,
    pub success_rate: f64,
    pub avg_duration_ms: f64,
    pub p95_duration_ms: f64,
    pub p99_duration_ms: f64,
    pub operations_per_second: f64,
}

/// Latency analysis summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencySummary {
    pub avg_latency_ms: f64,
    pub p50_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub min_latency_ms: f64,
    pub max_latency_ms: f64,
}

impl MetricsCollector {
    /// Create a new MetricsCollector instance
    pub fn new() -> Self {
        Self {
            operation_metrics: HashMap::new(),
            system_metrics: SystemMetrics::default(),
            latency_tracker: LatencyTracker::new(1000),
            throughput_tracker: ThroughputTracker::new(Duration::from_secs(60)),
            error_tracker: ErrorTracker::new(Duration::from_secs(300)),
            start_time: Instant::now(),
            system_info: System::new_all(),
        }
    }

    /// Record an operation with its duration and success status
    pub fn record_operation(&mut self, operation_name: &str, duration: Duration, success: bool) {
        // Update operation-specific metrics
        let metrics = self.operation_metrics.entry(operation_name.to_string()).or_default();
        metrics.total_operations += 1;
        
        if success {
            metrics.successful_operations += 1;
        } else {
            metrics.failed_operations += 1;
        }

        metrics.total_duration += duration;
        metrics.last_operation = Some(Instant::now());

        // Update min/max
        if metrics.min_duration.is_none() || Some(duration) < metrics.min_duration {
            metrics.min_duration = Some(duration);
        }
        if metrics.max_duration.is_none() || Some(duration) > metrics.max_duration {
            metrics.max_duration = Some(duration);
        }

        // Update latency tracking
        self.latency_tracker.record(duration);
        
        // Update throughput tracking
        self.throughput_tracker.record_operation();

        // Update error tracking
        self.error_tracker.record_operation(success);

        debug!("Recorded operation '{}': duration={:?}, success={}", operation_name, duration, success);
    }

    /// Update system metrics
    pub fn update_system_metrics(&mut self) {
        self.system_info.refresh_all();

        self.system_metrics.cpu_usage = self.system_info.global_cpu_info().cpu_usage();
        self.system_metrics.memory_usage_mb = self.system_info.used_memory() as f64 / 1_024_000.0;
        self.system_metrics.memory_available_mb = self.system_info.available_memory() as f64 / 1_024_000.0;
        self.system_metrics.memory_total_mb = self.system_info.total_memory() as f64 / 1_024_000.0;
        self.system_metrics.uptime_seconds = self.start_time.elapsed().as_secs();
    }

    /// Get a comprehensive metrics summary
    pub fn get_summary(&mut self) -> MetricsSummary {
        self.update_system_metrics();

        let mut operation_summaries = HashMap::new();
        for (name, metrics) in &self.operation_metrics {
            let summary = OperationMetricsSummary {
                total_operations: metrics.total_operations,
                successful_operations: metrics.successful_operations,
                failed_operations: metrics.failed_operations,
                success_rate: if metrics.total_operations > 0 {
                    metrics.successful_operations as f64 / metrics.total_operations as f64 * 100.0
                } else {
                    0.0
                },
                avg_duration_ms: if metrics.total_operations > 0 {
                    metrics.total_duration.as_millis() as f64 / metrics.total_operations as f64
                } else {
                    0.0
                },
                p95_duration_ms: metrics.p95_duration.as_millis() as f64,
                p99_duration_ms: metrics.p99_duration.as_millis() as f64,
                operations_per_second: self.throughput_tracker.get_ops_per_second(),
            };
            operation_summaries.insert(name.clone(), summary);
        }

        MetricsSummary {
            timestamp: SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
            system_metrics: SystemMetricsSummary {
                cpu_usage: self.system_metrics.cpu_usage,
                memory_usage_mb: self.system_metrics.memory_usage_mb,
                memory_usage_percent: if self.system_metrics.memory_total_mb > 0.0 {
                    self.system_metrics.memory_usage_mb / self.system_metrics.memory_total_mb * 100.0
                } else {
                    0.0
                },
                disk_usage_mb: self.system_metrics.disk_usage_mb,
                network_rx_mb: self.system_metrics.network_rx_mb,
                network_tx_mb: self.system_metrics.network_tx_mb,
                open_file_descriptors: self.system_metrics.open_file_descriptors,
            },
            operation_metrics: operation_summaries,
            latency_summary: self.latency_tracker.get_summary(),
            throughput_ops_per_second: self.throughput_tracker.get_ops_per_second(),
            error_rate: self.error_tracker.get_error_rate(),
            uptime_seconds: self.system_metrics.uptime_seconds,
        }
    }
}

impl LatencyTracker {
    pub fn new(max_samples: usize) -> Self {
        Self {
            samples: VecDeque::new(),
            max_samples,
        }
    }

    pub fn record(&mut self, duration: Duration) {
        self.samples.push_back(duration);
        if self.samples.len() > self.max_samples {
            self.samples.pop_front();
        }
    }

    pub fn get_summary(&self) -> LatencySummary {
        if self.samples.is_empty() {
            return LatencySummary {
                avg_latency_ms: 0.0,
                p50_latency_ms: 0.0,
                p95_latency_ms: 0.0,
                p99_latency_ms: 0.0,
                min_latency_ms: 0.0,
                max_latency_ms: 0.0,
            };
        }

        let mut sorted_samples: Vec<_> = self.samples.iter().collect();
        sorted_samples.sort();

        let avg = self.samples.iter().sum::<Duration>().as_millis() as f64 / self.samples.len() as f64;
        let p50_idx = (sorted_samples.len() as f64 * 0.50) as usize;
        let p95_idx = (sorted_samples.len() as f64 * 0.95) as usize;
        let p99_idx = (sorted_samples.len() as f64 * 0.99) as usize;

        LatencySummary {
            avg_latency_ms: avg,
            p50_latency_ms: sorted_samples.get(p50_idx).unwrap_or(&&Duration::ZERO).as_millis() as f64,
            p95_latency_ms: sorted_samples.get(p95_idx).unwrap_or(&&Duration::ZERO).as_millis() as f64,
            p99_latency_ms: sorted_samples.get(p99_idx).unwrap_or(&&Duration::ZERO).as_millis() as f64,
            min_latency_ms: sorted_samples.first().unwrap_or(&&Duration::ZERO).as_millis() as f64,
            max_latency_ms: sorted_samples.last().unwrap_or(&&Duration::ZERO).as_millis() as f64,
        }
    }
}

impl ThroughputTracker {
    pub fn new(window_duration: Duration) -> Self {
        Self {
            operations: VecDeque::new(),
            window_duration,
        }
    }

    pub fn record_operation(&mut self) {
        let now = Instant::now();
        self.operations.push_back(now);
        
        // Remove old operations outside the window
        let cutoff = now - self.window_duration;
        while let Some(&front) = self.operations.front() {
            if front < cutoff {
                self.operations.pop_front();
            } else {
                break;
            }
        }
    }

    pub fn get_ops_per_second(&self) -> f64 {
        if self.operations.is_empty() {
            return 0.0;
        }

        let window_secs = self.window_duration.as_secs_f64();
        self.operations.len() as f64 / window_secs
    }
}

impl ErrorTracker {
    pub fn new(window_duration: Duration) -> Self {
        Self {
            total_errors: 0,
            error_types: HashMap::new(),
            error_rate_window: VecDeque::new(),
            window_duration,
        }
    }

    pub fn record_operation(&mut self, success: bool) {
        let now = Instant::now();
        self.error_rate_window.push_back((now, success));

        if !success {
            self.total_errors += 1;
        }

        // Remove old operations outside the window
        let cutoff = now - self.window_duration;
        while let Some(&(timestamp, _)) = self.error_rate_window.front() {
            if timestamp < cutoff {
                self.error_rate_window.pop_front();
            } else {
                break;
            }
        }
    }

    pub fn get_error_rate(&self) -> f64 {
        if self.error_rate_window.is_empty() {
            return 0.0;
        }

        let errors = self.error_rate_window.iter().filter(|(_, success)| !*success).count();
        errors as f64 / self.error_rate_window.len() as f64 * 100.0
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}
