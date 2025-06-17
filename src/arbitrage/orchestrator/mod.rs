//! Arbitrage Orchestrator Module
//!
//! This module provides a modular, maintainable arbitrage orchestrator split into
//! focused components for better organization, testing, and development.

pub mod concurrency_manager;
pub mod core;
pub mod detection_engine;
pub mod execution_manager;

// Re-export the main orchestrator and key types
pub use concurrency_manager::ConcurrencyStatus;
pub use core::{ArbitrageOrchestrator, DetectionMetrics, OrchestratorStatus};
pub use detection_engine::CacheStats;
pub use execution_manager::{CompetitivenessAnalysis, ExecutionStrategy};

// Trait for price data providers (simplified interface)
pub trait PriceDataProvider: Send + Sync {
    fn get_current_price(&self, symbol: &str) -> Option<f64>;
}
