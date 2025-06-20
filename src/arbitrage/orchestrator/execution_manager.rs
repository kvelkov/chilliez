//! Execution Manager Module
//!
//! This module handles all execution-related logic including strategy selection,
//! execution coordination, and execution monitoring.

use super::core::ArbitrageOrchestrator;
use crate::{arbitrage::opportunity::MultiHopArbOpportunity, error::ArbError};

use log::{debug, info, warn};
use rust_decimal::Decimal;
use std::sync::atomic::Ordering;

/// Strategy for executing arbitrage opportunities
#[derive(Debug, Clone)]
pub enum ExecutionStrategy {
    /// Execute all opportunities immediately using single executor
    SingleExecution(Vec<MultiHopArbOpportunity>),
    /// Execute using batch engine
    BatchExecution(Vec<MultiHopArbOpportunity>, Vec<MultiHopArbOpportunity>),
    /// Hybrid approach: immediate execution + batching
    HybridExecution {
        immediate: Vec<MultiHopArbOpportunity>,
        batchable: Vec<MultiHopArbOpportunity>,
    },
}

/// Analysis of opportunity competitiveness for execution decision
#[derive(Debug, Clone)]
pub struct CompetitivenessAnalysis {
    _competitive_score: Decimal,
    _risk_factors: Vec<String>,
    execution_recommendation: ExecutionRecommendation,
    _reason: String,
}

/// Recommendation for execution method based on competitiveness
#[derive(Debug, Clone)]
enum ExecutionRecommendation {
    /// Execute immediately with single executor for speed
    ImmediateSingle,
    /// Safe to include in batch execution
    SafeToBatch,
}

