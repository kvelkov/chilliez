//! Unified Execution Module
//! 
//! This module consolidates all execution-related functionality:
//! - HftExecutor: High-frequency individual trade execution
//! - BatchExecutor: Batch execution with Jito bundle support
//! - ExecutionMetrics and event handling
//! 
//! Combines logic from executor.rs and execution_engine.rs

use crate::{
    arbitrage::{
        opportunity::MultiHopArbOpportunity,
        mev::{JitoHandler, MevProtectionConfig, JitoConfig},
    },
    config::settings::Config,
    error::ArbError,
    metrics::Metrics,
    solana::rpc::SolanaRpcClient,
};
use log::{info, warn, error};
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient as NonBlockingRpcClient;
use uuid;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::Instruction,
    signature::{Keypair, Signature, Signer},
    transaction::Transaction,
};
use std::{
    collections::HashMap,
    sync::Arc,
    time::Instant,
};
use tokio::sync::{mpsc, Mutex, RwLock};

// =============================================================================
// Event System and Core Types
// =============================================================================

type EventSender = mpsc::Sender<ExecutorEvent>;

#[derive(Debug)]
pub enum ExecutorEvent {
    OpportunityExecuted {
        opportunity_id: String,
        signature: Option<Signature>,
        timestamp: std::time::SystemTime,
        result: Result<(), String>,
    },
    BatchExecuted {
        batch_id: String,
        bundle_result: BundleExecutionResult,
        timestamp: std::time::SystemTime,
    },
}

// =============================================================================
// High-Frequency Executor (formerly ArbitrageExecutor)
// =============================================================================

#[derive(Clone)]
pub struct HftExecutor {
    wallet: Arc<Keypair>,
    rpc_client: Arc<NonBlockingRpcClient>,
    event_sender: Option<EventSender>,
    config: Arc<Config>,
    metrics: Arc<Mutex<Metrics>>,
}

impl HftExecutor {
    pub fn new(
        wallet: Arc<Keypair>, 
        rpc_client: Arc<NonBlockingRpcClient>, 
        event_sender: Option<EventSender>,
        config: Arc<Config>, 
        metrics: Arc<Mutex<Metrics>>
    ) -> Self {
        Self { wallet, rpc_client, event_sender, config, metrics }
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

        // Get CU limit and price from config, with defaults
        let cu_limit = self.config.transaction_cu_limit.unwrap_or(400_000); 
        let cu_price = self.config.transaction_priority_fee_lamports;

        let recent_blockhash = self.get_latest_blockhash().await?;
        let all_instructions: Vec<Instruction> = [
            ComputeBudgetInstruction::set_compute_unit_limit(cu_limit),
            ComputeBudgetInstruction::set_compute_unit_price(cu_price),
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
                if let Some(sender) = &self.event_sender {
                    let event = ExecutorEvent::OpportunityExecuted {
                        opportunity_id: opportunity.id.clone(),
                        signature: None,
                        timestamp: std::time::SystemTime::now(),
                        result: Err(format!("Transaction failed: {}", e)),
                    };
                    if let Err(send_err) = sender.send(event).await {
                        log::error!("Failed to send execution failure event: {}", send_err);
                    }
                }
                Err(format!("Transaction failed: {}", e))
            }
        }
    }

    fn build_instructions_from_multihop(
        &self,
        _opportunity: &MultiHopArbOpportunity,
    ) -> Result<Vec<Instruction>, String> {
        // Multi-pool execution logic would be handled here
        Ok(vec![])
    }

    async fn get_latest_blockhash(&self) -> Result<Hash, String> {
        self.rpc_client
            .get_latest_blockhash()
            .await
            .map_err(|e| format!("Failed to fetch latest blockhash: {}", e))
    }
}

// =============================================================================
// Batch Execution Configuration and Types
// =============================================================================

/// Configuration for the batch execution engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchExecutionConfig {
    /// Maximum number of opportunities per batch
    pub max_opportunities_per_batch: usize,
    /// Maximum compute units per transaction
    pub max_compute_units_per_tx: u32,
    /// Maximum total compute units per bundle
    pub max_compute_units_per_bundle: u32,
    /// Minimum profit threshold for batching (USD)
    pub min_batch_profit_usd: f64,
    /// Maximum batch assembly time (ms)
    pub max_batch_assembly_time_ms: u64,
    /// Enable parallel simulation
    pub enable_parallel_simulation: bool,
    /// Maximum simulation time per opportunity (ms)
    pub max_simulation_time_ms: u64,
    /// Jito tip amount (lamports)
    pub jito_tip_lamports: u64,
    /// Enable MEV protection
    pub enable_mev_protection: bool,
    /// Bundle submission timeout (ms)
    pub bundle_submission_timeout_ms: u64,
}

