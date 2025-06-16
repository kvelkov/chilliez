// src/performance/metrics.rs
//! Performance Metrics Collection and Analysis
//! 
//! This module provides comprehensive performance monitoring including:
//! - System resource usage tracking
//! - Latency measurements
//! - Throughput analysis
//! - Error rate monitoring

use anyhow::Result;
use serde::{Serialize, Deserialize};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant, SystemTime};
use sysinfo::System;
use log::{debug};

/// Comprehensive metrics collector
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
    pub network_bytes_sent: u64,
    pub network_bytes_received: u64,
    pub process_count: usize,
    pub thread_count: usize,
    pub uptime: Duration,
}

/// Latency tracking with percentile calculations
#[derive(Debug)]
pub struct LatencyTracker {
    samples: VecDeque<Duration>,
    max_samples: usize,
}

impl LatencyTracker {
    fn new(max_samples: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(max_samples),
            max_samples,
        }
    }

    fn record(&mut self, latency: Duration) {
        if self.samples.len() >= self.max_samples {
            self.samples.pop_front();
        }
        self.samples.push_back(latency);
    }

    fn percentile(&self, p: f64) -> Duration {
        if self.samples.is_empty() {
            return Duration::from_millis(0);
        }

        let mut sorted: Vec<_> = self.samples.iter().cloned().collect();
        sorted.sort();

        let index = ((sorted.len() as f64 - 1.0) * p / 100.0) as usize;
        sorted[index.min(sorted.len() - 1)]
    }

    fn average(&self) -> Duration {
        if self.samples.is_empty() {
            return Duration::from_millis(0);
        }

        let total: Duration = self.samples.iter().sum();
        total / self.samples.len() as u32
    }
}

/// Throughput tracking with time windows
#[derive(Debug)]
pub struct ThroughputTracker {
    operations: VecDeque<Instant>,
    window_duration: Duration,
}

impl ThroughputTracker {
    fn new(window_duration: Duration) -> Self {
        Self {
            operations: VecDeque::new(),
            window_duration,
        }
    }

    fn record_operation(&mut self) {
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

    fn operations_per_second(&self) -> f64 {
        if self.operations.is_empty() {
            return 0.0;
        }

        let window_secs = self.window_duration.as_secs_f64();
        self.operations.len() as f64 / window_secs
    }
}

/// Error tracking with categorization
#[derive(Debug, Default)]
pub struct ErrorTracker {
    error_counts: HashMap<String, u64>,
    total_errors: u64,
    error_rate_window: VecDeque<(Instant, bool)>, // (timestamp, is_error)
}

impl ErrorTracker {
    fn record_success(&mut self) {
        self.error_rate_window.push_back((Instant::now(), false));
        self.cleanup_old_entries();
    }

    fn record_error(&mut self, error_type: &str) {
        self.total_errors += 1;
        *self.error_counts.entry(error_type.to_string()).or_insert(0) += 1;
        self.error_rate_window.push_back((Instant::now(), true));
        self.cleanup_old_entries();
    }

    fn cleanup_old_entries(&mut self) {
        let cutoff = Instant::now() - Duration::from_secs(300); // 5 minute window
        while let Some(&(timestamp, _)) = self.error_rate_window.front() {
            if timestamp < cutoff {
                self.error_rate_window.pop_front();
            } else {
                break;
            }
        }
    }

    fn error_rate(&self) -> f64 {
        if self.error_rate_window.is_empty() {
            return 0.0;
        }

        let error_count = self.error_rate_window.iter()
            .filter(|(_, is_error)| *is_error)
            .count();
        
        error_count as f64 / self.error_rate_window.len() as f64
    }
}

/// Summary of all collected metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSummary {
    pub timestamp: u64,
    pub uptime_seconds: u64,
    pub cpu_usage: f32,
    pub memory_usage_mb: f64,
    pub network_latency_ms: f64,
    pub throughput_ops_per_sec: f64,
    pub error_rate: f64,
    pub operation_stats: HashMap<String, OperationStats>,
    pub system_health_score: f64, // 0.0 to 1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationStats {
    pub total_count: u64,
    pub success_rate: f64,
    pub avg_duration_ms: f64,
    pub p95_duration_ms: f64,
    pub p99_duration_ms: f64,
}

impl MetricsCollector {
    /// Create a new metrics collector
    pub fn new() -> Self {
        let mut system_info = System::new_all();
        system_info.refresh_all();

        Self {
            operation_metrics: HashMap::new(),
            system_metrics: SystemMetrics::default(),
            latency_tracker: LatencyTracker::new(10000), // Keep last 10k samples
            throughput_tracker: ThroughputTracker::new(Duration::from_secs(60)), // 1 minute window
            error_tracker: ErrorTracker::default(),
            start_time: Instant::now(),
            system_info,
        }
    }

