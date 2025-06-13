// src/arbitrage/batch_executor.rs
use crate::{
    arbitrage::opportunity::AdvancedMultiHopOpportunity,
    error::ArbError,
    utils::DexType,
};
use log::{info, warn, error};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::Signature,
};
use std::collections::HashMap;
use tokio::time::Instant;

#[derive(Debug, Clone)]
pub struct BatchExecutionConfig {
    pub max_batch_size: usize,
    pub max_compute_units: u32,
    pub max_batch_execution_time_ms: u64,
    pub priority_threshold: u8,
}

impl Default for BatchExecutionConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 5,
            max_compute_units: 1_400_000, // Conservative limit
            max_batch_execution_time_ms: 3000,
            priority_threshold: 7, // Only batch high-priority opportunities
        }
    }
}

#[derive(Debug, Clone)]
pub struct BatchOpportunity {
    pub opportunities: Vec<AdvancedMultiHopOpportunity>,
    pub total_profit_usd: f64,
    pub estimated_gas_cost: u64,
    pub execution_complexity: u8,
    pub requires_atomic_execution: bool,
}

pub struct AdvancedBatchExecutor {
    config: BatchExecutionConfig,
    pending_opportunities: Vec<AdvancedMultiHopOpportunity>,
    execution_stats: ExecutionStats,
}

#[derive(Debug, Default)]
pub struct ExecutionStats {
    pub total_batches_executed: u64,
    pub successful_batches: u64,
    pub failed_batches: u64,
    pub average_batch_size: f64,
    pub total_profit_usd: f64,
    pub total_gas_spent: u64,
}

impl AdvancedBatchExecutor {
    pub fn new(config: BatchExecutionConfig) -> Self {
        info!(
            "AdvancedBatchExecutor initialized with max_batch_size={}, max_compute_units={}, priority_threshold={}",
            config.max_batch_size, config.max_compute_units, config.priority_threshold
        );
        
        Self {
            config,
            pending_opportunities: Vec::new(),
            execution_stats: ExecutionStats::default(),
        }
    }

    /// Add an opportunity to the batch queue
    pub async fn queue_opportunity(&mut self, opportunity: AdvancedMultiHopOpportunity) {
        // Only queue high-priority opportunities for batching
        if opportunity.execution_priority >= self.config.priority_threshold {
            self.pending_opportunities.push(opportunity);
            
            // Check if we should execute a batch
            if self.should_execute_batch().await {
                if let Err(e) = self.execute_pending_batch().await {
                    error!("Failed to execute batch: {}", e);
                }
            }
        }
    }

    /// Determine if we should execute a batch based on various criteria
    async fn should_execute_batch(&self) -> bool {
        if self.pending_opportunities.is_empty() {
            return false;
        }

        // Execute if we have enough opportunities
        if self.pending_opportunities.len() >= self.config.max_batch_size {
            return true;
        }

        // Execute if we have very high priority opportunities
        let has_urgent = self.pending_opportunities.iter()
            .any(|opp| opp.execution_priority >= 9);
        
        if has_urgent && self.pending_opportunities.len() >= 2 {
            return true;
        }

        // Execute if total profit potential is high
        let total_profit: f64 = self.pending_opportunities.iter()
            .map(|opp| opp.expected_profit_usd)
            .sum();
        
        if total_profit > 100.0 && self.pending_opportunities.len() >= 2 {
            return true;
        }

        false
    }

