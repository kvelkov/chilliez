// src/paper_trading/reporter.rs
//! Reporting module for paper trading analytics and trade logs.

use super::{PaperTradingAnalytics, VirtualPortfolio};
use crate::arbitrage::opportunity::MultiHopArbOpportunity;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

/// Trade log entry for paper trading
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeLogEntry {
    pub timestamp: DateTime<Utc>,
    pub opportunity_id: String,
    pub token_in: String,
    pub token_out: String,
    pub amount_in: u64,
    pub amount_out: u64,
    pub expected_profit: u64,
    pub actual_profit: i64, // Can be negative
    pub slippage_applied: f64,
    pub fees_paid: u64,
    pub execution_success: bool,
    pub failure_reason: Option<String>,
    pub dex_route: Vec<String>,
    pub gas_cost: u64,
}

/// Performance summary for a trading session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceSummary {
    pub session_start: DateTime<Utc>,
    pub session_end: DateTime<Utc>,
    pub total_trades: u64,
    pub successful_trades: u64,
    pub failed_trades: u64,
    pub total_profit_loss: i64,
    pub total_fees_paid: u64,
    pub success_rate: f64,
    pub average_profit_per_trade: f64,
    pub largest_win: i64,
    pub largest_loss: i64,
    pub sharpe_ratio: Option<f64>,
    pub max_drawdown: f64,
    pub portfolio_value_start: u64,
    pub portfolio_value_end: u64,
    pub return_percentage: f64,
}

/// Paper trading reporter for exporting logs and analytics
pub struct PaperTradingReporter {
    trade_log_path: String,
    analytics_log_path: String,
    session_start: DateTime<Utc>,
}

impl PaperTradingReporter {
    /// Create a new paper trading reporter
    pub fn new(output_dir: &str) -> Result<Self> {
        // Ensure output directory exists
        std::fs::create_dir_all(output_dir)?;

        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let trade_log_path = format!("{}/paper_trades_{}.jsonl", output_dir, timestamp);
        let analytics_log_path = format!("{}/paper_analytics_{}.json", output_dir, timestamp);

        Ok(Self {
            trade_log_path,
            analytics_log_path,
            session_start: Utc::now(),
        })
    }

