use log::{info, warn};
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

/// Simple tuple struct for representing a trading pair, public for cross-module use.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct TradingPair(pub String, pub String);

pub struct Metrics {
    pub launch_time: u64,
    pub pools_fetched: usize,
    pub trading_pairs: HashSet<(String, String)>,
    pub opportunities_found: usize,
    pub trades_executed: usize,
    pub successful_trades: usize,
    pub failed_trades: usize,
    pub total_profit: f64,
    pub total_loss: f64,
    pub total_fees: f64,
    pub total_tx_cost: f64,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            launch_time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            pools_fetched: 0,
            trading_pairs: HashSet::new(),
            opportunities_found: 0,
            trades_executed: 0,
            successful_trades: 0,
            failed_trades: 0,
            total_profit: 0.0,
            total_loss: 0.0,
            total_fees: 0.0,
            total_tx_cost: 0.0,
        }
    }

    pub fn log_launch(&self) {
        info!("🚀 Bot launched at UNIX time: {}", self.launch_time);
    }

    pub fn log_pools_fetched(&mut self, count: usize) {
        self.pools_fetched = count;
        info!("✅ Fetched {} pools from all DEXes", count);
    }

    pub fn log_trading_pairs(&mut self, pairs: &[(String, String)]) {
        for (a, b) in pairs {
            self.trading_pairs.insert((a.clone(), b.clone()));
        }
        info!("📊 Trading pairs available ({})", self.trading_pairs.len());
    }

    pub fn record_arbitrage_opportunity(
        &mut self,
        profit_percent: f64,
        input_token: &str,
        output_token: &str,
        input_amt: f64,
        expected_output: f64,
    ) {
        self.opportunities_found += 1;
        info!(
            "💡 Arbitrage Opportunity: {:.2}% | {} -> {} | input: {:.4} | output: {:.4}",
            profit_percent, input_token, output_token, input_amt, expected_output,
        );
    }

    pub fn record_execution_result(&mut self, success: bool, tx_cost: f64, fees: f64) {
        self.trades_executed += 1;
        self.total_tx_cost += tx_cost;
        self.total_fees += fees;

        if success {
            self.successful_trades += 1;
            info!(
                "✅ Execution result: Success | tx cost: {:.5} | fees: {:.5}",
                tx_cost, fees
            );
        } else {
            self.failed_trades += 1;
            warn!(
                "❌ Execution result: FAIL | tx cost: {:.5} | fees: {:.5}",
                tx_cost, fees
            );
        }
    }

    pub fn log_trade_result(&mut self, pair: (&str, &str), profit: f64, simulated: bool) {
        if profit >= 0.0 {
            self.total_profit += profit;
            info!(
                "✅ {}TRADE WIN: {}-{} | P/L: +{:.6} | Total P: {:.6}",
                if simulated { "[SIM] " } else { "" },
                pair.0,
                pair.1,
                profit,
                self.total_profit
            );
        } else {
            self.total_loss += profit.abs();
            warn!(
                "❌ {}TRADE LOSS: {}-{} | P/L: -{:.6} | Total L: {:.6}",
                if simulated { "[SIM] " } else { "" },
                pair.0,
                pair.1,
                profit.abs(),
                self.total_loss
            );
        }
    }

    /// Logging function for arbitrage opportunities, used by engine & detector modules.
    pub fn log_opportunity(&mut self, pair: &TradingPair, profit: f64) {
        self.opportunities_found += 1;
        info!(
            "🔎 Arbitrage Opportunity: {} -> {} | Profit: {:.4}%",
            pair.0, pair.1, profit
        );
    }

    pub fn summary(&self) {
        info!("=== SESSION SUMMARY ===");
        info!("Pools fetched: {}", self.pools_fetched);
        info!("Trading pairs: {}", self.trading_pairs.len());
        info!("Opportunities found: {}", self.opportunities_found);
        info!("Trades executed: {}", self.trades_executed);
        info!("Successful trades: {}", self.successful_trades);
        info!("Failed trades: {}", self.failed_trades);
        info!("Total profit: {:.6}", self.total_profit);
        info!("Total loss: {:.6}", self.total_loss);
        info!("Total fees: {:.6}", self.total_fees);
        info!("Total tx cost: {:.6}", self.total_tx_cost);
        info!("=======================");
    }
}
