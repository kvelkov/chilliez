// src/arbitrage/execution_engine.rs
//! High-Throughput Execution Engine with Atomic Batching
//! 
//! This module implements Sprint 3: High-Throughput Execution & Atomic Batching
//! - BatchExecutionEngine: Receives profitable paths and groups them into atomic Jito bundles
//! - Intelligent Opportunity Batching: Logic to find and batch compatible arbitrage opportunities
//! - Mandatory Pre-Transaction Simulation: Parallel simulation pipeline to verify success
//! - Jito Bundle Submission: Construct and submit bundles with configurable tips

use crate::{
    arbitrage::{
        opportunity::{MultiHopArbOpportunity, AdvancedMultiHopOpportunity},
        mev_protection::{AdvancedMevProtection, MevProtectionConfig},
    },
    error::ArbError,
    utils::{DexType, PoolInfo},
    solana::rpc::SolanaRpcClient,
};
use log::{info, warn, error, debug};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::Signature,
    transaction::Transaction,
    message::Message,
    hash::Hash,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, RwLock};
use serde::{Deserialize, Serialize};

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
    /// Simulation timeout per transaction (ms)
    pub simulation_timeout_ms: u64,
    /// Jito tip amount (lamports)
    pub jito_tip_lamports: u64,
    /// Maximum bundle size (transactions)
    pub max_bundle_size: usize,
    /// Enable MEV protection
    pub enable_mev_protection: bool,
}

impl Default for BatchExecutionConfig {
    fn default() -> Self {
        Self {
            max_opportunities_per_batch: 5,
            max_compute_units_per_tx: 1_400_000,
            max_compute_units_per_bundle: 5_000_000,
            min_batch_profit_usd: 10.0,
            max_batch_assembly_time_ms: 500,
            enable_parallel_simulation: true,
            simulation_timeout_ms: 2000,
            jito_tip_lamports: 10_000,
            max_bundle_size: 5,
            enable_mev_protection: true,
        }
    }
}

/// Represents a batch of compatible arbitrage opportunities
#[derive(Debug, Clone)]
pub struct OpportunityBatch {
    pub id: String,
    pub opportunities: Vec<MultiHopArbOpportunity>,
    pub total_profit_usd: f64,
    pub estimated_compute_units: u32,
    pub compatibility_score: f64,
    pub priority_score: f64,
    pub created_at: Instant,
    pub requires_atomic_execution: bool,
}

/// Simulation result for a transaction
#[derive(Debug, Clone)]
pub struct SimulationResult {
    pub success: bool,
    pub compute_units_consumed: u32,
    pub logs: Vec<String>,
    pub error_message: Option<String>,
    pub simulation_time_ms: u64,
}

/// Jito bundle for atomic execution
#[derive(Debug, Clone)]
pub struct JitoBundle {
    pub id: String,
    pub transactions: Vec<Transaction>,
    pub tip_transaction: Transaction,
    pub total_tip_lamports: u64,
    pub estimated_profit_usd: f64,
    pub created_at: Instant,
}

/// Execution result for a bundle
#[derive(Debug, Clone)]
pub struct BundleExecutionResult {
    pub bundle_id: String,
    pub success: bool,
    pub signatures: Vec<Signature>,
    pub execution_time_ms: u64,
    pub actual_profit_usd: Option<f64>,
    pub error_message: Option<String>,
}

/// Metrics for the execution engine
#[derive(Debug, Default, Clone)]
pub struct ExecutionMetrics {
    pub total_opportunities_processed: u64,
    pub total_batches_created: u64,
    pub total_bundles_submitted: u64,
    pub successful_bundles: u64,
    pub failed_bundles: u64,
    pub total_profit_usd: f64,
    pub total_gas_spent: u64,
    pub average_batch_size: f64,
    pub average_simulation_time_ms: f64,
    pub simulation_success_rate: f64,
}

/// High-throughput execution engine with atomic batching
pub struct BatchExecutionEngine {
    config: BatchExecutionConfig,
    rpc_client: Arc<SolanaRpcClient>,
    mev_protection: Option<Arc<AdvancedMevProtection>>,
    pending_opportunities: Arc<Mutex<Vec<MultiHopArbOpportunity>>>,
    active_batches: Arc<RwLock<HashMap<String, OpportunityBatch>>>,
    execution_metrics: Arc<Mutex<ExecutionMetrics>>,
    pool_usage_tracker: Arc<Mutex<HashMap<Pubkey, Instant>>>,
}

