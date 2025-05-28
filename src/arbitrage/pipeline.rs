use crate::arbitrage::executor::{ArbitrageExecutor, ExecutorEvent};
use tokio::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use solana_sdk::signature::Signature;
use log::{info, error, debug};

/// ExecutionPipeline is responsible for forwarding execution events from the Executor
/// to any external monitoring or logging components. Additionally, it provides a hook
/// (via `transform_event`) where you can insert transitional logic or conversion if necessary.
pub struct ExecutionPipeline {
    sender: Sender<ExecutorEvent>,
    pub receiver: Receiver<ExecutorEvent>,
}

impl ExecutionPipeline {
    /// Initializes the pipeline with asynchronous message passing.
    /// Here we use a robust channel (buffer size 500) to support high-throughput.
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel(500);
        Self { sender, receiver }
    }

    /// Starts listening for executor events and processes them continuously.
    /// This function runs as an asynchronous background task.
    pub async fn start_listener(&mut self) {
        while let Some(event) = self.receiver.recv().await {
            // Directly process the event, no transform step needed.
            self.process_event(event).await;
        }
    }

    /// Processes an executor event asynchronously.
    async fn process_event(&self, event: ExecutorEvent) {
        match event {
            ExecutorEvent::OpportunityExecuted {
                opportunity_id,
                signature,
                timestamp,
                result,
            } => match result {
                Ok(_) => info!(
                    "[Execution Success] Trade ID: {} | Signature: {} | Time: {:?}",
                    opportunity_id,
                    signature.unwrap_or(Signature::default()),
                    timestamp
                ),
                Err(e) => error!(
                    "[Execution Failed] Trade ID: {} | Error: {:?} | Time: {:?}",
                    opportunity_id, e, timestamp
                ),
            },
        }
    }

    /// Exposes the sender side of the pipeline for upstream components.
    pub fn get_sender(&self) -> Sender<ExecutorEvent> {
        self.sender.clone()
    }
}
