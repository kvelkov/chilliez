//! Monitoring Module
//!
//! This module provides comprehensive monitoring capabilities including:
//! - Enhanced real-time balance monitoring with WebSocket integration
//! - Metrics collection and alerting
//! - Performance monitoring and benchmarking
//! - Health monitoring and diagnostics

pub mod balance_monitor;
pub mod health;
pub mod metrics;
pub mod performance;

pub use balance_monitor::{
    AlertSeverity, AlertType, AtomicBalanceOperations, BalanceAlert, BalanceMonitorConfig,
    BalanceMonitorMetrics, BalanceRecord, EnhancedBalanceMonitor,
};

pub use health::{
    HealthChecker, HealthConfig, HealthMonitor, HealthReport, HealthStatus,
    DatabaseHealthChecker, CacheHealthChecker, NetworkHealthChecker,
    SystemResourcesHealthChecker, DexHealthChecker,
};

pub use metrics::{
    Metrics as LocalMetrics,
};

pub use performance::{
    BenchmarkResults, BenchmarkRunner, PerformanceMetricsCollector, PerformanceConfig,
    PerformanceSummary, StressTestConfig, LatencyTracker, ThroughputTracker,
    ErrorTracker, SystemMetrics, OperationMetrics, OperationStats,
    PerformanceManager, PerformanceReport, ParallelExecutor, CacheManager,
};
