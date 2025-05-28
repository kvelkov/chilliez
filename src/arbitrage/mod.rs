use self::executor::ArbitrageExecutor;
use self::opportunity::MultiHopArbOpportunity;
use crate::metrics::Metrics; // This import is fine
use std::sync::Arc;
use tokio::sync::{mpsc::{self, Receiver, Sender}, Mutex}; // Added Mutex here
use tokio::spawn;
use log::{info, error, debug};
pub mod engine;
pub mod calculator;
pub mod pipeline;
pub mod fee_manager;
pub mod executor;
pub mod opportunity;
pub mod dynamic_threshold;
pub mod detector;
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
    executor: Arc<ArbitrageExecutor>,
    metrics: Arc<Mutex<Metrics>>, // Changed type here
    instruction_rx: Receiver<TradeInstruction>,
    instruction_tx: Sender<TradeInstruction>,
}

impl ArbitrageCoordinator {
    /// Constructs a new coordinator with the given Executor and Metrics.
    /// It establishes an internal MPSC channel (with a capacity of 100) for trade instructions.
    pub fn new(executor: Arc<ArbitrageExecutor>, metrics: Arc<Mutex<Metrics>>) -> Self { // Changed parameter type
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
                            self.metrics.lock().await.log_opportunity_executed_success();
                            // Add/update this method in Metrics if missing, or use a generic metric/log call
                            // self.metrics.lock().await.update_profit(opp.total_profit);
                            // If update_profit does not exist, use increment_main_cycles or another suitable method:
                            self.metrics.lock().await.increment_main_cycles();
                        }
                        Err(err) => {
                            error!(
                                "Execution failed for opportunity {}: {:?}",
                                opp.id, err
                            );
                            self.metrics.lock().await.log_opportunity_executed_failure();
                        }
                    }
                }
            }
        }
    }
}
