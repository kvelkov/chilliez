// src/arbitrage/execution_manager.rs
use crate::{
    arbitrage::{
        opportunity::MultiHopArbOpportunity,
        execution::{BatchExecutor, HftExecutor},
        types::{CompetitivenessAnalysis, ExecutionRecommendation, ExecutionStrategy},
    },
    config::settings::Config,
    error::ArbError,
    paper_trading::{PaperTradingAnalytics, PaperTradingReporter, SafeVirtualPortfolio, SimulatedExecutionEngine},
    solana::BalanceMonitor,
};
use log::{debug, info, warn};
use rust_decimal::prelude::*;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    collections::HashMap,
};
use tokio::sync::{mpsc, Mutex};

/// Execution manager responsible for trade execution and safety
pub struct ExecutionManager {
    pub executor: Option<Arc<HftExecutor>>,
    pub batch_execution_engine: Option<Arc<BatchExecutor>>,
    pub execution_enabled: Arc<AtomicBool>,
    pub opportunity_sender: Option<mpsc::UnboundedSender<MultiHopArbOpportunity>>, // not in use - Initialized but not used within ExecutionManager's methods.
    pub balance_monitor: Option<Arc<BalanceMonitor>>,
    
    // Paper trading components
    pub paper_trading_engine: Option<Arc<SimulatedExecutionEngine>>,
    pub paper_trading_portfolio: Option<Arc<SafeVirtualPortfolio>>,
    pub paper_trading_analytics: Option<Arc<Mutex<PaperTradingAnalytics>>>,
    pub paper_trading_reporter: Option<Arc<PaperTradingReporter>>, // not in use - Initialized but not used within ExecutionManager's methods.
    pub is_paper_trading: bool,
}

impl ExecutionManager {
    pub fn new(
        executor: Option<Arc<HftExecutor>>,
        batch_execution_engine: Option<Arc<BatchExecutor>>,
        balance_monitor: Option<Arc<BalanceMonitor>>,
        paper_trading_engine: Option<Arc<SimulatedExecutionEngine>>,
        paper_trading_portfolio: Option<Arc<SafeVirtualPortfolio>>,
        paper_trading_analytics: Option<Arc<Mutex<PaperTradingAnalytics>>>,
        paper_trading_reporter: Option<Arc<PaperTradingReporter>>,
        config: &Config,
    ) -> Self {
        let (opportunity_sender, _opportunity_receiver) = mpsc::unbounded_channel();
        let is_paper_trading = config.paper_trading;

        Self {
            executor,
            batch_execution_engine,
            execution_enabled: Arc::new(AtomicBool::new(true)),
            opportunity_sender: Some(opportunity_sender),
            balance_monitor,
            paper_trading_engine,
            paper_trading_portfolio,
            paper_trading_analytics,
            paper_trading_reporter,
            is_paper_trading,
        }
    }

    /// Analyze competitiveness and determine execution strategy
    pub async fn analyze_competitiveness(
        &self,
        opportunities: &[MultiHopArbOpportunity],
    ) -> Result<CompetitivenessAnalysis, ArbError> {
        debug!("🎯 Analyzing competitiveness for {} opportunities", opportunities.len());

        if opportunities.is_empty() {
            return Ok(CompetitivenessAnalysis {
                competitive_score: Decimal::ZERO,
                risk_factors: vec!["No opportunities to analyze".to_string()],
                execution_recommendation: ExecutionRecommendation::SafeToBatch,
                reason: "No opportunities available".to_string(),
            });
        }

        // Calculate average expected profit
        let total_profit: Decimal = opportunities.iter()
            .map(|opp| opp.expected_profit_usd)
            .sum();
        let avg_profit = total_profit / Decimal::new(opportunities.len() as i64, 0);

        // Simple competitiveness scoring
        let competitive_score = if avg_profit > Decimal::new(100, 0) {
            Decimal::new(90, 0) // High profit - very competitive
        } else if avg_profit > Decimal::new(50, 0) {
            Decimal::new(70, 0) // Medium profit - moderately competitive
        } else {
            Decimal::new(40, 0) // Low profit - less competitive
        };

        let (recommendation, reason) = if avg_profit > Decimal::new(100, 0) {
            (ExecutionRecommendation::ImmediateSingle, 
             "High profit opportunities require immediate execution".to_string())
        } else {
            (ExecutionRecommendation::SafeToBatch,
             "Lower profit opportunities can be batched for efficiency".to_string())
        };

        Ok(CompetitivenessAnalysis {
            competitive_score,
            risk_factors: vec!["Market volatility".to_string(), "Gas costs".to_string()],
            execution_recommendation: recommendation,
            reason,
        })
    }

    /// Determine execution strategy based on opportunities and competitiveness
    pub async fn determine_execution_strategy(
        &self,
        opportunities: Vec<MultiHopArbOpportunity>,
    ) -> Result<ExecutionStrategy, ArbError> {
        if opportunities.is_empty() {
            debug!("🎯 No opportunities to execute");
            return Ok(ExecutionStrategy::SingleExecution(vec![]));
        }

        let analysis = self.analyze_competitiveness(&opportunities).await?;
        
        match analysis.execution_recommendation {
            ExecutionRecommendation::ImmediateSingle => {
                info!("⚡ Using immediate single execution strategy: {}", analysis.reason);
                Ok(ExecutionStrategy::SingleExecution(opportunities))
            }
            ExecutionRecommendation::SafeToBatch => {
                info!("📦 Using batch execution strategy: {}", analysis.reason);
                // For simplicity, treat all as batchable for now
                Ok(ExecutionStrategy::BatchExecution(opportunities, vec![]))
            }
        }
    }

