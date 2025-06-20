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
    /// DEX-specific error details (optional, for edge case tracking)
    pub dex_error_details: Option<String>,
    /// Rent paid for this trade (lamports)
    pub rent_paid: u64,
    /// Account creation fees paid for this trade (lamports)
    pub account_creation_fees: u64,
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
    pub dex_breakdown: Vec<DexBreakdown>,
    pub token_pair_breakdown: Vec<TokenPairBreakdown>,
    pub percentile_metrics: PercentileMetrics,
    pub top_trades: Vec<TopTrade>,
    pub worst_trades: Vec<TopTrade>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DexBreakdown {
    pub dex: String,
    pub trades: u64,
    pub successful_trades: u64,
    pub total_profit_loss: i64,
    pub avg_slippage_bps: f64,
    pub avg_execution_time_ms: f64,
    pub success_rate: f64,
    pub total_fees_paid: u64,
    pub rent_paid: u64,
    pub account_creation_fees: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPairBreakdown {
    pub token_in: String,
    pub token_out: String,
    pub trades: u64,
    pub total_profit_loss: i64,
    pub success_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PercentileMetrics {
    pub p95_slippage_bps: f64,
    pub p99_slippage_bps: f64,
    pub p95_pnl: f64,
    pub p99_pnl: f64,
    pub p95_execution_time_ms: f64,
    pub p99_execution_time_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopTrade {
    pub timestamp: DateTime<Utc>,
    pub profit_loss: i64,
    pub slippage_bps: f64,
    pub token_in: String,
    pub token_out: String,
    pub dex: String,
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
        dex_error_details: Option<String>,
        rent_paid: u64,
        account_creation_fees: u64,
    ) -> TradeLogEntry {
        TradeLogEntry {
            timestamp: Utc::now(),
            opportunity_id: format!("arb_{}", chrono::Utc::now().timestamp_millis()),
            token_in: format!("Token_{}", opportunity.input_token),
            token_out: format!("Token_{}", opportunity.output_token),
            amount_in,
            amount_out,
            expected_profit: (opportunity.total_profit * 1_000_000.0) as u64,
            actual_profit,
            slippage_applied,
            fees_paid,
            execution_success,
            failure_reason,
            dex_route: opportunity.pool_path.iter().map(|p| format!("Pool_{}", p)).collect(),
            gas_cost,
            dex_error_details,
            rent_paid,
            account_creation_fees,
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

        // DEX breakdown
        let dex_breakdown = analytics
            .performance_by_dex
            .iter()
            .map(
                |(dex, perf)| DexBreakdown {
                    dex: dex.clone(),
                    trades: perf.trades_executed,
                    successful_trades: perf.successful_trades,
                    total_profit_loss: perf.total_profit_loss,
                    avg_slippage_bps: perf.avg_slippage_bps,
                    avg_execution_time_ms: perf.avg_execution_time_ms,
                    success_rate: perf.success_rate,
                    total_fees_paid: 0, // TODO: aggregate per DEX if tracked
                    rent_paid: perf.rent_paid,
                    account_creation_fees: perf.account_creation_fees,
                },
            )
            .collect();
        // Token pair breakdown (from trade logs)
        let trade_logs = self.read_trade_logs().unwrap_or_default();
        let mut token_pair_map = std::collections::HashMap::new();
        for trade in &trade_logs {
            let key = (trade.token_in.clone(), trade.token_out.clone());
            let entry = token_pair_map.entry(key).or_insert((0u64, 0i64, 0u64));
            entry.0 += 1;
            entry.1 += trade.actual_profit;
            if trade.execution_success {
                entry.2 += 1;
            }
        }
        let token_pair_breakdown = token_pair_map
            .into_iter()
            .map(
                |((token_in, token_out), (trades, total_profit_loss, successful_trades))| {
                    TokenPairBreakdown {
                        token_in,
                        token_out,
                        trades,
                        total_profit_loss,
                        success_rate: if trades > 0 {
                            successful_trades as f64 / trades as f64 * 100.0
                        } else {
                            0.0
                        },
                    }
                },
            )
            .collect();
        // Percentile metrics
        let mut slippages: Vec<f64> = trade_logs.iter().map(|t| t.slippage_applied).collect();
        let mut pnls: Vec<f64> = trade_logs.iter().map(|t| t.actual_profit as f64).collect();
        let _exec_times: Vec<f64> = vec![]; // TODO: add execution time to TradeLogEntry if needed
        slippages.sort_by(|a, b| a.partial_cmp(b).unwrap());
        pnls.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p = |v: &Vec<f64>, pct: f64| -> f64 {
            if v.is_empty() {
                0.0
            } else {
                v[((v.len() as f64 * pct).ceil() as usize).saturating_sub(1)]
            }
        };
        let percentile_metrics = PercentileMetrics {
            p95_slippage_bps: p(&slippages, 0.95),
            p99_slippage_bps: p(&slippages, 0.99),
            p95_pnl: p(&pnls, 0.95),
            p99_pnl: p(&pnls, 0.99),
            p95_execution_time_ms: 0.0, // TODO: fill if available
            p99_execution_time_ms: 0.0, // TODO: fill if available
        };
        // Top N trades
        let mut sorted_trades = trade_logs.clone();
        sorted_trades.sort_by_key(|t| -t.actual_profit);
        let top_trades = sorted_trades
            .iter()
            .take(5)
            .map(
                |t| TopTrade {
                    timestamp: t.timestamp,
                    profit_loss: t.actual_profit,
                    slippage_bps: t.slippage_applied,
                    token_in: t.token_in.clone(),
                    token_out: t.token_out.clone(),
                    dex: t.dex_route.first().cloned().unwrap_or_default(),
                },
            )
            .collect();
        let worst_trades = sorted_trades
            .iter()
            .rev()
            .take(5)
            .map(
                |t| TopTrade {
                    timestamp: t.timestamp,
                    profit_loss: t.actual_profit,
                    slippage_bps: t.slippage_applied,
                    token_in: t.token_in.clone(),
                    token_out: t.token_out.clone(),
                    dex: t.dex_route.first().cloned().unwrap_or_default(),
                },
            )
            .collect();

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
            dex_breakdown,
            token_pair_breakdown,
            percentile_metrics,
            top_trades,
            worst_trades,
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
            "rent_paid",
            "account_creation_fees",
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
                trade.rent_paid.to_string(),
                trade.account_creation_fees.to_string(),
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
        println!("🏦 Portfolio Value: {} -> {} lamports", summary.portfolio_value_start, summary.portfolio_value_end);
        println!("📊 Portfolio Return: {:.2}%", summary.return_percentage);
        println!("📊 Average Profit/Trade: {:.2} lamports", summary.average_profit_per_trade);
        println!("🎯 Largest Win: {} lamports", summary.largest_win);
        println!("😞 Largest Loss: {} lamports", summary.largest_loss);
        if let Some(sharpe) = summary.sharpe_ratio {
            println!("📈 Sharpe Ratio: {:.4}", sharpe);
        }
        println!("📉 Max Drawdown: {:.2}%", summary.max_drawdown);
        println!("\n--- DEX Breakdown ---");
        for dex in &summary.dex_breakdown {
            println!(
                "  {}: trades={}, win_rate={:.2}%, P&L={}, fees={}, rent={}, acct_fees={}",
                dex.dex, dex.trades, dex.success_rate, dex.total_profit_loss, dex.total_fees_paid, dex.rent_paid, dex.account_creation_fees
            );
        }
        println!("\n--- Token Pair Breakdown ---");
        for pair in &summary.token_pair_breakdown {
            println!(
                "  {} -> {}: trades={}, win_rate={:.2}%, P&L={}",
                pair.token_in, pair.token_out, pair.trades, pair.success_rate, pair.total_profit_loss
            );
        }
        println!("\n--- Percentile Metrics ---");
        println!(
            "  95th slippage: {:.4}, 99th slippage: {:.4}, 95th P&L: {:.2}, 99th P&L: {:.2}",
            summary.percentile_metrics.p95_slippage_bps,
            summary.percentile_metrics.p99_slippage_bps,
            summary.percentile_metrics.p95_pnl,
            summary.percentile_metrics.p99_pnl
        );
        println!("\n--- Top 5 Trades ---");
        for t in &summary.top_trades {
            println!(
                "  [{}] {} -> {} on {}: P&L={}, slippage={:.4}",
                t.timestamp.format("%Y-%m-%d %H:%M:%S"), t.token_in, t.token_out, t.dex, t.profit_loss, t.slippage_bps
            );
        }
        println!("\n--- Worst 5 Trades ---");
        for t in &summary.worst_trades {
            println!(
                "  [{}] {} -> {} on {}: P&L={}, slippage={:.4}",
                t.timestamp.format("%Y-%m-%d %H:%M:%S"), t.token_in, t.token_out, t.dex, t.profit_loss, t.slippage_bps
            );
        }
        println!("================================================\n");
    }

    /// Print a live summary after each trade
    pub fn print_live_trade_summary(
        &self,
        analytics: &PaperTradingAnalytics,
        last_trade: &TradeLogEntry,
        portfolio: &VirtualPortfolio,
    ) {
        // Print balances (highlight SOL and USDC if present)
        let sol_mint = "So11111111111111111111111111111111111111112"; // Mainnet SOL mint
        let usdc_mint = "EPjFWdd5AufqSSqeM2q8VsJb9G9ZpwkRkzGwdrDam1w"; // Mainnet USDC mint
        println!(
            "[LIVE] {} {} -> {} | P&L: {} | Slippage: {:.4} | Success: {} | DEX: {} | Total P&L: {} | Trades: {} | Win Rate: {:.2}%",
            last_trade.timestamp.format("%H:%M:%S"),
            last_trade.token_in,
            last_trade.token_out,
            last_trade.actual_profit,
            last_trade.slippage_applied,
            last_trade.execution_success,
            last_trade.dex_route.first().cloned().unwrap_or_default(),
            analytics.total_pnl,
            analytics.opportunities_executed,
            analytics.get_success_rate()
        );
        println!("  Balances:");
        for (mint, balance) in portfolio.get_balances() {
            let mint_str = mint.to_string();
            if mint_str == sol_mint {
                println!("    SOL: {} lamports", balance);
            } else if mint_str == usdc_mint {
                println!("    USDC: {} micro-units", balance);
            } else {
                println!("    {}: {}", mint_str, balance);
            }
        }
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
            None,
            1000,
            2000,
        );

        assert_eq!(entry.amount_in, 1000000);
        assert_eq!(entry.amount_out, 1050000);
        assert_eq!(entry.actual_profit, 50000);
        assert!(entry.execution_success);
        assert_eq!(entry.rent_paid, 1000);
        assert_eq!(entry.account_creation_fees, 2000);
    }
}
