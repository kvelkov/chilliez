use tokio::sync::mpsc;
type EventSender = mpsc::Sender<ExecutorEvent>;
use crate::arbitrage::opportunity::MultiHopArbOpportunity;
use tokio::sync::Mutex;
use crate::metrics::Metrics;

use solana_client::nonblocking::rpc_client::RpcClient as NonBlockingRpcClient;
use solana_sdk::{ // ComputeBudgetInstruction was unused, now it will be used in the stub
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::Instruction,
    signature::{Keypair, Signature, Signer},
    transaction::Transaction,
};

use std::{sync::Arc, time::Instant};

#[derive(Clone)]
pub struct ArbitrageExecutor {
    wallet: Arc<Keypair>,
    rpc_client: Arc<NonBlockingRpcClient>,
    event_sender: Option<EventSender>, // Add this
    metrics: Arc<Mutex<Metrics>>,      // Add this
}

#[derive(Debug)] // Add this line
pub enum ExecutorEvent {
    OpportunityExecuted {
        opportunity_id: String,
        signature: Option<Signature>,
        timestamp: std::time::SystemTime,
        result: Result<(), String>, // Ok(()) for success, Err(reason) for failure
    },
    // Potentially other event types in the future
}





impl ArbitrageExecutor {
    pub fn new(
        wallet: Arc<Keypair>, 
        rpc_client: Arc<NonBlockingRpcClient>, 
        event_sender: Option<EventSender>, 
        metrics: Arc<Mutex<Metrics>>
    ) -> Self {
        Self { wallet, rpc_client, event_sender, metrics }
    }

    pub async fn execute_opportunity(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<Signature, String> {
        let start_time = Instant::now();
        let instructions = self.build_instructions_from_multihop(opportunity)?;

        if instructions.is_empty() {
            return Err("No swap instructions generated".to_string());
        }

        let recent_blockhash = self.get_latest_blockhash().await?;
        let all_instructions: Vec<Instruction> = [
            ComputeBudgetInstruction::set_compute_unit_limit(400_000), // Example: Set CU limit
            ComputeBudgetInstruction::set_compute_unit_price(10_000),   // Example: Set priority fee per CU
        ].into_iter().chain(instructions.into_iter()).collect();

        let transaction = Transaction::new_signed_with_payer(
            &all_instructions,
            Some(&self.wallet.pubkey()),
            &[&*self.wallet],
            recent_blockhash,
        );

        match self
            .rpc_client
            .send_and_confirm_transaction_with_spinner(&transaction)
            .await
        {
            Ok(signature) => {
                let duration = start_time.elapsed();
                self.metrics.lock().await.record_execution_time(duration);
                self.metrics.lock().await.log_opportunity_executed_success();
                if let Some(sender) = &self.event_sender {
                    let event = ExecutorEvent::OpportunityExecuted {
                        opportunity_id: opportunity.id.clone(),
                        signature: Some(signature),
                        timestamp: std::time::SystemTime::now(),
                        result: Ok(()),
                    };
                    if let Err(e) = sender.send(event).await {
                        log::error!("Failed to send execution success event: {}", e);
                    }
                }
                Ok(signature)
            }
            Err(e) => {
                self.metrics.lock().await.log_opportunity_executed_failure();
                // Potentially send failure event via self.event_sender as well
                Err(format!("Transaction failed: {}", e))
            }
        }
    }

    fn build_instructions_from_multihop(
        &self,
        _opportunity: &MultiHopArbOpportunity,
    ) -> Result<Vec<Instruction>, String> {
        // Multi-pool execution logic would be handled here
        // Example of how ComputeBudgetInstruction might be added if not done in execute_opportunity:
        // let mut instructions = vec![
        //     ComputeBudgetInstruction::set_compute_unit_limit(desired_limit),
        //     ComputeBudgetInstruction::set_compute_unit_price(desired_price),
        // ];
        Ok(vec![])
    }

    async fn get_latest_blockhash(&self) -> Result<Hash, String> {
        self.rpc_client
            .get_latest_blockhash()
            .await
            .map_err(|e| format!("Failed to fetch latest blockhash: {}", e))
    }
}