impl Default for BatchExecutionConfig {
    fn default() -> Self {
        Self {
            max_opportunities_per_batch: 5,
            max_compute_units_per_tx: 1_400_000,
            max_compute_units_per_bundle: 7_000_000,
            min_batch_profit_usd: 10.0,
            max_batch_assembly_time_ms: 100,
            enable_parallel_simulation: true,
            max_simulation_time_ms: 50,
            jito_tip_lamports: 10_000,
            enable_mev_protection: true,
            bundle_submission_timeout_ms: 1000,
        }
    }
}

/// Represents a batch of arbitrage opportunities to be executed together
#[derive(Debug, Clone)]
pub struct OpportunityBatch {
    pub id: String,
    pub opportunities: Vec<MultiHopArbOpportunity>,
    pub estimated_total_profit_usd: f64,
    pub estimated_total_compute_units: u32,
    pub created_at: Instant,
    pub priority_score: f64,
}

/// Result of simulating an opportunity
#[derive(Debug, Clone)]
pub struct SimulationResult {
    pub opportunity_id: String,
    pub success: bool,
    pub estimated_profit_usd: f64,
    pub estimated_compute_units: u32,
    pub simulation_time_ms: u64,
    pub error_message: Option<String>,
}

/// Represents a Jito bundle ready for submission
#[derive(Debug, Clone)]
pub struct JitoBundle {
    pub id: String,
    pub transactions: Vec<Transaction>,
    pub tip_amount_lamports: u64,
    pub estimated_total_profit_usd: f64,
    pub created_at: Instant,
}

/// Result of executing a bundle
#[derive(Debug, Clone)]
pub struct BundleExecutionResult {
    pub bundle_id: String,
    pub success: bool,
    pub signatures: Vec<Option<Signature>>,
    pub execution_time_ms: u64,
    pub actual_profit_usd: Option<f64>,
    pub error_message: Option<String>,
}

/// Metrics for tracking execution performance
#[derive(Debug, Default)]
pub struct ExecutionMetrics {
    pub total_batches_created: u64,
    pub total_batches_executed: u64,
    pub total_successful_batches: u64,
    pub total_failed_batches: u64,
    pub avg_batch_assembly_time_ms: f64,
    pub avg_batch_execution_time_ms: f64,
    pub total_profit_captured_usd: f64,
    pub total_opportunities_processed: u64,
}

// =============================================================================
// Batch Executor
// =============================================================================

pub struct BatchExecutor {
    config: BatchExecutionConfig,
    solana_client: Arc<SolanaRpcClient>,
    wallet: Arc<Keypair>,
    #[allow(dead_code)]
    mev_handler: Option<Arc<JitoHandler>>,
    metrics: Arc<RwLock<ExecutionMetrics>>,
    active_batches: Arc<RwLock<HashMap<String, OpportunityBatch>>>,
    pending_opportunities: Arc<RwLock<Vec<MultiHopArbOpportunity>>>,
    event_sender: Option<EventSender>,
}

impl BatchExecutor {
    pub fn new(
        config: BatchExecutionConfig,
        solana_client: Arc<SolanaRpcClient>,
        wallet: Arc<Keypair>,
        event_sender: Option<EventSender>,
    ) -> Self {
        let mev_handler = if config.enable_mev_protection {
            let mev_config = MevProtectionConfig::default();
            let jito_config = JitoConfig::default();
            Some(Arc::new(JitoHandler::new(mev_config, jito_config)))
        } else {
            None
        };

        Self {
            config,
            solana_client,
            wallet,
            mev_handler,
            metrics: Arc::new(RwLock::new(ExecutionMetrics::default())),
            active_batches: Arc::new(RwLock::new(HashMap::new())),
            pending_opportunities: Arc::new(RwLock::new(Vec::new())),
            event_sender,
        }
    }

    /// Process a new arbitrage opportunity for batch execution
    pub async fn process_opportunity(&self, opportunity: MultiHopArbOpportunity) -> Result<(), ArbError> {
        info!("Processing opportunity {} for batch execution", opportunity.id);
        
        // Add to pending opportunities
        {
            let mut pending = self.pending_opportunities.write().await;
            pending.push(opportunity);
        }

        // Try to create batches from pending opportunities
        self.try_create_batches().await?;
        
        Ok(())
    }