    /// Execute the pending batch of opportunities
    async fn execute_pending_batch(&mut self) -> Result<Vec<Signature>, ArbError> {
        if self.pending_opportunities.is_empty() {
            return Ok(vec![]);
        }

        let start_time = Instant::now();
        
        // Group compatible opportunities into batches
        let batches = self.create_optimal_batches();
        let mut all_signatures = Vec::new();

        info!("Executing {} batches with {} total opportunities", 
              batches.len(), self.pending_opportunities.len());

        for (batch_idx, batch) in batches.into_iter().enumerate() {
            match self.execute_single_batch(batch, batch_idx).await {
                Ok(signatures) => {
                    all_signatures.extend(signatures);
                    self.execution_stats.successful_batches += 1;
                }
                Err(e) => {
                    error!("Batch {} execution failed: {}", batch_idx, e);
                    self.execution_stats.failed_batches += 1;
                }
            }
        }

        // Update execution statistics
        let execution_time = start_time.elapsed();
        self.execution_stats.total_batches_executed += 1;
        self.execution_stats.average_batch_size = 
            (self.execution_stats.average_batch_size * (self.execution_stats.total_batches_executed as f64 - 1.0) + 
             self.pending_opportunities.len() as f64) / self.execution_stats.total_batches_executed as f64;

        // Clear pending opportunities
        self.pending_opportunities.clear();

        info!("Batch execution completed in {:.2}ms, {} signatures obtained", 
              execution_time.as_secs_f64() * 1000.0, all_signatures.len());

        Ok(all_signatures)
    }

    /// Create optimal batches from pending opportunities
    fn create_optimal_batches(&self) -> Vec<BatchOpportunity> {
        let mut batches = Vec::new();
        let mut remaining_opportunities = self.pending_opportunities.clone();

        // Sort by priority and profit
        remaining_opportunities.sort_by(|a, b| {
            b.execution_priority.cmp(&a.execution_priority)
                .then(b.expected_profit_usd.partial_cmp(&a.expected_profit_usd).unwrap())
        });

        while !remaining_opportunities.is_empty() {
            let batch = self.create_single_batch(&mut remaining_opportunities);
            if !batch.opportunities.is_empty() {
                batches.push(batch);
            } else {
                break; // Prevent infinite loop
            }
        }

        batches
    }

    /// Create a single optimized batch
    fn create_single_batch(&self, opportunities: &mut Vec<AdvancedMultiHopOpportunity>) -> BatchOpportunity {
        let mut batch_opportunities = Vec::new();
        let mut total_gas_cost = 0u64;
        let mut total_profit = 0.0;
        let mut max_complexity = 0u8;

        // DEX usage tracking to avoid conflicts
        let mut dex_usage: HashMap<DexType, u32> = HashMap::new();

        for i in (0..opportunities.len()).rev() {
            let opp = &opportunities[i];
            
            // Check if adding this opportunity would exceed limits
            if batch_opportunities.len() >= self.config.max_batch_size {
                break;
            }

            if total_gas_cost + opp.estimated_gas_cost > self.config.max_compute_units as u64 {
                continue;
            }

            // Check for DEX conflicts (simplified - in practice would check specific pools)
            let mut has_conflict = false;
            for dex in &opp.dex_sequence {
                if dex_usage.get(dex).unwrap_or(&0) >= &2 {
                    has_conflict = true;
                    break;
                }
            }

            if has_conflict {
                continue;
            }

            // Add opportunity to batch
            for dex in &opp.dex_sequence {
                *dex_usage.entry(dex.clone()).or_insert(0) += 1;
            }

            total_gas_cost += opp.estimated_gas_cost;
            total_profit += opp.expected_profit_usd;
            max_complexity = max_complexity.max(opp.path_complexity);
            
            batch_opportunities.push(opportunities.remove(i));
        }

        BatchOpportunity {
            opportunities: batch_opportunities,
            total_profit_usd: total_profit,
            estimated_gas_cost: total_gas_cost,
            execution_complexity: max_complexity,
            requires_atomic_execution: max_complexity > 5,
        }
    }

