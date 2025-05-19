use log::info;
use prometheus::{register_counter, register_gauge, register_histogram, Counter, Gauge, Histogram};
use serde_json::json;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::arbitrage::opportunity::ArbOpportunity;
use crate::dex::pool::Pool;
use crate::error::ArbError;
pub struct Metrics {
    log_file: Arc<Mutex<std::fs::File>>,

    // Prometheus metrics
    opportunities_detected: Counter,
    opportunities_executed: Counter,
    opportunities_failed: Counter,
    profit_total: Gauge,
    execution_time: Histogram,
    active_pools: Gauge,
}

impl Metrics {
    pub fn new(log_path: &str) -> Result<Self, ArbError> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .map_err(|e| ArbError::Unknown(format!("Failed to open log file: {}", e)))?;

        // Register Prometheus metrics
        let opportunities_detected = register_counter!(
            "arb_opportunities_detected",
            "Total arbitrage opportunities detected"
        )
        .unwrap();
        let opportunities_executed = register_counter!(
            "arb_opportunities_executed",
            "Total arbitrage opportunities executed"
        )
        .unwrap();
        let opportunities_failed = register_counter!(
            "arb_opportunities_failed",
            "Total arbitrage opportunities that failed execution"
        )
        .unwrap();
        let profit_total = register_gauge!("arb_profit_total", "Total profit in USD").unwrap();
        let execution_time =
            register_histogram!("arb_execution_time", "Time to execute arbitrage in ms").unwrap();
        let active_pools =
            register_gauge!("arb_active_pools", "Number of active pools being monitored").unwrap();

        Ok(Self {
            log_file: Arc::new(Mutex::new(file)),
            opportunities_detected,
            opportunities_executed,
            opportunities_failed,
            profit_total,
            execution_time,
            active_pools,
        })
    }

    pub fn log_pools_fetched(&self, count: usize) {
        info!("Fetched {} pools from DEXs", count);
    }

    pub async fn log_opportunity(&self, opportunity: &ArbOpportunity) -> Result<(), ArbError> {
        // Increment counter
        self.opportunities_detected.inc();

        // Log to file
        let log_entry = json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "event": "opportunity_detected",
            "input_token": opportunity.input_token.to_string(),
            "output_token": opportunity.output_token.to_string(),
            "expected_profit_percent": opportunity.expected_profit_percent,
            "input_amount": opportunity.input_amount.to_string(),
            "expected_output_amount": opportunity.expected_output_amount.to_string(),
            "dex": format!("{:?}", opportunity.dex),
        });

        let mut file = self.log_file.lock().await;
        writeln!(file, "{}", log_entry.to_string())
            .map_err(|e| ArbError::Unknown(format!("Failed to write to log file: {}", e)))?;

        Ok(())
    }

    pub async fn log_execution(
        &self,
        opportunity: &ArbOpportunity,
        success: bool,
        execution_time_ms: u64,
        actual_profit: Option<f64>,
    ) -> Result<(), ArbError> {
        // Update metrics
        if success {
            self.opportunities_executed.inc();
            if let Some(profit) = actual_profit {
                self.profit_total.add(profit);
            }
        } else {
            self.opportunities_failed.inc();
        }

        self.execution_time.observe(execution_time_ms as f64);

        // Log to file
        let log_entry = json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "event": if success { "execution_success" } else { "execution_failed" },
            "input_token": opportunity.input_token.to_string(),
            "output_token": opportunity.output_token.to_string(),
            "expected_profit_percent": opportunity.expected_profit_percent,
            "actual_profit": actual_profit,
            "execution_time_ms": execution_time_ms,
            "dex": format!("{:?}", opportunity.dex),
        });

        let mut file = self.log_file.lock().await;
        writeln!(file, "{}", log_entry.to_string())
            .map_err(|e| ArbError::Unknown(format!("Failed to write to log file: {}", e)))?;

        Ok(())
    }

    pub fn update_active_pools(&self, count: usize) {
        self.active_pools.set(count as f64);
    }
}