    /// Attempt to create executable batches from pending opportunities
    async fn try_create_batches(&self) -> Result<(), ArbError> {
        let batch_start = Instant::now();
        let pending_opportunities = {
            let mut pending = self.pending_opportunities.write().await;
            std::mem::take(&mut *pending)
        };

        if pending_opportunities.is_empty() {
            return Ok(());
        }

        info!("Attempting to create batches from {} pending opportunities", pending_opportunities.len());

        // Group opportunities into compatible batches
        let batches = self.create_compatible_batches(pending_opportunities).await?;
        
        for batch in batches {
            if batch.opportunities.len() > 1 {
                info!("Created batch {} with {} opportunities (${:.2} profit)", 
                      batch.id, batch.opportunities.len(), batch.estimated_total_profit_usd);
                self.execute_batch(batch).await?;
            } else if let Some(single_opp) = batch.opportunities.into_iter().next() {
                // Single opportunity - add back to pending or execute immediately
                self.pending_opportunities.write().await.push(single_opp);
            }
        }

        let batch_time = batch_start.elapsed().as_millis() as u64;
        self.update_batch_assembly_metrics(batch_time).await;

        Ok(())
    }

    /// Create compatible batches from opportunities
    async fn create_compatible_batches(&self, opportunities: Vec<MultiHopArbOpportunity>) -> Result<Vec<OpportunityBatch>, ArbError> {
        let mut batches = Vec::new();
        let mut remaining_opportunities = opportunities;

        while !remaining_opportunities.is_empty() {
            let mut current_batch_opps = vec![remaining_opportunities.remove(0)];
            let mut current_batch_profit = current_batch_opps[0].estimated_profit_usd.unwrap_or(0.0);
            let mut current_batch_compute_units = self.estimate_compute_units(&current_batch_opps[0]);

            // Try to add compatible opportunities to current batch
            let mut indices_to_remove = Vec::new();
            for (idx, candidate) in remaining_opportunities.iter().enumerate() {
                if current_batch_opps.len() >= self.config.max_opportunities_per_batch {
                    break;
                }
                
                let candidate_compute_units = self.estimate_compute_units(candidate);

                // Check compatibility and constraints
                if self.are_opportunities_compatible(&current_batch_opps, candidate) &&
                   current_batch_compute_units + candidate_compute_units <= self.config.max_compute_units_per_bundle {
                    
                    current_batch_profit += candidate.estimated_profit_usd.unwrap_or(0.0);
                    current_batch_compute_units += candidate_compute_units;
                    indices_to_remove.push(idx);
                }
            }

            // Remove selected opportunities from remaining (in reverse order to maintain indices)
            let mut selected_opps = Vec::new();
            for &idx in indices_to_remove.iter().rev() {
                selected_opps.push(remaining_opportunities.remove(idx));
            }
            current_batch_opps.extend(selected_opps);

            // Create batch if profitable enough
            if current_batch_profit >= self.config.min_batch_profit_usd {
                let batch_size = current_batch_opps.len();
                let batch = OpportunityBatch {
                    id: uuid::Uuid::new_v4().to_string(),
                    opportunities: current_batch_opps,
                    estimated_total_profit_usd: current_batch_profit,
                    estimated_total_compute_units: current_batch_compute_units,
                    created_at: Instant::now(),
                    priority_score: current_batch_profit / (batch_size as f64),
                };
                batches.push(batch);
            } else {
                // Batch not profitable enough, add opportunities back
                remaining_opportunities.extend(current_batch_opps);
            }
        }

        Ok(batches)
    }

    /// Check if two opportunities are compatible for batching
    fn are_opportunities_compatible(&self, batch_opps: &[MultiHopArbOpportunity], candidate: &MultiHopArbOpportunity) -> bool {
        // Check for token conflicts (simplified)
        for existing in batch_opps {
            if self.have_token_conflicts(existing, candidate) {
                return false;
            }
        }
        true
    }

    /// Check if two opportunities have conflicting token usage
    fn have_token_conflicts(&self, opp1: &MultiHopArbOpportunity, opp2: &MultiHopArbOpportunity) -> bool {
        // Simplified conflict detection - check if they use the same input token
        opp1.input_token == opp2.input_token
    }

    /// Estimate compute units for an opportunity
    fn estimate_compute_units(&self, _opportunity: &MultiHopArbOpportunity) -> u32 {
        // Simplified estimation
        300_000 // Base estimate per opportunity
    }

