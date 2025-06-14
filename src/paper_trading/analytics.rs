// src/paper_trading/analytics.rs
//! Analytics and performance tracking for paper trading

use super::{
    engine::SimulatedTradeResult,
    portfolio::{PortfolioSummary, VirtualPortfolio},
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

/// Performance analytics for paper trading
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperTradingAnalytics {
    /// Total number of opportunities analyzed
    pub opportunities_analyzed: u64,
    
    /// Number of opportunities executed
    pub opportunities_executed: u64,
    
    /// Number of successful executions
    pub successful_executions: u64,
    
    /// Total profit/loss by token
    pub pnl_by_token: HashMap<Pubkey, i64>,
    
    /// Total fees paid
    pub total_fees_paid: u64,
    
    /// Average execution time in milliseconds
    pub avg_execution_time_ms: f64,
    
    /// Best trade profit (in basis points)
    pub best_trade_profit_bps: u16,
    
    /// Worst trade loss (in basis points)
    pub worst_trade_loss_bps: u16,
    
    /// Average slippage experienced
    pub avg_slippage_bps: f64,
    
    /// Execution rate (executed / analyzed)
    pub execution_rate: f64,
    
    /// Success rate (successful / executed)
    pub success_rate: f64,
    
    /// Total uptime in seconds
    pub uptime_seconds: u64,
    
    /// Trades per hour
    pub trades_per_hour: f64,
    
    /// Performance by DEX
    pub performance_by_dex: HashMap<String, DexPerformance>,
    
    /// Analysis start time
    pub start_time: u64,
    
    /// Last update time
    pub last_update: u64,
    
    /// Largest profit recorded
    pub largest_win: i64,
    
    /// Largest loss recorded
    pub largest_loss: i64,
    
    /// Total profit and loss
    pub total_pnl: i64,
    
    /// Execution history for performance analysis
    pub execution_history: Vec<(chrono::DateTime<chrono::Utc>, i64)>,
    
    /// Failed executions count
    pub failed_executions: u64,
    
    /// Failure reasons count
    pub failure_reasons: HashMap<String, u64>,
}

/// Performance metrics for a specific DEX
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DexPerformance {
    pub trades_executed: u64,
    pub successful_trades: u64,
    pub total_profit_loss: i64,
    pub avg_slippage_bps: f64,
    pub avg_execution_time_ms: f64,
    pub success_rate: f64,
}

/// Individual trade record for detailed analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRecord {
    pub timestamp: u64,
    pub opportunity_id: String,
    pub input_mint: Pubkey,
    pub output_mint: Pubkey,
    pub input_amount: u64,
    pub output_amount: u64,
    pub expected_output: u64,
    pub profit_loss_bps: i16,
    pub slippage_bps: u16,
    pub execution_time_ms: u64,
    pub dex_name: String,
    pub success: bool,
    pub error_message: Option<String>,
}

impl PaperTradingAnalytics {
    /// Create new analytics instance
    pub fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
            
