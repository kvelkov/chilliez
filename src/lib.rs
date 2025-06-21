pub mod arbitrage;
pub mod config;
pub mod data; // Moved from src/cache.rs as part of refactor plan
pub mod dex;
pub mod error;
pub mod ffi; // FFI exports for JavaScript bridge
pub mod monitoring; // Enhanced monitoring and alerting
pub mod simulation; // Simulation system (was paper_trading)
pub mod solana;
pub mod testing; // Testing infrastructure
pub mod utils;
pub mod wallet; // NEW: Wallet management features

// Re-export key testing components for easy access
pub use testing::{MarketCondition, MockDexEnvironment, TestSuiteRunner};

// Re-export key monitoring and performance components
pub use monitoring::{
    BenchmarkResults, BenchmarkRunner, HealthMonitor, HealthStatus, PerformanceConfig,
    PerformanceManager, PerformanceMetricsCollector, PerformanceReport, PerformanceSummary,
};