    /// Record an operation completion
    pub fn record_operation(&mut self, operation_name: &str, duration: Duration, success: bool) {
        // Update operation metrics
        let metrics = self.operation_metrics.entry(operation_name.to_string()).or_default();
        metrics.total_operations += 1;
        metrics.total_duration += duration;
        metrics.last_operation = Some(Instant::now());

        if success {
            metrics.successful_operations += 1;
            self.error_tracker.record_success();
        } else {
            metrics.failed_operations += 1;
            self.error_tracker.record_error(operation_name);
        }

        // Update min/max duration
        if metrics.min_duration.is_none() || Some(duration) < metrics.min_duration {
            metrics.min_duration = Some(duration);
        }
        if metrics.max_duration.is_none() || Some(duration) > metrics.max_duration {
            metrics.max_duration = Some(duration);
        }

        // Update average duration
        metrics.avg_duration = metrics.total_duration / metrics.total_operations as u32;

        // Record for latency tracking
        self.latency_tracker.record(duration);
        
        // Record for throughput tracking
        self.throughput_tracker.record_operation();

        debug!("Recorded operation '{}': duration={:?}, success={}", operation_name, duration, success);
    }

    /// Record system statistics
    pub async fn record_system_stats(&mut self) {
        self.system_info.refresh_all();

        // CPU usage
        self.system_metrics.cpu_usage = self.system_info.global_cpu_info().cpu_usage();

        // Memory usage
        self.system_metrics.memory_total_mb = self.system_info.total_memory() as f64 / 1024.0 / 1024.0;
        self.system_metrics.memory_available_mb = self.system_info.available_memory() as f64 / 1024.0 / 1024.0;
        self.system_metrics.memory_usage_mb = self.system_metrics.memory_total_mb - self.system_metrics.memory_available_mb;

        // Process information
        self.system_metrics.process_count = self.system_info.processes().len();
        
        // Uptime
        self.system_metrics.uptime = self.start_time.elapsed();

        debug!("System stats updated: CPU={:.1}%, Memory={:.1}MB", 
               self.system_metrics.cpu_usage, self.system_metrics.memory_usage_mb);
    }

    /// Calculate health score based on various metrics
    pub fn calculate_health_score(&self) -> f64 {
        let mut score: f64 = 1.0;

        // CPU health (penalty for high usage)
        if self.system_metrics.cpu_usage > 80.0 {
            score -= 0.3;
        } else if self.system_metrics.cpu_usage > 60.0 {
            score -= 0.1;
        }

        // Memory health (penalty for high usage)
        let memory_usage_ratio = self.system_metrics.memory_usage_mb / self.system_metrics.memory_total_mb;
        if memory_usage_ratio > 0.9 {
            score -= 0.3;
        } else if memory_usage_ratio > 0.7 {
            score -= 0.1;
        }

        // Error rate health
        let error_rate = self.error_tracker.error_rate();
        if error_rate > 0.1 {
            score -= 0.4;
        } else if error_rate > 0.05 {
            score -= 0.2;
        }

        // Throughput health (penalty for very low throughput)
        let throughput = self.throughput_tracker.operations_per_second();
        if throughput < 1.0 {
            score -= 0.1;
        }

        score.max(0.0).min(1.0)
    }

    /// Get comprehensive metrics summary
    pub fn get_summary(&self) -> MetricsSummary {
        let mut operation_stats = HashMap::new();
        
        for (name, metrics) in &self.operation_metrics {
            let success_rate = if metrics.total_operations > 0 {
                metrics.successful_operations as f64 / metrics.total_operations as f64
            } else {
                1.0
            };

            operation_stats.insert(name.clone(), OperationStats {
                total_count: metrics.total_operations,
                success_rate,
                avg_duration_ms: metrics.avg_duration.as_millis() as f64,
                p95_duration_ms: self.latency_tracker.percentile(95.0).as_millis() as f64,
                p99_duration_ms: self.latency_tracker.percentile(99.0).as_millis() as f64,
            });
        }

        MetricsSummary {
            timestamp: SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default().as_secs(),
            uptime_seconds: self.system_metrics.uptime.as_secs(),
            cpu_usage: self.system_metrics.cpu_usage,
            memory_usage_mb: self.system_metrics.memory_usage_mb,
            network_latency_ms: self.latency_tracker.average().as_millis() as f64,
            throughput_ops_per_sec: self.throughput_tracker.operations_per_second(),
            error_rate: self.error_tracker.error_rate(),
            operation_stats,
            system_health_score: self.calculate_health_score(),
        }
    }