    /// Execute opportunities using the determined strategy
    pub async fn execute_opportunities(
        &self,
        opportunities: Vec<MultiHopArbOpportunity>,
    ) -> Result<(), ArbError> {
        if !self.execution_enabled.load(Ordering::Relaxed) {
            info!("⏸️ Execution disabled, skipping {} opportunities", opportunities.len());
            return Ok(());
        }

        if opportunities.is_empty() {
            return Ok(());
        }

        // Check balance safety if monitor is available
        if let Some(balance_monitor) = &self.balance_monitor {
            if balance_monitor.is_safety_mode_active() {
                warn!("🚨 Safety mode active - blocking execution");
                return Err(ArbError::Safety("Balance monitor in safety mode".to_string()));
            }
        }

        let strategy = self.determine_execution_strategy(opportunities).await?;

        match strategy {
            ExecutionStrategy::SingleExecution(opps) => {
                self.execute_single_strategy(opps).await
            }
            ExecutionStrategy::BatchExecution(batchable, immediate) => {
                self.execute_batch_strategy(batchable, immediate).await
            }
            ExecutionStrategy::HybridExecution { immediate, batchable } => {
                self.execute_hybrid_strategy(immediate, batchable).await
            }
        }
    }

    /// Execute using single execution strategy
    async fn execute_single_strategy(
        &self,
        opportunities: Vec<MultiHopArbOpportunity>,
    ) -> Result<(), ArbError> {
        info!("⚡ Executing {} opportunities using single strategy", opportunities.len());

        for opportunity in opportunities {
            if self.is_paper_trading {
                self.execute_paper_trade(&opportunity).await?;
            } else {
                self.execute_real_trade(&opportunity).await?;
            }
        }

        Ok(())
    }

    /// Execute using batch strategy
    async fn execute_batch_strategy(
        &self,
        batchable: Vec<MultiHopArbOpportunity>,
        immediate: Vec<MultiHopArbOpportunity>,
    ) -> Result<(), ArbError> {
        info!("📦 Executing batch strategy: {} batchable, {} immediate", 
              batchable.len(), immediate.len());

        // Execute immediate opportunities first
        for opportunity in immediate {
            if self.is_paper_trading {
                self.execute_paper_trade(&opportunity).await?;
            } else {
                self.execute_real_trade(&opportunity).await?;
            }
        }

        // Batch execute remaining opportunities
        if !batchable.is_empty() {
            if self.is_paper_trading {
                for opportunity in batchable {
                    self.execute_paper_trade(&opportunity).await?;
                }
            } else {
                self.execute_batch_real_trades(batchable).await?;
            }
        }

        Ok(())
    }

    /// Execute using hybrid strategy
    async fn execute_hybrid_strategy(
        &self,
        immediate: Vec<MultiHopArbOpportunity>,
        batchable: Vec<MultiHopArbOpportunity>,
    ) -> Result<(), ArbError> {
        info!("🔄 Executing hybrid strategy: {} immediate, {} batchable", 
              immediate.len(), batchable.len());

        // Same as batch strategy for now
        self.execute_batch_strategy(batchable, immediate).await
    }

    /// Execute a single paper trade
    async fn execute_paper_trade(&self, opportunity: &MultiHopArbOpportunity) -> Result<(), ArbError> {
        if let Some(engine) = &self.paper_trading_engine {
            match engine.simulate_arbitrage_execution(opportunity).await {
                Ok(result) => {
                    info!("📄 Paper trade executed: ${:.2} profit", result.realized_profit_usd);
                    
                    // Update analytics
                    if let Some(analytics) = &self.paper_trading_analytics {
                        let mut analytics_guard = analytics.lock().await;
                        analytics_guard.record_trade(result);
                    }
                    
                    Ok(())
                }
                Err(e) => {
                    warn!("📄 Paper trade failed: {}", e);
                    Err(e)
                }
            }
        } else {
            Err(ArbError::Configuration("Paper trading engine not initialized".to_string()))
        }
    }

    /// Execute a single real trade
    async fn execute_real_trade(&self, opportunity: &MultiHopArbOpportunity) -> Result<(), ArbError> {
        if let Some(executor) = &self.executor {
            info!("💰 Executing real trade for ${:.2} expected profit", opportunity.expected_profit_usd);
            executor.execute_arbitrage(opportunity).await
        } else {
            Err(ArbError::Configuration("Real trade executor not initialized".to_string()))
        }
    }

    /// Execute multiple real trades in batch
    async fn execute_batch_real_trades(&self, opportunities: Vec<MultiHopArbOpportunity>) -> Result<(), ArbError> {
        if let Some(batch_executor) = &self.batch_execution_engine {
            info!("📦 Executing {} trades in batch", opportunities.len());
            batch_executor.execute_batch(opportunities).await
        } else {
            // Fall back to individual execution
            warn!("📦 Batch executor not available, falling back to individual execution");
            for opportunity in opportunities {
                self.execute_real_trade(&opportunity).await?;
            }
            Ok(())
        }
    }

    /// Enable or disable execution
    pub fn set_execution_enabled(&self, enabled: bool) {
        self.execution_enabled.store(enabled, Ordering::Relaxed);
        if enabled {
            info!("✅ Execution enabled");
        } else {
            info!("⏸️ Execution disabled");
        }
    }

    /// Check if execution is enabled
    pub fn is_execution_enabled(&self) -> bool {
        self.execution_enabled.load(Ordering::Relaxed)
    }
}