    /// Execute a batch of opportunities
    async fn execute_batch(&self, batch: OpportunityBatch) -> Result<BundleExecutionResult, ArbError> {
        let start_time = Instant::now();
        info!("Executing batch {} with {} opportunities", batch.id, batch.opportunities.len());

        // Simulate all opportunities in parallel if enabled
        if self.config.enable_parallel_simulation {
            if let Err(e) = self.simulate_batch_parallel(&batch).await {
                warn!("Batch simulation failed: {}", e);
                return Err(e);
            }
        }

        // Create Jito bundle
        let jito_bundle = self.create_jito_bundle(&batch).await?;
        
        // Submit bundle
        let result = self.submit_jito_bundle(jito_bundle).await?;
        
        // Update metrics
        let execution_time = start_time.elapsed().as_millis() as u64;
        self.update_execution_metrics(&result, execution_time).await;

        // Send event if configured
        if let Some(sender) = &self.event_sender {
            let event = ExecutorEvent::BatchExecuted {
                batch_id: batch.id.clone(),
                bundle_result: result.clone(),
                timestamp: std::time::SystemTime::now(),
            };
            if let Err(e) = sender.send(event).await {
                error!("Failed to send batch execution event: {}", e);
            }
        }

        Ok(result)
    }

    /// Simulate batch opportunities in parallel
    async fn simulate_batch_parallel(&self, batch: &OpportunityBatch) -> Result<Vec<SimulationResult>, ArbError> {
        info!("Running parallel simulation for batch {}", batch.id);
        
        let mut simulation_tasks = Vec::new();
        
        for opportunity in &batch.opportunities {
            let opp_clone = opportunity.clone();
            
            let task = tokio::spawn(async move {
                let start_time = Instant::now();
                
                // Simulate the opportunity
                // In a real implementation, this would call the appropriate DEX client
                let success = true; // Placeholder
                let simulation_time = start_time.elapsed().as_millis() as u64;
                
                SimulationResult {
                    opportunity_id: opp_clone.id,
                    success,
                    estimated_profit_usd: opp_clone.estimated_profit_usd.unwrap_or(0.0),
                    estimated_compute_units: 300_000, // Estimate
                    simulation_time_ms: simulation_time,
                    error_message: if success { None } else { Some("Simulation failed".to_string()) },
                }
            });
            
            simulation_tasks.push(task);
        }

        // Wait for all simulations to complete
        let simulation_results = futures::future::try_join_all(simulation_tasks)
            .await
            .map_err(|e| ArbError::ExecutionError(format!("Simulation task failed: {}", e)))?;

        // Check if all simulations succeeded
        for result in &simulation_results {
            if !result.success {
                return Err(ArbError::ExecutionError(
                    format!("Simulation failed for opportunity {}: {}", 
                            result.opportunity_id, 
                            result.error_message.as_deref().unwrap_or("Unknown error"))
                ));
            }
        }

        info!("All {} simulations succeeded for batch {}", simulation_results.len(), batch.id);
        Ok(simulation_results)
    }

    /// Create a Jito bundle from a batch
    async fn create_jito_bundle(&self, batch: &OpportunityBatch) -> Result<JitoBundle, ArbError> {
        info!("Creating Jito bundle for batch {}", batch.id);
        
        let mut transactions = Vec::new();
        
        for opportunity in &batch.opportunities {
            // Build transaction for this opportunity
            let transaction = self.build_opportunity_transaction(opportunity).await?;
            transactions.push(transaction);
        }

        Ok(JitoBundle {
            id: batch.id.clone(),
            transactions,
            tip_amount_lamports: self.config.jito_tip_lamports,
            estimated_total_profit_usd: batch.estimated_total_profit_usd,
            created_at: Instant::now(),
        })
    }

    /// Build a transaction for a single opportunity
    async fn build_opportunity_transaction(&self, _opportunity: &MultiHopArbOpportunity) -> Result<Transaction, ArbError> {
        // Placeholder implementation
        // In real implementation, this would:
        // 1. Build swap instructions for the opportunity
        // 2. Add compute budget instructions
        // 3. Create and sign the transaction
        
        let instructions = vec![]; // Placeholder
        let recent_blockhash = self.solana_client.primary_client.get_latest_blockhash().await
            .map_err(|e| ArbError::ExecutionError(format!("Failed to get blockhash: {}", e)))?;
        
        let transaction = Transaction::new_signed_with_payer(
            &instructions,
            Some(&self.wallet.pubkey()),
            &[&*self.wallet],
            recent_blockhash,
        );

        Ok(transaction)
    }

