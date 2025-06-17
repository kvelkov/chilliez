//! Timing utilities for performance monitoring
//! 
//! This module provides utilities for measuring and logging execution times
//! of various bot operations.

use log::{info, debug, warn};
use std::time::{Duration, Instant};
use std::collections::HashMap;

/// A timer for measuring operation durations
#[derive(Debug)]
pub struct Timer {
    start_time: Instant,
    operation_name: String,
    checkpoints: Vec<(String, Instant)>,
}

impl Timer {
    /// Create a new timer for the given operation
    pub fn start(operation_name: &str) -> Self {
        let start_time = Instant::now();
        debug!("⏱️ Starting timer for: {}", operation_name);
        
        Self {
            start_time,
            operation_name: operation_name.to_string(),
            checkpoints: Vec::new(),
        }
    }

    /// Add a checkpoint to track intermediate timing
    pub fn checkpoint(&mut self, checkpoint_name: &str) {
        let now = Instant::now();
        self.checkpoints.push((checkpoint_name.to_string(), now));
        
        let elapsed = now.duration_since(self.start_time);
        debug!("📍 {} - {}: {:.2}ms", 
               self.operation_name, checkpoint_name, elapsed.as_millis());
    }

    /// Finish the timer and log the total duration
    pub fn finish(self) -> Duration {
        let total_duration = self.start_time.elapsed();
        
        // Log checkpoints if any
        if !self.checkpoints.is_empty() {
            info!("🕐 {} completed in {:.2}ms with checkpoints:", 
                  self.operation_name, total_duration.as_millis());
            
            let mut last_time = self.start_time;
            for (name, time) in &self.checkpoints {
                let segment_duration = time.duration_since(last_time);
                info!("   └─ {}: {:.2}ms", name, segment_duration.as_millis());
                last_time = *time;
            }
        } else {
            info!("🕐 {} completed in {:.2}ms", 
                  self.operation_name, total_duration.as_millis());
        }

        total_duration
    }

    /// Finish with a warning if the operation took too long
    pub fn finish_with_threshold(self, threshold_ms: u64) -> Duration {
        let total_duration = self.start_time.elapsed();
        
        // Log checkpoints if any
        if !self.checkpoints.is_empty() {
            info!("🕐 {} completed in {:.2}ms with checkpoints:", 
                  self.operation_name, total_duration.as_millis());
            
            let mut last_time = self.start_time;
            for (name, time) in &self.checkpoints {
                let segment_duration = time.duration_since(last_time);
                info!("   └─ {}: {:.2}ms", name, segment_duration.as_millis());
                last_time = *time;
            }
        } else {
            info!("🕐 {} completed in {:.2}ms", 
                  self.operation_name, total_duration.as_millis());
        }
        
        if total_duration.as_millis() > threshold_ms as u128 {
            warn!("⚠️ {} took {:.2}ms (exceeds threshold of {}ms)", 
                  self.operation_name, total_duration.as_millis(), threshold_ms);
        }
        
        total_duration
    }
}

/// Performance tracker for aggregating timing statistics
#[derive(Debug)]
pub struct PerformanceTracker {
    operation_stats: HashMap<String, OperationStats>,
}

#[derive(Debug)]
struct OperationStats {
    count: u64,
    total_duration: Duration,
    min_duration: Duration,
    max_duration: Duration,
    last_duration: Duration,
}

impl PerformanceTracker {
    pub fn new() -> Self {
        Self {
            operation_stats: HashMap::new(),
        }
    }

    /// Record a completed operation
    pub fn record_operation(&mut self, operation_name: &str, duration: Duration) {
        let stats = self.operation_stats
            .entry(operation_name.to_string())
            .or_insert_with(|| OperationStats {
                count: 0,
                total_duration: Duration::from_millis(0),
                min_duration: duration,
                max_duration: duration,
                last_duration: duration,
            });

        stats.count += 1;
        stats.total_duration += duration;
        stats.last_duration = duration;
        
        if duration < stats.min_duration {
            stats.min_duration = duration;
        }
        if duration > stats.max_duration {
            stats.max_duration = duration;
        }
    }

    /// Get average duration for an operation
    pub fn get_average_duration(&self, operation_name: &str) -> Option<Duration> {
        self.operation_stats.get(operation_name).map(|stats| {
            stats.total_duration / stats.count as u32
        })
    }

    /// Print performance summary
    pub fn print_summary(&self) {
        if self.operation_stats.is_empty() {
            info!("📊 No performance data available");
            return;
        }

        info!("📊 ═══════════════════════════════════════════════════════════════════════════");
        info!("📊 PERFORMANCE SUMMARY");
        info!("📊 ═══════════════════════════════════════════════════════════════════════════");

        for (operation, stats) in &self.operation_stats {
            let avg_ms = (stats.total_duration.as_millis() as f64) / (stats.count as f64);
            info!("📈 {}: ", operation);
            info!("   Count: {} | Avg: {:.2}ms | Min: {:.2}ms | Max: {:.2}ms | Last: {:.2}ms",
                  stats.count,
                  avg_ms,
                  stats.min_duration.as_millis(),
                  stats.max_duration.as_millis(),
                  stats.last_duration.as_millis());
        }
    }

    /// Get operations that are performing slowly
    pub fn get_slow_operations(&self, threshold_ms: u64) -> Vec<String> {
        self.operation_stats
            .iter()
            .filter_map(|(name, stats)| {
                let avg_ms = (stats.total_duration.as_millis() as u64) / stats.count;
                if avg_ms > threshold_ms {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .collect()
    }
}

/// Helper macro for timing operations
#[macro_export]
macro_rules! time_operation {
    ($operation_name:expr, $code:expr) => {{
        let timer = $crate::utils::timing::Timer::start($operation_name);
        let result = $code;
        timer.finish();
        result
    }};
}

/// Helper macro for timing operations with threshold warnings
#[macro_export]
macro_rules! time_operation_with_threshold {
    ($operation_name:expr, $threshold_ms:expr, $code:expr) => {{
        let timer = $crate::utils::timing::Timer::start($operation_name);
        let result = $code;
        timer.finish_with_threshold($threshold_ms);
        result
    }};
}

impl Default for PerformanceTracker {
    fn default() -> Self {
        Self::new()
    }
}
