//! Arbitrage Orchestrator Module
//! 
//! This module provides a modular, maintainable arbitrage orchestrator split into
//! focused components for better organization, testing, and development.

pub mod core;
pub mod detection_engine;
pub mod execution_manager;
pub mod concurrency_manager;

// Re-export the main orchestrator and key types
pub use core::{ArbitrageOrchestrator, OrchestratorStatus, DetectionMetrics};
pub use detection_engine::CacheStats;
pub use execution_manager::{ExecutionStrategy, CompetitivenessAnalysis};
pub use concurrency_manager::{ConcurrencyStatus};

// Trait for price data providers (simplified interface)
pub trait PriceDataProvider: Send + Sync {
    fn get_current_price(&self, symbol: &str) -> Option<f64>;
}