    /// Submit a Jito bundle
    async fn submit_jito_bundle(&self, bundle: JitoBundle) -> Result<BundleExecutionResult, ArbError> {
        info!("Submitting Jito bundle {} with {} transactions", bundle.id, bundle.transactions.len());
        
        let start_time = Instant::now();
        
        // In real implementation, this would:
        // 1. Submit the bundle to Jito
        // 2. Wait for confirmation
        // 3. Return the result
        
        // Placeholder implementation
        let success = true;
        let signatures = vec![None; bundle.transactions.len()]; // Placeholder
        let execution_time = start_time.elapsed().as_millis() as u64;
        
        Ok(BundleExecutionResult {
            bundle_id: bundle.id,
            success,
            signatures,
            execution_time_ms: execution_time,
            actual_profit_usd: if success { Some(bundle.estimated_total_profit_usd) } else { None },
            error_message: if success { None } else { Some("Bundle execution failed".to_string()) },
        })
    }

    /// Update batch assembly metrics
    async fn update_batch_assembly_metrics(&self, assembly_time_ms: u64) {
        let mut metrics = self.metrics.write().await;
        metrics.total_batches_created += 1;
        metrics.avg_batch_assembly_time_ms = 
            (metrics.avg_batch_assembly_time_ms * (metrics.total_batches_created - 1) as f64 + assembly_time_ms as f64) 
            / metrics.total_batches_created as f64;
    }

    /// Update execution metrics
    async fn update_execution_metrics(&self, result: &BundleExecutionResult, execution_time_ms: u64) {
        let mut metrics = self.metrics.write().await;
        metrics.total_batches_executed += 1;
        
        if result.success {
            metrics.total_successful_batches += 1;
            if let Some(profit) = result.actual_profit_usd {
                metrics.total_profit_captured_usd += profit;
            }
        } else {
            metrics.total_failed_batches += 1;
        }
        
        metrics.avg_batch_execution_time_ms = 
            (metrics.avg_batch_execution_time_ms * (metrics.total_batches_executed - 1) as f64 + execution_time_ms as f64) 
            / metrics.total_batches_executed as f64;
    }

    /// Get current status of the batch execution engine
    pub async fn get_status(&self) -> BatchEngineStatus {
        let metrics = self.metrics.read().await;
        let active_batches_count = self.active_batches.read().await.len();
        let pending_opportunities_count = self.pending_opportunities.read().await.len();

        BatchEngineStatus {
            active_batches_count,
            pending_opportunities_count,
            total_batches_created: metrics.total_batches_created,
            total_batches_executed: metrics.total_batches_executed,
            success_rate: if metrics.total_batches_executed > 0 {
                metrics.total_successful_batches as f64 / metrics.total_batches_executed as f64
            } else {
                0.0
            },
            avg_assembly_time_ms: metrics.avg_batch_assembly_time_ms,
            avg_execution_time_ms: metrics.avg_batch_execution_time_ms,
            total_profit_usd: metrics.total_profit_captured_usd,
        }
    }

    /// Force execution of all pending opportunities
    pub async fn force_execute_pending(&self) -> Result<Vec<BundleExecutionResult>, ArbError> {
        info!("Force executing all pending opportunities");
        
        let pending_opportunities = {
            let mut pending = self.pending_opportunities.write().await;
            std::mem::take(&mut *pending)
        };

        let mut results = Vec::new();
        
        // Create batches without strict profitability requirements
        for chunk in pending_opportunities.chunks(self.config.max_opportunities_per_batch) {
            let batch = OpportunityBatch {
                id: uuid::Uuid::new_v4().to_string(),
                opportunities: chunk.to_vec(),
                estimated_total_profit_usd: chunk.iter().map(|o| o.estimated_profit_usd.unwrap_or(0.0)).sum(),
                estimated_total_compute_units: chunk.len() as u32 * 300_000,
                created_at: Instant::now(),
                priority_score: 1.0,
            };
            
            let result = self.execute_batch(batch).await?;
            results.push(result);
        }

        Ok(results)
    }
}

// =============================================================================
// Status and Monitoring
// =============================================================================

#[derive(Debug, Clone)]
pub struct BatchEngineStatus {
    pub active_batches_count: usize,
    pub pending_opportunities_count: usize,
    pub total_batches_created: u64,
    pub total_batches_executed: u64,
    pub success_rate: f64,
    pub avg_assembly_time_ms: f64,
    pub avg_execution_time_ms: f64,
    pub total_profit_usd: f64,
}

// =============================================================================
// Legacy Type Aliases for Backward Compatibility
// =============================================================================

/// Legacy alias for HftExecutor
pub type ArbitrageExecutor = HftExecutor;

/// Legacy alias for BatchExecutor
pub type BatchExecutionEngine = BatchExecutor;
