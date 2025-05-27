use crate::arbitrage::executor::{ArbitrageExecutor, ExecutorEvent};
use tokio::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use solana_sdk::signature::Signature;
use std::time::Instant;
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
        while let Some(raw_event) = self.receiver.recv().await {
            // Transitional/Conversion logic: transform the event if needed.
            let event = self.transform_event(raw_event);
            self.process_event(event).await;
        }
    }

    /// A placeholder transformation function.
    /// Here you can convert or enrich the incoming ExecutorEvent if needed before processing.
    fn transform_event(&self, event: ExecutorEvent) -> ExecutorEvent {
        // Insert any transitional logic or conversion here.
        // For example, you might convert units, filter out certain events, or batch them.
        debug!("Transforming event: {:?}", event);
        event // Currently, it passes through unchanged.
    }

    /// Processes an executor event asynchronously.
    /// Currently, it logs success or failure details for each execution.
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

    /// (Optional) Exposes the sender side of the pipeline in case upstream components
    /// want to push ExecutorEvents directly into the pipeline.
    pub fn get_sender(&self) -> Sender<ExecutorEvent> {
        self.sender.clone()
    }

    /// As an example of how to dispatch an execution request, we include a stub method.
    /// It calls the executor's method to execute an opportunity by its ID.
    /// This method shows how to initiate an execution via the pipeline.
    pub async fn dispatch_execution(
        &self,
        opportunity_id: String,
        executor: Arc<ArbitrageExecutor>,
    ) {
        if let Err(e) = executor.execute_opportunity_by_id(opportunity_id.clone()).await {
            error!(
                "[Dispatch Failure] Could not execute trade {}: {:?}",
                opportunity_id, e
            );
        }
    }
}