impl ArbitrageOrchestrator {
    /// Execute a single arbitrage opportunity
    pub async fn execute_single_opportunity(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<(), ArbError> {
        if !self.execution_enabled.load(Ordering::Relaxed) {
            return Err(ArbError::ExecutionDisabled(
                "Execution is currently disabled".to_string(),
            ));
        }

        info!(
            "🚀 Executing single arbitrage opportunity: {} -> {}",
            opportunity.input_token, opportunity.output_token
        );

        // Validate opportunity quotes using price aggregator
        if let Ok(is_valid) = self.validate_opportunity_quotes(opportunity).await {
            if !is_valid {
                warn!("⚠️ Skipping opportunity due to quote validation failure");
                return Err(ArbError::InvalidPoolState(
                    "Quote validation failed".to_string(),
                ));
            }
        }

        // Check if paper trading mode
        if let Some(ref paper_engine) = self.paper_trading_engine {
            return self
                .execute_paper_trading_opportunity(opportunity, paper_engine)
                .await;
        }

        // Live execution
        // Here, implement the actual trade execution logic (send transaction, etc.)
        // For now, just log and return Ok
        info!("[LIVE] Would execute trade on-chain here");
        Ok(())
    }

    /// Execute multiple opportunities using the optimal strategy
    pub async fn execute_opportunities_managed(
        &self,
        opportunities: Vec<MultiHopArbOpportunity>,
    ) -> Result<(), ArbError> {
        if opportunities.is_empty() {
            debug!("No opportunities to execute");
            return Ok(());
        }

        info!(
            "🎯 Executing {} arbitrage opportunities",
            opportunities.len()
        );

        // Analyze competitiveness and determine execution strategy
        let strategy = self.determine_execution_strategy(&opportunities).await?;

        match strategy {
            ExecutionStrategy::SingleExecution(ops) => {
                self.execute_opportunities_sequentially(ops).await
            }
            ExecutionStrategy::BatchExecution(batchable, immediate) => {
                self.execute_opportunities_in_batch(batchable, immediate)
                    .await
            }
            ExecutionStrategy::HybridExecution {
                immediate,
                batchable,
            } => {
                self.execute_opportunities_hybrid(immediate, batchable)
                    .await
            }
        }
    }

    /// Determine the optimal execution strategy for a set of opportunities
    async fn determine_execution_strategy(
        &self,
        opportunities: &[MultiHopArbOpportunity],
    ) -> Result<ExecutionStrategy, ArbError> {
        let mut immediate_ops = Vec::new();
        let mut batchable_ops = Vec::new();

        for opportunity in opportunities {
            let analysis = self.analyze_competitiveness(opportunity).await?;

            match analysis.execution_recommendation {
                ExecutionRecommendation::ImmediateSingle => {
                    immediate_ops.push(opportunity.clone());
                }
                ExecutionRecommendation::SafeToBatch => {
                    batchable_ops.push(opportunity.clone());
                }
            }
        }

        // Determine strategy based on distribution
        let strategy = if immediate_ops.is_empty() && !batchable_ops.is_empty() {
            ExecutionStrategy::BatchExecution(batchable_ops, Vec::new())
        } else if !immediate_ops.is_empty() && batchable_ops.is_empty() {
            ExecutionStrategy::SingleExecution(immediate_ops)
        } else if !immediate_ops.is_empty() && !batchable_ops.is_empty() {
            ExecutionStrategy::HybridExecution {
                immediate: immediate_ops,
                batchable: batchable_ops,
            }
        } else {
            ExecutionStrategy::SingleExecution(Vec::new()) // No opportunities
        };

        debug!("📋 Execution strategy determined: {:?}", strategy);
        Ok(strategy)
    }

    /// Analyze the competitiveness of an opportunity
    async fn analyze_competitiveness(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<CompetitivenessAnalysis, ArbError> {
        let mut risk_factors = Vec::new();
        let mut competitive_score = Decimal::from(100); // Start with 100% competitiveness

        // Factor 1: Profit margin
        if opportunity.profit_pct < 50.0 {
            risk_factors.push("Low profit margin".to_string());
            competitive_score -= Decimal::from(20);
        }

        // Factor 2: Gas costs vs profit
        let estimated_gas_cost = self.estimate_execution_cost(opportunity).await?;
        if estimated_gas_cost > opportunity.total_profit * 0.5 {
            risk_factors.push("High gas cost ratio".to_string());
            competitive_score -= Decimal::from(30);
        }

        // Factor 3: Hop count (more hops = more risk)
        if opportunity.hops.len() > 3 {
            risk_factors.push("High hop count".to_string());
            competitive_score -= Decimal::from(15);
        }

        // Factor 4: Pool liquidity - need to get pool info from somewhere
        // This is a simplified check - in practice we'd look up pool info
        for _hop in &opportunity.hops {
            // Since hop.pool is just a Pubkey, we'd need to look up the actual pool info
            // For now, we'll skip this check or use opportunity-level pool info
        }

        // Determine execution recommendation
        let execution_recommendation = if competitive_score >= Decimal::from(70) {
            ExecutionRecommendation::ImmediateSingle
        } else {
            ExecutionRecommendation::SafeToBatch
        };

        let reason = format!(
            "Competitive score: {}, Risk factors: {}",
            competitive_score,
            risk_factors.len()
        );

        Ok(CompetitivenessAnalysis {
            _competitive_score: competitive_score,
            _risk_factors: risk_factors,
            execution_recommendation,
            _reason: reason,
        })
    }

    /// Execute opportunities sequentially
    async fn execute_opportunities_sequentially(
        &self,
        opportunities: Vec<MultiHopArbOpportunity>,
    ) -> Result<(), ArbError> {
        info!(
            "🔄 Executing {} opportunities sequentially",
            opportunities.len()
        );

        let mut successful = 0;
        let mut failed = 0;

        for opportunity in opportunities {
            match self.execute_single_opportunity(&opportunity).await {
                Ok(_) => successful += 1,
                Err(e) => {
                    failed += 1;
                    warn!("⚠️ Sequential execution failed for opportunity: {}", e);
                }
            }
        }

        info!(
            "📊 Sequential execution completed: {} successful, {} failed",
            successful, failed
        );
        Ok(())
    }

    /// Execute opportunities in batch
    async fn execute_opportunities_in_batch(
        &self,
        batchable: Vec<MultiHopArbOpportunity>,
        immediate: Vec<MultiHopArbOpportunity>,
    ) -> Result<(), ArbError> {
        info!(
            "📦 Executing {} batchable + {} immediate opportunities",
            batchable.len(),
            immediate.len()
        );

        // Execute immediate opportunities first
        if !immediate.is_empty() {
            self.execute_opportunities_sequentially(immediate).await?;
        }

        // Execute batchable opportunities
        if !batchable.is_empty() {
            // For now, fall back to sequential execution since execute_batch is private
            warn!("⚠️ Batch execution method is private, falling back to sequential execution");
            self.execute_opportunities_sequentially(batchable).await?;
        }

        Ok(())
    }

    /// Execute opportunities using hybrid strategy
    async fn execute_opportunities_hybrid(
        &self,
        immediate: Vec<MultiHopArbOpportunity>,
        batchable: Vec<MultiHopArbOpportunity>,
    ) -> Result<(), ArbError> {
        info!(
            "🔀 Executing hybrid strategy: {} immediate, {} batchable",
            immediate.len(),
            batchable.len()
        );

        // Execute immediate and batchable concurrently
        let immediate_task = async {
            if !immediate.is_empty() {
                self.execute_opportunities_sequentially(immediate).await
            } else {
                Ok(())
            }
        };

        let batch_task = async {
            if !batchable.is_empty() {
                self.execute_opportunities_in_batch(batchable, Vec::new())
                    .await
            } else {
                Ok(())
            }
        };

        // Execute both tasks concurrently
        let (immediate_result, batch_result) = tokio::join!(immediate_task, batch_task);

        // Check results
        immediate_result?;
        batch_result?;

        info!("✅ Hybrid execution strategy completed successfully");
        Ok(())
    }

    /// Execute opportunity in paper trading mode
    async fn execute_paper_trading_opportunity(
        &self,
        opportunity: &MultiHopArbOpportunity,
        _paper_engine: &crate::paper_trading::SimulatedExecutionEngine,
    ) -> Result<(), ArbError> {
        debug!("📄 Executing opportunity in paper trading mode");

        // Simplified paper trading execution for now
        info!(
            "📄 Paper trade executed: estimated profit ${:.2}",
            opportunity.total_profit
        );

        Ok(())
    }

    /// Estimate execution cost for an opportunity
    async fn estimate_execution_cost(
        &self,
        opportunity: &MultiHopArbOpportunity,
    ) -> Result<f64, ArbError> {
        // Simplified cost estimation
        let base_cost = 0.0001; // Base transaction cost in SOL
        let hop_cost = opportunity.hops.len() as f64 * 0.00005; // Additional cost per hop

        Ok(base_cost + hop_cost)
    }
}
