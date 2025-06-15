//! Monitoring Module
//! 
//! This module provides comprehensive monitoring capabilities including:
//! - Enhanced real-time balance monitoring with WebSocket integration
//! - Metrics collection and alerting
//! - Health monitoring and diagnostics

pub mod balance_monitor_enhanced;

pub use balance_monitor_enhanced::{
    EnhancedBalanceMonitor,
    BalanceMonitorConfig,
    BalanceRecord,
    BalanceAlert,
    AlertType,
    AlertSeverity,
    BalanceMonitorMetrics,
    AtomicBalanceOperations,
};
