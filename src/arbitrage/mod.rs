//! Arbitrage Module
//! 
//! This module contains all arbitrage-related functionality organized in a modular structure:
//! - orchestrator: Modular central control and coordination (core, detection, execution, concurrency)
//! - strategy: Opportunity detection and path finding
//! - execution: Trade execution (both HFT and batch)
//! - analysis: Mathematical analysis, fees, and thresholds
//! - mev: MEV protection and Jito integration
//! - safety: Transaction safety, retry logic, and recovery

use crate::local_metrics::Metrics;
use std::sync::Arc;
use tokio::sync::{mpsc::{self, Receiver, Sender}, Mutex};
use log::{info, error};

// =============================================================================
// Module Declarations
// =============================================================================

// Core modules
pub mod opportunity;
pub mod tests;
pub mod calculator_tests;

// Modular orchestrator (replaces the monolithic orchestrator.rs)
pub mod orchestrator;     // Modular central controller with focused components

// Specialized modules
pub mod strategy;         // Opportunity detection and path finding
pub mod execution;        // All execution logic (HFT + batch)
pub mod analysis;         // Mathematical analysis, fees, thresholds
pub mod mev;              // MEV protection and Jito integration
pub mod safety;           // Transaction safety, retry logic, and recovery

// New modular architecture (refactored from orchestrator.rs) - temporarily disabled
// pub mod types;            // Common types and enums
// pub mod market_data;      // Market data and price feeds
// pub mod execution_manager; // Execution coordination
// pub mod strategy_manager; // Strategy coordination
// pub mod orchestrator_new; // New simplified orchestrator

// =============================================================================
// Public Re-exports (New Flat Structure)
// =============================================================================

// Primary exports from new consolidated modules
pub use self::orchestrator::ArbitrageOrchestrator;
pub use self::strategy::ArbitrageStrategy;
pub use self::analysis::ArbitragePath;
pub use self::execution::{
    HftExecutor, BatchExecutor, ExecutorEvent,
    BatchExecutionConfig, OpportunityBatch, SimulationResult, 
    JitoBundle, BundleExecutionResult, ExecutionMetrics
};
pub use self::analysis::{
    ArbitrageAnalyzer, OpportunityCalculationResult, OptimalArbitrageResult,
    VolatilityTracker, DynamicThresholdUpdater, FeeBreakdown, SlippageModel, XYKSlippageModel,
    OptimalInputResult, SimulationResult as AnalysisSimulationResult, ContractSelector, ExecutionStrategy
};

// New modular components (temporarily disabled)
// pub use self::types::{ExecutionStrategy as TypesExecutionStrategy, CompetitivenessAnalysis, ExecutionRecommendation, DetectionMetrics};
// pub use self::market_data::{MarketDataManager, PriceDataProvider};
// pub use self::execution_manager::ExecutionManager;
// pub use self::strategy_manager::StrategyManager;
pub use self::mev::{
    JitoHandler, MevProtectionConfig, JitoConfig, GasOptimizationMetrics, NetworkConditions,
    MevProtectionStrategy, MevProtectionStatus, JitoBundleResult
};
pub use self::safety::{
    SafeTransactionHandler, TransactionSafetyConfig, TransactionResult, SafetyViolation,
    RetryPolicy, BalanceValidationConfig, SlippageProtectionConfig, MevProtectionConfig as SafetyMevConfig,
    ConfirmationConfig, FailureRecoveryStrategy, SafetyConfig, TransactionRecord
};
pub use self::opportunity::{ArbHop, MultiHopArbOpportunity, AdvancedMultiHopOpportunity, EnhancedArbHop};

// Backward compatibility aliases
pub use self::strategy::ArbitrageStrategy as ArbitrageDetector;
pub use self::execution::HftExecutor as ArbitrageExecutor;
pub use self::execution::BatchExecutor as BatchExecutionEngine;

// =============================================================================
// Trade Coordination System
// =============================================================================

/// TradeInstruction is used to convey a new trade that must be executed.
/// It carries all the metadata required (price, quantity, pool info, fees, slippage, etc.)
/// to enable the executor to perform the trade.
pub enum TradeInstruction {
    ExecuteOpportunity(MultiHopArbOpportunity),
}

/// The ArbitrageCoordinator acts as an event-driven nexus between the upstream logic (Engine/Detector)
/// and the on-chain trade execution (via the Executor). It listens for incoming trade instructions,
/// dispatches them immediately to the Executor, and can record the execution results in Metrics.
pub struct ArbitrageCoordinator {
    executor: Arc<HftExecutor>, // Updated to use new HftExecutor
    metrics: Arc<Mutex<Metrics>>,
    instruction_rx: Receiver<TradeInstruction>,
    instruction_tx: Sender<TradeInstruction>,
}

impl ArbitrageCoordinator {
    /// Constructs a new coordinator with the given Executor and Metrics.
    /// It establishes an internal MPSC channel (with a capacity of 100) for trade instructions.
    pub fn new(executor: Arc<HftExecutor>, metrics: Arc<Mutex<Metrics>>) -> Self {
        let (instruction_tx, instruction_rx) = mpsc::channel(100);
        Self {
            executor,
            metrics,
            instruction_rx,
            instruction_tx,
        }
    }

    /// Returns a cloneable Sender which upstream modules—such as the Engine—can use
    /// to push trade instructions immediately when an opportunity is validated.
    pub fn get_instruction_sender(&self) -> Sender<TradeInstruction> {
        self.instruction_tx.clone()
    }

    /// The primary run loop: it continuously listens (non-blockingly) for new trade instructions.
    /// For each received instruction, it dispatches the execution call to the Executor.
    /// Execution events (successes or failures) are logged and can be further recorded in Metrics.
    pub async fn run(&mut self) {
        // Main loop: process incoming trade instructions immediately.
        while let Some(instruction) = self.instruction_rx.recv().await {
            match instruction {
                TradeInstruction::ExecuteOpportunity(opp) => {
                    info!("Received instruction to execute opportunity: {}", opp.id);
                    // Optionally push details to metrics here.
                    match self.executor.execute_opportunity(&opp).await {
                        Ok(signature) => {
                            info!(
                                "Successfully executed opportunity {} with signature {:?}",
                                opp.id, signature
                            );
                            // Refined metrics update for successful trade
                            let metrics_guard = self.metrics.lock().await;
                            // metrics_guard.log_opportunity_executed_success();
                            if let Some(profit_usd) = opp.estimated_profit_usd {
                                metrics_guard.add_to_total_profit(profit_usd);
                            }
                            metrics_guard.log_opportunity_executed_success();
                            // metrics_guard.last_successful_trade_timestamp = Some(chrono::Utc::now());
                        }
                        Err(err) => {
                            error!(
                                "Execution failed for opportunity {}: {:?}",
                                opp.id, err
                            );
                            // self.metrics.lock().await.log_opportunity_executed_failure();
                        }
                    }
                }
            }
        }
    }
}