    /// Execute a single batch
    async fn execute_single_batch(
        &mut self, 
        batch: BatchOpportunity, 
        batch_idx: usize
    ) -> Result<Vec<Signature>, ArbError> {
        let start_time = Instant::now();
        
        info!(
            "Executing batch {} with {} opportunities, expected profit: ${:.2}, gas cost: {}",
            batch_idx, batch.opportunities.len(), batch.total_profit_usd, batch.estimated_gas_cost
        );

        // Build transaction instructions for the batch
        let instructions = self.build_batch_instructions(&batch).await?;
        
        if instructions.is_empty() {
            warn!("No instructions generated for batch {}", batch_idx);
            return Ok(vec![]);
        }

        // Add compute budget instructions
        let mut all_instructions = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(batch.estimated_gas_cost as u32),
            ComputeBudgetInstruction::set_compute_unit_price(5_000), // micro-lamports per CU
        ];
        all_instructions.extend(instructions);

        // For simulation purposes, we'll return success
        // In real implementation, this would create and send the transaction
        let execution_time = start_time.elapsed();
        
        // Update statistics
        self.execution_stats.total_profit_usd += batch.total_profit_usd;
        self.execution_stats.total_gas_spent += batch.estimated_gas_cost;

        info!(
            "Batch {} executed successfully in {:.2}ms",
            batch_idx, execution_time.as_secs_f64() * 1000.0
        );

        // Return mock signatures for now
        let signatures: Vec<Signature> = (0..batch.opportunities.len())
            .map(|_| Signature::default())
            .collect();

        Ok(signatures)
    }

    /// Build transaction instructions for a batch
    async fn build_batch_instructions(&self, batch: &BatchOpportunity) -> Result<Vec<Instruction>, ArbError> {
        let mut instructions = Vec::new();

        for opportunity in &batch.opportunities {
            // For each opportunity, build the swap instructions
            for (hop_idx, hop) in opportunity.path.iter().enumerate() {
                // This is where we would build actual DEX-specific swap instructions
                // For now, we'll create placeholder instructions
                
                info!(
                    "Building instruction for hop {} of opportunity {}: {} -> {}",
                    hop_idx + 1, opportunity.id, hop.input_token, hop.output_token
                );

                // In real implementation, this would call the appropriate DEX client
                // to build swap instructions based on the hop.pool_info.dex_type
                
                // Placeholder instruction (in real implementation, would be DEX-specific)
                let instruction = Instruction::new_with_bincode(
                    Pubkey::default(), // Would be the DEX program ID
                    &(), // Would be the actual instruction data
                    vec![], // Would be the actual accounts
                );
                
                instructions.push(instruction);
            }
        }

        Ok(instructions)
    }

    /// Get execution statistics
    pub fn get_stats(&self) -> &ExecutionStats {
        &self.execution_stats
    }

    /// Force execute any pending opportunities
    pub async fn force_execute_pending(&mut self) -> Result<Vec<Signature>, ArbError> {
        if self.pending_opportunities.is_empty() {
            return Ok(vec![]);
        }

        info!("Force executing {} pending opportunities", self.pending_opportunities.len());
        self.execute_pending_batch().await
    }

    /// Clear all pending opportunities
    pub fn clear_pending(&mut self) {
        let count = self.pending_opportunities.len();
        self.pending_opportunities.clear();
        if count > 0 {
            warn!("Cleared {} pending opportunities", count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_batch_executor_initialization() {
        let config = BatchExecutionConfig::default();
        let executor = AdvancedBatchExecutor::new(config.clone());
        
        assert_eq!(executor.config.max_batch_size, config.max_batch_size);
        assert_eq!(executor.pending_opportunities.len(), 0);
    }

    #[tokio::test]
    async fn test_should_execute_batch() {
        let config = BatchExecutionConfig::default();
        let executor = AdvancedBatchExecutor::new(config);
        
        // Should not execute with empty queue
        assert!(!executor.should_execute_batch().await);
    }

    #[test]
    fn test_batch_opportunity_creation() {
        let batch = BatchOpportunity {
            opportunities: vec![],
            total_profit_usd: 100.0,
            estimated_gas_cost: 400_000,
            execution_complexity: 5,
            requires_atomic_execution: true,
        };
        
        assert_eq!(batch.total_profit_usd, 100.0);
        assert!(batch.requires_atomic_execution);
    }
}