        Self {
            opportunities_analyzed: 0,
            opportunities_executed: 0,
            successful_executions: 0,
            pnl_by_token: HashMap::new(),
            total_fees_paid: 0,
            avg_execution_time_ms: 0.0,
            best_trade_profit_bps: 0,
            worst_trade_loss_bps: 0,
            avg_slippage_bps: 0.0,
            execution_rate: 0.0,
            success_rate: 0.0,
            uptime_seconds: 0,
            trades_per_hour: 0.0,
            performance_by_dex: HashMap::new(),
            start_time: now,
            last_update: now,
            largest_win: 0,
            largest_loss: 0,
            total_pnl: 0,
            execution_history: Vec::new(),
            failed_executions: 0,
            failure_reasons: HashMap::new(),
        }
    }
    
    /// Record an analyzed opportunity (not necessarily executed)
    pub fn record_opportunity_analyzed(&mut self) {
        self.opportunities_analyzed += 1;
        self.update_rates();
    }
    
    /// Record an executed trade
    pub fn record_trade_execution(&mut self, result: &SimulatedTradeResult, dex_name: &str) {
        self.opportunities_executed += 1;
        
        if result.success {
            self.successful_executions += 1;
        }
        
        // Update execution time average
        let total_time = self.avg_execution_time_ms * (self.opportunities_executed - 1) as f64;
        self.avg_execution_time_ms = (total_time + result.execution_time_ms as f64) / self.opportunities_executed as f64;
        
        // Update slippage average
        let total_slippage = self.avg_slippage_bps * (self.opportunities_executed - 1) as f64;
        self.avg_slippage_bps = (total_slippage + result.slippage_bps as f64) / self.opportunities_executed as f64;
        
        // Update best/worst trades
        if result.success {
            let profit_bps = self.calculate_profit_bps(result.input_amount, result.output_amount);
            if profit_bps > 0 && profit_bps > self.best_trade_profit_bps as i16 {
                self.best_trade_profit_bps = profit_bps as u16;
            } else if profit_bps < 0 && (-profit_bps) > self.worst_trade_loss_bps as i16 {
                self.worst_trade_loss_bps = (-profit_bps) as u16;
            }
        }
        
        // Update DEX performance
        self.update_dex_performance(dex_name, result);
        
        // Update fees
        self.total_fees_paid += result.fee_amount;
        
        self.update_rates();
        self.update_timestamp();
    }
    
    /// Record a successful execution
    pub fn record_successful_execution(&mut self, input_amount: u64, output_amount: u64, profit_loss: i64, fees: u64) {
        self.successful_executions += 1;
        self.opportunities_executed += 1;
        self.total_fees_paid += fees;
        
        // Update P&L tracking
        if profit_loss > 0 && profit_loss > self.largest_win {
            self.largest_win = profit_loss;
        }
        if profit_loss < 0 && profit_loss < self.largest_loss {
            self.largest_loss = profit_loss;
        }
        
        self.total_pnl += profit_loss;
        
        // Record in execution history for analysis
        let execution_time = chrono::Utc::now();
        self.execution_history.push((execution_time, profit_loss));
        
        // Limit history size to prevent memory growth
        if self.execution_history.len() > 10000 {
            self.execution_history.remove(0);
        }
    }
    
    /// Record a failed execution
    pub fn record_failed_execution(&mut self, input_amount: u64, fees: u64, reason: String) {
        self.failed_executions += 1;
        self.opportunities_executed += 1;
        self.total_fees_paid += fees;
        
        // Record failure reason
        *self.failure_reasons.entry(reason).or_insert(0) += 1;
    }
    
    /// Update from portfolio summary
    pub fn update_from_portfolio(&mut self, summary: &PortfolioSummary) {
        self.pnl_by_token = summary.pnl_by_token.clone();
        self.total_fees_paid = summary.total_fees_paid;
        self.uptime_seconds = summary.uptime_seconds;
        
        // Calculate trades per hour
        if self.uptime_seconds > 0 {
            self.trades_per_hour = (summary.total_trades as f64) * 3600.0 / (self.uptime_seconds as f64);
        }
        
        self.update_timestamp();
    }
    
    /// Calculate profit in basis points
    fn calculate_profit_bps(&self, input_amount: u64, output_amount: u64) -> i16 {
        if input_amount == 0 {
            return 0;
        }
        
        let profit_ratio = (output_amount as f64 / input_amount as f64) - 1.0;
        (profit_ratio * 10_000.0) as i16
    }
    
    /// Update DEX-specific performance metrics
    fn update_dex_performance(&mut self, dex_name: &str, result: &SimulatedTradeResult) {
        // Calculate profit before mutable borrow to avoid borrow checker issues
        let profit_bps = if result.success {
            self.calculate_profit_bps(result.input_amount, result.output_amount)
        } else {
            0
        };
        
        let perf = self.performance_by_dex.entry(dex_name.to_string()).or_insert(DexPerformance {
            trades_executed: 0,
            successful_trades: 0,
            total_profit_loss: 0,
            avg_slippage_bps: 0.0,
            avg_execution_time_ms: 0.0,
            success_rate: 0.0,
        });
        
        perf.trades_executed += 1;
        if result.success {
            perf.successful_trades += 1;
            perf.total_profit_loss += profit_bps as i64;
        }
        
        // Update averages
        let total_slippage = perf.avg_slippage_bps * (perf.trades_executed - 1) as f64;
        perf.avg_slippage_bps = (total_slippage + result.slippage_bps as f64) / perf.trades_executed as f64;
        
        let total_time = perf.avg_execution_time_ms * (perf.trades_executed - 1) as f64;
        perf.avg_execution_time_ms = (total_time + result.execution_time_ms as f64) / perf.trades_executed as f64;
        
        perf.success_rate = if perf.trades_executed > 0 {
            (perf.successful_trades as f64 / perf.trades_executed as f64) * 100.0
        } else {
            0.0
        };
    }
    
    /// Update calculated rates
    fn update_rates(&mut self) {
        self.execution_rate = if self.opportunities_analyzed > 0 {
            (self.opportunities_executed as f64 / self.opportunities_analyzed as f64) * 100.0
        } else {
            0.0
        };
        
        self.success_rate = if self.opportunities_executed > 0 {
            (self.successful_executions as f64 / self.opportunities_executed as f64) * 100.0
        } else {
            0.0
        };
    }
    
    /// Update timestamp
    fn update_timestamp(&mut self) {
        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        self.uptime_seconds = self.last_update.saturating_sub(self.start_time);
    }
    
    /// Get summary statistics
    pub fn get_summary(&self) -> AnalyticsSummary {
        let total_pnl: i64 = self.pnl_by_token.values().sum();
        
        AnalyticsSummary {
            total_opportunities: self.opportunities_analyzed,
            total_executions: self.opportunities_executed,
            successful_executions: self.successful_executions,
            execution_rate: self.execution_rate,
            success_rate: self.success_rate,
            total_pnl,
            total_fees_paid: self.total_fees_paid,
            avg_execution_time_ms: self.avg_execution_time_ms,
            best_trade_profit_bps: self.best_trade_profit_bps,
            worst_trade_loss_bps: self.worst_trade_loss_bps,
            trades_per_hour: self.trades_per_hour,
            uptime_hours: self.uptime_seconds as f64 / 3600.0,
        }
    }
    
    /// Get the success rate of executed trades
    pub fn get_success_rate(&self) -> f64 {
        if self.opportunities_executed > 0 {
            self.successful_executions as f64 / self.opportunities_executed as f64
        } else {
            0.0
        }
    }
    
    /// Get average profit per trade in basis points
    pub fn get_average_profit_per_trade(&self) -> f64 {
        if self.successful_executions > 0 {
            let total_profit: i64 = self.pnl_by_token.values().sum();
            total_profit as f64 / self.successful_executions as f64
        } else {
            0.0
        }
    }
    
    /// Calculate Sharpe ratio for the trading strategy
    pub fn calculate_sharpe_ratio(&self) -> f64 {
        // Simplified Sharpe ratio calculation
        // In a real implementation, this would use variance of returns
        if self.successful_executions > 0 {
            let avg_profit = self.get_average_profit_per_trade();
            let volatility = 100.0; // Simplified constant volatility
            avg_profit / volatility
        } else {
            0.0
        }
    }
    
    /// Get maximum drawdown experienced
    pub fn get_max_drawdown(&self) -> f64 {
        // Simplified implementation - in reality would track running drawdown
        let min_loss: i64 = self.pnl_by_token.values().min().cloned().unwrap_or(0);
        if min_loss < 0 {
            (-min_loss) as f64
        } else {
            0.0
        }
    }

    // ...existing methods...
}

/// Summary of analytics for reporting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsSummary {
    pub total_opportunities: u64,
    pub total_executions: u64,
    pub successful_executions: u64,
    pub execution_rate: f64,
    pub success_rate: f64,
    pub total_pnl: i64,
    pub total_fees_paid: u64,
    pub avg_execution_time_ms: f64,
    pub best_trade_profit_bps: u16,
    pub worst_trade_loss_bps: u16,
    pub trades_per_hour: f64,
    pub uptime_hours: f64,
}

impl Default for PaperTradingAnalytics {
    fn default() -> Self {
        Self::new()
    }
}