    /// Reset all metrics
    pub fn reset(&mut self) {
        self.operation_metrics.clear();
        self.latency_tracker = LatencyTracker::new(10000);
        self.throughput_tracker = ThroughputTracker::new(Duration::from_secs(60));
        self.error_tracker = ErrorTracker::default();
        self.start_time = Instant::now();
    }

    /// Export metrics to JSON
    pub fn export_json(&self) -> Result<String> {
        let summary = self.get_summary();
        serde_json::to_string_pretty(&summary).map_err(|e| anyhow::anyhow!("JSON export failed: {}", e))
    }

    /// Get detailed performance report
    pub fn get_detailed_report(&self) -> String {
        let summary = self.get_summary();
        
        format!(
            "PERFORMANCE METRICS REPORT\n\
             ==========================\n\
             Timestamp: {}\n\
             Uptime: {:.1} hours\n\
             \n\
             SYSTEM HEALTH\n\
             -------------\n\
             Health Score: {:.1}%\n\
             CPU Usage: {:.1}%\n\
             Memory Usage: {:.1} MB ({:.1}% of total)\n\
             Error Rate: {:.2}%\n\
             \n\
             PERFORMANCE\n\
             -----------\n\
             Throughput: {:.1} operations/second\n\
             Average Latency: {:.1} ms\n\
             P95 Latency: {:.1} ms\n\
             P99 Latency: {:.1} ms\n\
             \n\
             OPERATIONS\n\
             ----------\n\
             {}",
            summary.timestamp,
            summary.uptime_seconds as f64 / 3600.0,
            summary.system_health_score * 100.0,
            summary.cpu_usage,
            summary.memory_usage_mb,
            summary.memory_usage_mb / (summary.memory_usage_mb + 1000.0) * 100.0, // Approximate percentage
            summary.error_rate * 100.0,
            summary.throughput_ops_per_sec,
            summary.network_latency_ms,
            summary.operation_stats.get("route_calculation")
                .map(|s| s.p95_duration_ms)
                .unwrap_or(0.0),
            summary.operation_stats.get("route_calculation")
                .map(|s| s.p99_duration_ms)
                .unwrap_or(0.0),
            self.format_operation_stats(&summary.operation_stats)
        )
    }

    fn format_operation_stats(&self, stats: &HashMap<String, OperationStats>) -> String {
        let mut result = String::new();
        
        for (operation, stat) in stats {
            result.push_str(&format!(
                "{}: {} ops, {:.1}% success, {:.1}ms avg\n",
                operation,
                stat.total_count,
                stat.success_rate * 100.0,
                stat.avg_duration_ms
            ));
        }
        
        if result.is_empty() {
            result = "No operations recorded yet\n".to_string();
        }
        
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_latency_tracker() {
        let mut tracker = LatencyTracker::new(5);
        
        // Add some samples
        tracker.record(Duration::from_millis(10));
        tracker.record(Duration::from_millis(20));
        tracker.record(Duration::from_millis(30));
        tracker.record(Duration::from_millis(40));
        tracker.record(Duration::from_millis(50));
        
        // Test percentiles
        assert_eq!(tracker.percentile(50.0), Duration::from_millis(30));
        assert_eq!(tracker.percentile(95.0), Duration::from_millis(50));
        
        // Test average
        assert_eq!(tracker.average(), Duration::from_millis(30));
    }

    #[test]
    fn test_throughput_tracker() {
        let mut tracker = ThroughputTracker::new(Duration::from_secs(1));
        
        // Record some operations
        for _ in 0..5 {
            tracker.record_operation();
        }
        
        // Should show 5 ops/sec
        assert_eq!(tracker.operations_per_second(), 5.0);
    }

    #[test]
    fn test_error_tracker() {
        let mut tracker = ErrorTracker::default();
        
        // Record some operations
        tracker.record_success();
        tracker.record_success();
        tracker.record_error("timeout");
        tracker.record_success();
        
        // Error rate should be 25%
        assert!((tracker.error_rate() - 0.25).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_metrics_collector() {
        let mut collector = MetricsCollector::new();
        
        // Record some operations
        collector.record_operation("test_op", Duration::from_millis(50), true);
        collector.record_operation("test_op", Duration::from_millis(75), true);
        collector.record_operation("test_op", Duration::from_millis(25), false);
        
        let summary = collector.get_summary();
        
        assert_eq!(summary.operation_stats.get("test_op").unwrap().total_count, 3);
        assert!((summary.operation_stats.get("test_op").unwrap().success_rate - 0.6667).abs() < 0.01);
    }
}