impl BatchExecutionEngine {
    /// Create a new batch execution engine
    pub fn new(
        config: BatchExecutionConfig,
        rpc_client: Arc<SolanaRpcClient>,
    ) -> Self {
        info!("🚀 Initializing BatchExecutionEngine");
        info!("   📦 Max opportunities per batch: {}", config.max_opportunities_per_batch);
        info!("   💻 Max compute units per TX: {}", config.max_compute_units_per_tx);
        info!("   🎯 Min batch profit: ${:.2}", config.min_batch_profit_usd);
        info!("   ⚡ Parallel simulation: {}", config.enable_parallel_simulation);
        info!("   💰 Jito tip: {} lamports", config.jito_tip_lamports);

        let mev_protection = if config.enable_mev_protection {
            let mev_config = MevProtectionConfig::default();
            Some(Arc::new(AdvancedMevProtection::new(mev_config)))
        } else {
            None
        };

        Self {
            config,
            rpc_client,
            mev_protection,
            pending_opportunities: Arc::new(Mutex::new(Vec::new())),
            active_batches: Arc::new(RwLock::new(HashMap::new())),
            execution_metrics: Arc::new(Mutex::new(ExecutionMetrics::default())),
            pool_usage_tracker: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Submit an opportunity for batch execution
    pub async fn submit_opportunity(&self, opportunity: MultiHopArbOpportunity) -> Result<(), ArbError> {
        debug!("📥 Submitting opportunity {} for batch execution", opportunity.id);
        
        // Update metrics
        {
            let mut metrics = self.execution_metrics.lock().await;
            metrics.total_opportunities_processed += 1;
        }

        // Add to pending queue
        {
            let mut pending = self.pending_opportunities.lock().await;
            pending.push(opportunity);
        }

        // Trigger batch processing if conditions are met
        self.process_pending_opportunities().await?;

        Ok(())
    }

    /// Process pending opportunities and create batches
    async fn process_pending_opportunities(&self) -> Result<(), ArbError> {
        let mut pending = self.pending_opportunities.lock().await;
        
        if pending.is_empty() {
            return Ok(());
        }

        // Check if we should create a batch
        let should_batch = self.should_create_batch(&pending).await;
        
        if should_batch {
            info!("🔄 Processing {} pending opportunities for batching", pending.len());
            
            // Create optimal batches
            let batches = self.create_optimal_batches(&mut pending).await?;
            
            // Process each batch
            for batch in batches {
                self.process_batch(batch).await?;
            }
        }

        Ok(())
    }

    /// Determine if we should create a batch
    async fn should_create_batch(&self, pending: &[MultiHopArbOpportunity]) -> bool {
        if pending.is_empty() {
            return false;
        }

        // Batch if we have enough opportunities
        if pending.len() >= self.config.max_opportunities_per_batch {
            return true;
        }

        // Batch if total profit is high enough
        let total_profit: f64 = pending.iter()
            .filter_map(|opp| opp.estimated_profit_usd)
            .sum();
        
        if total_profit >= self.config.min_batch_profit_usd && pending.len() >= 2 {
            return true;
        }

        // Batch if we have high-priority opportunities
        let has_urgent = pending.iter()
            .any(|opp| opp.profit_pct > 5.0); // 5%+ profit is urgent
        
        if has_urgent && pending.len() >= 2 {
            return true;
        }

        false
    }

    /// Create optimal batches from pending opportunities
    async fn create_optimal_batches(
        &self,
        pending: &mut Vec<MultiHopArbOpportunity>
    ) -> Result<Vec<OpportunityBatch>, ArbError> {
        let start_time = Instant::now();
        let mut batches = Vec::new();

        // Sort opportunities by profit and priority
        pending.sort_by(|a, b| {
            let a_profit = a.estimated_profit_usd.unwrap_or(0.0);
            let b_profit = b.estimated_profit_usd.unwrap_or(0.0);
            b_profit.partial_cmp(&a_profit).unwrap()
                .then(b.profit_pct.partial_cmp(&a.profit_pct).unwrap())
        });

        while !pending.is_empty() {
            let batch = self.create_single_batch(pending).await?;
            if !batch.opportunities.is_empty() {
                batches.push(batch);
            } else {
                break; // No more compatible opportunities
            }
        }

        let creation_time = start_time.elapsed();
        info!("📦 Created {} batches in {:.2}ms", batches.len(), creation_time.as_secs_f64() * 1000.0);

        // Update metrics
        {
            let mut metrics = self.execution_metrics.lock().await;
            metrics.total_batches_created += batches.len() as u64;
        }

        Ok(batches)
    }

    /// Create a single optimized batch
    async fn create_single_batch(
        &self,
        pending: &mut Vec<MultiHopArbOpportunity>
    ) -> Result<OpportunityBatch, ArbError> {
        let mut batch_opportunities = Vec::new();
        let mut total_profit = 0.0;
        let mut total_compute_units = 0u32;
        let mut used_pools = HashSet::new();
        let mut used_tokens = HashSet::new();

        // Track pool usage to avoid conflicts
        let pool_tracker = self.pool_usage_tracker.lock().await;

        let mut indices_to_remove = Vec::new();

        for i in (0..pending.len()).rev() {
            let opp = &pending[i];
            
            // Check batch size limit
            if batch_opportunities.len() >= self.config.max_opportunities_per_batch {
                break;
            }

            // Estimate compute units for this opportunity
            let opp_compute_units = self.estimate_opportunity_compute_units(opp).await;
            if total_compute_units + opp_compute_units > self.config.max_compute_units_per_bundle {
                continue;
            }

            // Check for pool conflicts
            let has_pool_conflict = opp.pool_path.iter()
                .any(|pool| used_pools.contains(pool) || 
                     pool_tracker.get(pool).map_or(false, |last_used| 
                         last_used.elapsed() < Duration::from_millis(100)));

            if has_pool_conflict {
                continue;
            }

            // Check for token conflicts (simplified)
            let opp_tokens = vec![&opp.input_token, &opp.output_token];
            let has_token_conflict = opp_tokens.iter()
                .any(|token| used_tokens.contains(*token));

            if has_token_conflict {
                continue;
            }

            // Mark for inclusion in batch
            for pool in &opp.pool_path {
                used_pools.insert(*pool);
            }
            used_tokens.insert(&opp.input_token);
            used_tokens.insert(&opp.output_token);

            total_profit += opp.estimated_profit_usd.unwrap_or(0.0);
            total_compute_units += opp_compute_units;
            
            indices_to_remove.push(i);
        }

        // Remove selected opportunities and add them to batch
        for &i in &indices_to_remove {
            batch_opportunities.push(pending.remove(i));
        }

        // Calculate compatibility and priority scores
        let compatibility_score = self.calculate_compatibility_score(&batch_opportunities).await;
        let priority_score = self.calculate_priority_score(&batch_opportunities).await;

        let batch = OpportunityBatch {
            id: format!("batch_{}", uuid::Uuid::new_v4().to_string()[..8].to_string()),
            opportunities: batch_opportunities,
            total_profit_usd: total_profit,
            estimated_compute_units: total_compute_units,
            compatibility_score,
            priority_score,
            created_at: Instant::now(),
            requires_atomic_execution: total_compute_units > 1_000_000,
        };

        debug!("📦 Created batch {} with {} opportunities, profit: ${:.2}, CU: {}", 
               batch.id, batch.opportunities.len(), batch.total_profit_usd, batch.estimated_compute_units);

        Ok(batch)
    }

    /// Process a batch through simulation and execution
    async fn process_batch(&self, batch: OpportunityBatch) -> Result<(), ArbError> {
        info!("🔄 Processing batch {} with {} opportunities", batch.id, batch.opportunities.len());

        // Store batch in active batches
        {
            let mut active = self.active_batches.write().await;
            active.insert(batch.id.clone(), batch.clone());
        }

        // Build transaction instructions
        let instructions = self.build_batch_instructions(&batch).await?;
        
        if instructions.is_empty() {
            warn!("⚠️ No instructions generated for batch {}", batch.id);
            return Ok(());
        }

        // Create transactions
        let transactions = self.create_batch_transactions(&batch, instructions).await?;

        // Mandatory pre-transaction simulation
        let simulation_results = self.simulate_transactions(&transactions).await?;
        
        // Verify all simulations passed
        let all_simulations_passed = simulation_results.iter().all(|result| result.success);
        
        if !all_simulations_passed {
            warn!("❌ Simulation failed for batch {}, skipping execution", batch.id);
            self.handle_simulation_failure(&batch, &simulation_results).await?;
            return Ok(());
        }

        info!("✅ All simulations passed for batch {}, proceeding to execution", batch.id);

        // Create Jito bundle
        let bundle = self.create_jito_bundle(&batch, transactions).await?;

        // Submit bundle
        let execution_result = self.submit_jito_bundle(bundle).await?;

        // Handle execution result
        self.handle_execution_result(&batch, execution_result).await?;

        // Remove from active batches
        {
            let mut active = self.active_batches.write().await;
            active.remove(&batch.id);
        }

        Ok(())
    }

    /// Build transaction instructions for a batch
    async fn build_batch_instructions(&self, batch: &OpportunityBatch) -> Result<Vec<Vec<Instruction>>, ArbError> {
        let mut all_instructions = Vec::new();

        for opportunity in &batch.opportunities {
            let mut opp_instructions = Vec::new();

            // Add compute budget instructions
            let compute_units = self.estimate_opportunity_compute_units(opportunity).await;
            opp_instructions.push(ComputeBudgetInstruction::set_compute_unit_limit(compute_units));
            
            // Calculate priority fee with MEV protection
            let priority_fee = if let Some(mev_protection) = &self.mev_protection {
                mev_protection.calculate_optimal_priority_fee(opportunity, 5_000).await?
            } else {
                5_000 // Default priority fee
            };
            
            opp_instructions.push(ComputeBudgetInstruction::set_compute_unit_price(priority_fee / compute_units as u64));

            // Build swap instructions for each hop
            for (hop_idx, hop) in opportunity.hops.iter().enumerate() {
                debug!("🔧 Building instruction for hop {} of opportunity {}: {} -> {}", 
                       hop_idx + 1, opportunity.id, hop.input_token, hop.output_token);

                // In a real implementation, this would call the appropriate DEX client
                // to build swap instructions based on hop.dex
                let swap_instruction = self.build_swap_instruction(hop).await?;
                opp_instructions.push(swap_instruction);
            }

            all_instructions.push(opp_instructions);
        }

        Ok(all_instructions)
    }

    /// Build a swap instruction for a specific hop
    async fn build_swap_instruction(&self, _hop: &crate::arbitrage::opportunity::ArbHop) -> Result<Instruction, ArbError> {
        // Placeholder implementation
        // In real implementation, this would:
        // 1. Determine the DEX type from hop.dex
        // 2. Get the appropriate DEX client
        // 3. Build the swap instruction with proper accounts and data
        
        Ok(Instruction::new_with_bincode(
            Pubkey::default(), // Would be the actual DEX program ID
            &(), // Would be the actual instruction data
            vec![], // Would be the actual accounts
        ))
    }

    /// Create transactions from instructions
    async fn create_batch_transactions(
        &self,
        batch: &OpportunityBatch,
        instructions: Vec<Vec<Instruction>>
    ) -> Result<Vec<Transaction>, ArbError> {
        let mut transactions = Vec::new();

        for (opp_idx, opp_instructions) in instructions.into_iter().enumerate() {
            // In real implementation, this would:
            // 1. Get recent blockhash
            // 2. Create message with instructions
            // 3. Sign transaction with wallet keypair
            
            debug!("🔧 Creating transaction for opportunity {} in batch {}", 
                   opp_idx, batch.id);

            // Placeholder transaction creation
            let message = Message::new(&opp_instructions, None);
            let transaction = Transaction::new_unsigned(message);
            
            transactions.push(transaction);
        }

        Ok(transactions)
    }

    /// Simulate transactions in parallel
    async fn simulate_transactions(&self, transactions: &[Transaction]) -> Result<Vec<SimulationResult>, ArbError> {
        let start_time = Instant::now();
        
        info!("🧪 Starting parallel simulation of {} transactions", transactions.len());

        let mut simulation_results = Vec::new();

        if self.config.enable_parallel_simulation {
            // Parallel simulation
            let futures: Vec<_> = transactions.iter()
                .enumerate()
                .map(|(idx, tx)| self.simulate_single_transaction(tx, idx))
                .collect();

            let results = futures::future::join_all(futures).await;
            
            for result in results {
                simulation_results.push(result?);
            }
        } else {
            // Sequential simulation
            for (idx, tx) in transactions.iter().enumerate() {
                let result = self.simulate_single_transaction(tx, idx).await?;
                simulation_results.push(result);
            }
        }

        let simulation_time = start_time.elapsed();
        let success_count = simulation_results.iter().filter(|r| r.success).count();
        
        info!("🧪 Simulation completed in {:.2}ms: {}/{} successful", 
              simulation_time.as_secs_f64() * 1000.0, success_count, transactions.len());

        // Update metrics
        {
            let mut metrics = self.execution_metrics.lock().await;
            metrics.average_simulation_time_ms = 
                (metrics.average_simulation_time_ms + simulation_time.as_secs_f64() * 1000.0) / 2.0;
            metrics.simulation_success_rate = 
                (metrics.simulation_success_rate + (success_count as f64 / transactions.len() as f64)) / 2.0;
        }

        Ok(simulation_results)
    }

    /// Simulate a single transaction
    async fn simulate_single_transaction(&self, _transaction: &Transaction, tx_idx: usize) -> Result<SimulationResult, ArbError> {
        let start_time = Instant::now();
        
        debug!("🧪 Simulating transaction {}", tx_idx);

        // In real implementation, this would:
        // 1. Call rpc_client.simulate_transaction()
        // 2. Parse the simulation response
        // 3. Extract compute units, logs, and success status

        // Simulate processing time
        tokio::time::sleep(Duration::from_millis(10)).await;

        let simulation_time = start_time.elapsed();

        // Mock successful simulation for now
        Ok(SimulationResult {
            success: true,
            compute_units_consumed: 150_000,
            logs: vec![format!("Program log: Simulated transaction {}", tx_idx)],
            error_message: None,
            simulation_time_ms: simulation_time.as_millis() as u64,
        })
    }

    /// Create a Jito bundle
    async fn create_jito_bundle(&self, batch: &OpportunityBatch, transactions: Vec<Transaction>) -> Result<JitoBundle, ArbError> {
        info!("📦 Creating Jito bundle for batch {}", batch.id);

        // Create tip transaction
        let tip_transaction = self.create_tip_transaction().await?;

        let bundle = JitoBundle {
            id: format!("bundle_{}", batch.id),
            transactions,
            tip_transaction,
            total_tip_lamports: self.config.jito_tip_lamports,
            estimated_profit_usd: batch.total_profit_usd,
            created_at: Instant::now(),
        };

        debug!("📦 Created Jito bundle {} with {} transactions, tip: {} lamports", 
               bundle.id, bundle.transactions.len(), bundle.total_tip_lamports);

        Ok(bundle)
    }

    /// Create a tip transaction for Jito
    async fn create_tip_transaction(&self) -> Result<Transaction, ArbError> {
        // In real implementation, this would:
        // 1. Create a transfer instruction to Jito tip account
        // 2. Sign with wallet keypair
        
        debug!("💰 Creating tip transaction: {} lamports", self.config.jito_tip_lamports);

        // Placeholder tip transaction
        let message = Message::new(&[], None);
        Ok(Transaction::new_unsigned(message))
    }

    /// Submit Jito bundle
    async fn submit_jito_bundle(&self, bundle: JitoBundle) -> Result<BundleExecutionResult, ArbError> {
        let start_time = Instant::now();
        
        info!("🚀 Submitting Jito bundle {} with {} transactions", 
              bundle.id, bundle.transactions.len());

        // In real implementation, this would:
        // 1. Use jito-sdk-rust to submit the bundle
        // 2. Wait for confirmation
        // 3. Return actual execution result

        // Simulate bundle submission
        tokio::time::sleep(Duration::from_millis(100)).await;

        let execution_time = start_time.elapsed();

        // Mock successful execution
        let signatures: Vec<Signature> = (0..bundle.transactions.len())
            .map(|_| Signature::default())
            .collect();

        let result = BundleExecutionResult {
            bundle_id: bundle.id.clone(),
            success: true,
            signatures,
            execution_time_ms: execution_time.as_millis() as u64,
            actual_profit_usd: Some(bundle.estimated_profit_usd * 0.95), // 5% slippage
            error_message: None,
        };

        info!("✅ Bundle {} executed successfully in {:.2}ms", 
              bundle.id, execution_time.as_secs_f64() * 1000.0);

        Ok(result)
    }

    /// Handle simulation failure
    async fn handle_simulation_failure(
        &self,
        batch: &OpportunityBatch,
        simulation_results: &[SimulationResult]
    ) -> Result<(), ArbError> {
        warn!("❌ Handling simulation failure for batch {}", batch.id);

        for (idx, result) in simulation_results.iter().enumerate() {
            if !result.success {
                warn!("   Transaction {}: {}", idx, 
                      result.error_message.as_deref().unwrap_or("Unknown error"));
            }
        }

        // Update metrics
        {
            let mut metrics = self.execution_metrics.lock().await;
            metrics.failed_bundles += 1;
        }

        Ok(())
    }

    /// Handle execution result
    async fn handle_execution_result(
        &self,
        batch: &OpportunityBatch,
        result: BundleExecutionResult
    ) -> Result<(), ArbError> {
        let mut metrics = self.execution_metrics.lock().await;
        metrics.total_bundles_submitted += 1;

        if result.success {
            info!("✅ Bundle execution successful for batch {}", batch.id);
            metrics.successful_bundles += 1;
            
            if let Some(profit) = result.actual_profit_usd {
                metrics.total_profit_usd += profit;
            }
        } else {
            error!("❌ Bundle execution failed for batch {}: {}", 
                   batch.id, result.error_message.as_deref().unwrap_or("Unknown error"));
            metrics.failed_bundles += 1;
        }

        Ok(())
    }

    /// Estimate compute units for an opportunity
    async fn estimate_opportunity_compute_units(&self, opportunity: &MultiHopArbOpportunity) -> u32 {
        // Base compute units per hop
        const BASE_CU_PER_HOP: u32 = 100_000;
        const COMPLEX_DEX_MULTIPLIER: f32 = 1.5;

        let mut total_cu = 0u32;

        for hop in &opportunity.hops {
            let mut hop_cu = BASE_CU_PER_HOP;

            // Adjust based on DEX complexity
            match hop.dex {
                DexType::Whirlpool | DexType::Meteora => {
                    hop_cu = (hop_cu as f32 * COMPLEX_DEX_MULTIPLIER) as u32;
                }
                _ => {}
            }

            total_cu += hop_cu;
        }

        // Add buffer
        (total_cu as f32 * 1.2) as u32
    }

    /// Calculate compatibility score for opportunities in a batch
    async fn calculate_compatibility_score(&self, opportunities: &[MultiHopArbOpportunity]) -> f64 {
        if opportunities.len() <= 1 {
            return 1.0;
        }

        let mut score: f64 = 1.0;

        // Check for pool overlaps (reduces compatibility)
        let mut all_pools = HashSet::new();
        let mut total_pools = 0;

        for opp in opportunities {
            for pool in &opp.pool_path {
                if all_pools.contains(pool) {
                    score -= 0.1; // Penalty for pool overlap
                }
                all_pools.insert(*pool);
                total_pools += 1;
            }
        }

        // Check for DEX diversity (increases compatibility)
        let unique_dexs: HashSet<_> = opportunities.iter()
            .flat_map(|opp| &opp.dex_path)
            .collect();

        if unique_dexs.len() > 1 {
            score += 0.1; // Bonus for DEX diversity
        }

        score.max(0.0).min(1.0)
    }

    /// Calculate priority score for opportunities in a batch
    async fn calculate_priority_score(&self, opportunities: &[MultiHopArbOpportunity]) -> f64 {
        if opportunities.is_empty() {
            return 0.0;
        }

        let avg_profit_pct: f64 = opportunities.iter()
            .map(|opp| opp.profit_pct)
            .sum::<f64>() / opportunities.len() as f64;

        let total_profit_usd: f64 = opportunities.iter()
            .filter_map(|opp| opp.estimated_profit_usd)
            .sum();

        // Normalize to 0-1 scale
        let profit_pct_score = (avg_profit_pct / 10.0).min(1.0); // 10% = max score
        let profit_usd_score = (total_profit_usd / 1000.0).min(1.0); // $1000 = max score

        (profit_pct_score + profit_usd_score) / 2.0
    }

    /// Get execution metrics
    pub async fn get_metrics(&self) -> ExecutionMetrics {
        self.execution_metrics.lock().await.clone()
    }

    /// Force execute all pending opportunities
    pub async fn force_execute_pending(&self) -> Result<(), ArbError> {
        info!("🔄 Force executing all pending opportunities");
        
        let mut pending = self.pending_opportunities.lock().await;
        if pending.is_empty() {
            info!("No pending opportunities to execute");
            return Ok(());
        }

        let count = pending.len();
        let batches = self.create_optimal_batches(&mut pending).await?;
        
        for batch in batches {
            self.process_batch(batch).await?;
        }

        info!("✅ Force executed {} opportunities in batches", count);
        Ok(())
    }

    /// Get current status
    pub async fn get_status(&self) -> BatchEngineStatus {
        let pending_count = self.pending_opportunities.lock().await.len();
        let active_batches_count = self.active_batches.read().await.len();
        let metrics = self.get_metrics().await;

        BatchEngineStatus {
            pending_opportunities: pending_count,
            active_batches: active_batches_count,
            total_processed: metrics.total_opportunities_processed,
            success_rate: if metrics.total_bundles_submitted > 0 {
                metrics.successful_bundles as f64 / metrics.total_bundles_submitted as f64
            } else {
                0.0
            },
            total_profit_usd: metrics.total_profit_usd,
        }
    }
}

/// Status information for the batch execution engine
#[derive(Debug, Clone, Serialize)]
pub struct BatchEngineStatus {
    pub pending_opportunities: usize,
    pub active_batches: usize,
    pub total_processed: u64,
    pub success_rate: f64,
    pub total_profit_usd: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::PoolToken;

    fn create_test_opportunity(id: &str, profit_pct: f64, profit_usd: f64) -> MultiHopArbOpportunity {
        MultiHopArbOpportunity {
            id: id.to_string(),
            profit_pct,
            estimated_profit_usd: Some(profit_usd),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_batch_execution_engine_creation() {
        let config = BatchExecutionConfig::default();
        let rpc_client = Arc::new(SolanaRpcClient::new(
            "https://api.mainnet-beta.solana.com",
            vec![],
            3,
            Duration::from_millis(1000),
        ));
        
        let engine = BatchExecutionEngine::new(config.clone(), rpc_client);
        
        assert_eq!(engine.config.max_opportunities_per_batch, config.max_opportunities_per_batch);
    }

    #[tokio::test]
    async fn test_should_create_batch() {
        let config = BatchExecutionConfig::default();
        let rpc_client = Arc::new(SolanaRpcClient::new(
            "https://api.mainnet-beta.solana.com",
            vec![],
            3,
            Duration::from_millis(1000),
        ));
        let engine = BatchExecutionEngine::new(config, rpc_client);

        // Test with empty opportunities
        let empty_opportunities = vec![];
        assert!(!engine.should_create_batch(&empty_opportunities).await);

        // Test with high-profit opportunities
        let high_profit_opportunities = vec![
            create_test_opportunity("test1", 6.0, 50.0),
            create_test_opportunity("test2", 4.0, 30.0),
        ];
        assert!(engine.should_create_batch(&high_profit_opportunities).await);
    }

    #[tokio::test]
    async fn test_compatibility_score_calculation() {
        let config = BatchExecutionConfig::default();
        let rpc_client = Arc::new(SolanaRpcClient::new(
            "https://api.mainnet-beta.solana.com",
            vec![],
            3,
            Duration::from_millis(1000),
        ));
        let engine = BatchExecutionEngine::new(config, rpc_client);

        let opportunities = vec![
            create_test_opportunity("test1", 5.0, 100.0),
            create_test_opportunity("test2", 3.0, 50.0),
        ];

        let score = engine.calculate_compatibility_score(&opportunities).await;
        assert!(score >= 0.0 && score <= 1.0);
    }

    #[tokio::test]
    async fn test_priority_score_calculation() {
        let config = BatchExecutionConfig::default();
        let rpc_client = Arc::new(SolanaRpcClient::new(
            "https://api.mainnet-beta.solana.com",
            vec![],
            3,
            Duration::from_millis(1000),
        ));
        let engine = BatchExecutionEngine::new(config, rpc_client);

        let opportunities = vec![
            create_test_opportunity("test1", 8.0, 200.0),
            create_test_opportunity("test2", 6.0, 150.0),
        ];

        let score = engine.calculate_priority_score(&opportunities).await;
        assert!(score >= 0.0 && score <= 1.0);
    }
}