    /// Log a trade entry to the trade log file
    pub fn log_trade(&self, entry: TradeLogEntry) -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.trade_log_path)?;

        let json_line = serde_json::to_string(&entry)?;
        writeln!(file, "{}", json_line)?;
        file.flush()?;

        log::info!(
            "📝 Logged paper trade: {} {} -> {} (profit: {})",
            entry.opportunity_id,
            entry.token_in,
            entry.token_out,
            entry.actual_profit
        );

        Ok(())
    }

    /// Create a trade log entry from an opportunity and execution result
    pub fn create_trade_log_entry(
        opportunity: &MultiHopArbOpportunity,
        amount_in: u64,
        amount_out: u64,
        actual_profit: i64,
        slippage_applied: f64,
        fees_paid: u64,
        execution_success: bool,
        failure_reason: Option<String>,
        gas_cost: u64,
    ) -> TradeLogEntry {
        TradeLogEntry {
            timestamp: Utc::now(),
            opportunity_id: format!("arb_{}", chrono::Utc::now().timestamp_millis()),
            token_in: format!("Token_{}", opportunity.input_token), // Use actual token symbol
            token_out: format!("Token_{}", opportunity.output_token), // Use actual token symbol
            amount_in,
            amount_out,
            expected_profit: (opportunity.total_profit * 1_000_000.0) as u64, // Convert to lamports equivalent
            actual_profit,
            slippage_applied,
            fees_paid,
            execution_success,
            failure_reason,
            dex_route: opportunity
                .pool_path
                .iter()
                .map(|p| format!("Pool_{}", p))
                .collect(),
            gas_cost,
        }
    }

    /// Export analytics data to JSON file
    pub fn export_analytics(
        &self,
        analytics: &PaperTradingAnalytics,
        portfolio: &VirtualPortfolio,
    ) -> Result<()> {
        let performance_data = self.generate_performance_summary(analytics, portfolio);

        let json_data = serde_json::to_string_pretty(&performance_data)?;
        std::fs::write(&self.analytics_log_path, json_data)?;

        log::info!(
            "📊 Exported paper trading analytics to: {}",
            self.analytics_log_path
        );
        Ok(())
    }

    /// Generate performance summary from analytics and portfolio data
    fn generate_performance_summary(
        &self,
        analytics: &PaperTradingAnalytics,
        portfolio: &VirtualPortfolio,
    ) -> PerformanceSummary {
        let session_end = Utc::now();
        let balances = portfolio.get_balances();

        // Calculate portfolio values (simplified - would need actual token prices)
        let portfolio_value_start = 1_000_000_000; // 1 SOL equivalent in lamports (example)
        let portfolio_value_end: u64 = balances.values().sum();

        let return_percentage = if portfolio_value_start > 0 {
            ((portfolio_value_end as f64 - portfolio_value_start as f64)
                / portfolio_value_start as f64)
                * 100.0
        } else {
            0.0
        };

        PerformanceSummary {
            session_start: self.session_start,
            session_end,
            total_trades: analytics.opportunities_executed,
            successful_trades: analytics.successful_executions,
            failed_trades: analytics.failed_executions,
            total_profit_loss: analytics.total_pnl,
            total_fees_paid: analytics.total_fees_paid,
            success_rate: analytics.get_success_rate(),
            average_profit_per_trade: analytics.get_average_profit_per_trade(),
            largest_win: analytics.largest_win,
            largest_loss: analytics.largest_loss,
            sharpe_ratio: Some(analytics.calculate_sharpe_ratio()),
            max_drawdown: analytics.get_max_drawdown(),
            portfolio_value_start,
            portfolio_value_end,
            return_percentage,
        }
    }

    /// Export trade logs to CSV format
    pub fn export_trades_to_csv(&self, output_path: &str) -> Result<()> {
        // Read JSONL file and convert to CSV
        let trade_logs = self.read_trade_logs()?;

        let mut wtr = csv::Writer::from_path(output_path)?;

        // Write header
        wtr.write_record(&[
            "timestamp",
            "opportunity_id",
            "token_in",
            "token_out",
            "amount_in",
            "amount_out",
            "expected_profit",
            "actual_profit",
            "slippage_applied",
            "fees_paid",
            "execution_success",
            "failure_reason",
            "dex_route",
            "gas_cost",
        ])?;

        // Write trade data
        for trade in trade_logs {
            wtr.write_record(&[
                trade.timestamp.to_rfc3339(),
                trade.opportunity_id,
                trade.token_in,
                trade.token_out,
                trade.amount_in.to_string(),
                trade.amount_out.to_string(),
                trade.expected_profit.to_string(),
                trade.actual_profit.to_string(),
                trade.slippage_applied.to_string(),
                trade.fees_paid.to_string(),
                trade.execution_success.to_string(),
                trade.failure_reason.unwrap_or_default(),
                trade.dex_route.join("|"),
                trade.gas_cost.to_string(),
            ])?;
        }

        wtr.flush()?;
        log::info!("📈 Exported trade logs to CSV: {}", output_path);
        Ok(())
    }

    /// Read trade logs from the JSONL file
    fn read_trade_logs(&self) -> Result<Vec<TradeLogEntry>> {
        if !Path::new(&self.trade_log_path).exists() {
            return Ok(Vec::new());
        }

        let content = std::fs::read_to_string(&self.trade_log_path)?;
        let mut trades = Vec::new();

        for line in content.lines() {
            if let Ok(trade) = serde_json::from_str::<TradeLogEntry>(line) {
                trades.push(trade);
            }
        }

        Ok(trades)
    }

    /// Print performance summary to console
    pub fn print_performance_summary(
        &self,
        analytics: &PaperTradingAnalytics,
        portfolio: &VirtualPortfolio,
    ) {
        let summary = self.generate_performance_summary(analytics, portfolio);

        println!("\n🏆 === Paper Trading Performance Summary ===");
        println!(
            "📅 Session Duration: {} to {}",
            summary.session_start.format("%Y-%m-%d %H:%M:%S UTC"),
            summary.session_end.format("%Y-%m-%d %H:%M:%S UTC")
        );
        println!(
            "📊 Total Trades: {} (✅ {} successful, ❌ {} failed)",
            summary.total_trades, summary.successful_trades, summary.failed_trades
        );
        println!("📈 Success Rate: {:.2}%", summary.success_rate);
        println!("💰 Total P&L: {} lamports", summary.total_profit_loss);
        println!("💸 Total Fees: {} lamports", summary.total_fees_paid);
        println!(
            "📊 Average Profit/Trade: {:.2} lamports",
            summary.average_profit_per_trade
        );
        println!("🎯 Largest Win: {} lamports", summary.largest_win);
        println!("😞 Largest Loss: {} lamports", summary.largest_loss);
        println!("📊 Portfolio Return: {:.2}%", summary.return_percentage);
        if let Some(sharpe) = summary.sharpe_ratio {
            println!("📈 Sharpe Ratio: {:.4}", sharpe);
        }
        println!("📉 Max Drawdown: {:.2}%", summary.max_drawdown);
        println!(
            "💼 Portfolio Value: {} -> {} lamports",
            summary.portfolio_value_start, summary.portfolio_value_end
        );
        println!("================================================\n");
    }

    /// Get the paths to the generated log files
    pub fn get_log_paths(&self) -> (String, String) {
        (self.trade_log_path.clone(), self.analytics_log_path.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::{DexType, PoolInfo, PoolToken};
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn test_reporter_creation() {
        let temp_dir = tempdir().unwrap();
        let reporter = PaperTradingReporter::new(temp_dir.path().to_str().unwrap()).unwrap();

        let (trade_path, analytics_path) = reporter.get_log_paths();
        assert!(trade_path.contains("paper_trades_"));
        assert!(analytics_path.contains("paper_analytics_"));
    }

    #[test]
    fn test_trade_log_entry_creation() {
        // Create a mock opportunity
        use crate::arbitrage::opportunity::*;
        use solana_sdk::pubkey::Pubkey;

        let pool = PoolInfo {
            address: Pubkey::new_unique(), // Fix: Use 'address' instead of 'pool_address'
            name: "Test Pool".to_string(),
            token_a: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "TokenA".to_string(),
                decimals: 6,
                reserve: 1000000,
            },
            token_b: PoolToken {
                mint: Pubkey::new_unique(),
                symbol: "TokenB".to_string(),
                decimals: 6,
                reserve: 2000000,
            },
            token_a_vault: Pubkey::new_unique(),
            token_b_vault: Pubkey::new_unique(),
            fee_numerator: Some(3),
            fee_denominator: Some(1000),
            fee_rate_bips: Some(30),
            last_update_timestamp: 0,
            dex_type: DexType::Orca,
            liquidity: None,
            sqrt_price: None,
            tick_current_index: None,
            tick_spacing: None,
            tick_array_0: None,
            tick_array_1: None,
            tick_array_2: None,
            oracle: None,
        };

        let opportunity = MultiHopArbOpportunity {
            id: "test_opportunity".to_string(),
            hops: vec![], // Empty for this test
            total_profit: 100000.0,
            profit_pct: 10.0,
            input_token: "TokenA".to_string(),
            output_token: "TokenB".to_string(),
            input_amount: 1000000.0,
            expected_output: 1100000.0,
            dex_path: vec![DexType::Orca],
            pool_path: vec![pool.address],
            risk_score: Some(0.5),
            notes: Some("Test opportunity".to_string()),
            estimated_profit_usd: Some(100.0),
            input_amount_usd: Some(1000.0),
            output_amount_usd: Some(1100.0),
            intermediate_tokens: vec![],
            source_pool: Arc::new(pool.clone()),
            target_pool: Arc::new(pool.clone()),
            input_token_mint: Pubkey::new_unique(),
            output_token_mint: Pubkey::new_unique(),
            intermediate_token_mint: None,
            estimated_gas_cost: Some(5000),
            detected_at: Some(std::time::Instant::now()),
        };

        let entry = PaperTradingReporter::create_trade_log_entry(
            &opportunity,
            1000000,
            1050000,
            50000,
            0.01,
            5000,
            true,
            None,
            5000,
        );

        assert_eq!(entry.amount_in, 1000000);
        assert_eq!(entry.amount_out, 1050000);
        assert_eq!(entry.actual_profit, 50000);
        assert!(entry.execution_success);
    }
}
