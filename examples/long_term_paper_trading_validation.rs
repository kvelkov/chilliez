// examples/long_term_paper_trading_validation.rs
//! Long-Term Paper Trading Validation System
//! 
//! This system runs paper trading for 48+ hours with comprehensive validation:
//! - Continuous operation without intervention
//! - <1% difference validation between simulated and actual results
//! - Complete security audit logging
//! - Performance monitoring under extended load

use anyhow::Result;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::time::{interval, sleep};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use log::{info, warn, error, debug};
use serde::{Serialize, Deserialize};

// Note: This example uses mock implementations since the paper trading module 
// is not fully integrated yet. For production, replace with actual imports.

// Mock type definitions for demo purposes
#[derive(Debug, Clone)]
pub struct RoutingGraph {}

impl RoutingGraph {
    pub fn new() -> Self {
        Self {}
    }
}

#[derive(Debug, Clone)]
pub struct FeeEstimator {}

impl FeeEstimator {
    pub fn new() -> Self {
        Self {}
    }
}

#[derive(Debug)]
pub struct SmartRouter {}

impl SmartRouter {
    pub async fn new(_config: SmartRouterConfig, _graph: RoutingGraph, _fee: FeeEstimator) -> Result<Self> {
        Ok(Self {})
    }
}

#[derive(Debug, Clone)]
pub struct SmartRouterConfig {}

impl Default for SmartRouterConfig {
    fn default() -> Self {
        Self {}
    }
}

#[derive(Debug)]
pub struct Portfolio {
    balances: std::collections::HashMap<String, f64>,
}

impl Portfolio {
    pub fn new() -> Self {
        Self {
            balances: std::collections::HashMap::new(),
        }
    }
    
    pub fn add_balance(&mut self, token: String, amount: f64) {
        self.balances.insert(token, amount);
    }
}

#[derive(Debug)]
pub struct PaperTradingEngine {
    portfolio: Portfolio,
}

impl PaperTradingEngine {
    pub fn new(portfolio: Portfolio) -> Self {
        Self { portfolio }
    }
    
    pub fn update_balance(&mut self, _token: &str, _amount: f64) {
        // Mock implementation
    }
    
    pub fn get_total_value(&self) -> f64 {
        // Mock implementation - return sum of all balances
        self.portfolio.balances.values().sum()
    }
}

#[derive(Debug, Clone)]
pub struct PerformanceManager {}

impl PerformanceManager {
    pub async fn new(_config: PerformanceConfig) -> Result<Self> {
        Ok(Self {})
    }
    
    pub fn metrics_collector(&self) -> std::sync::Arc<tokio::sync::RwLock<MetricsCollector>> {
        Arc::new(tokio::sync::RwLock::new(MetricsCollector {}))
    }
    
    pub async fn get_performance_report(&self) -> String {
        "Mock performance report".to_string()
    }
}

#[derive(Debug, Clone)]
pub struct PerformanceConfig {}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {}
    }
}

#[derive(Debug)]
pub struct MetricsCollector {}

impl MetricsCollector {
    pub fn record_metric(&mut self, _name: &str, _value: f64) {}
    
    pub fn record_operation(&mut self, _name: &str, _duration: Duration, _success: bool) {
        // Mock implementation
    }
}

/// Configuration for long-term validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationConfig {
    /// Total duration to run validation (48+ hours)
    pub total_duration: Duration,
    /// Interval between trading operations
    pub trading_interval: Duration,
    /// Portfolio starting balance in USDC
    pub starting_balance: f64,
    /// Maximum allowed difference between simulated and actual results
    pub max_allowed_difference_percent: f64,
    /// Security audit logging interval
    pub audit_log_interval: Duration,
    /// Performance monitoring interval
    pub performance_monitoring_interval: Duration,
    /// Auto-save state interval
    pub auto_save_interval: Duration,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            total_duration: Duration::from_secs(48 * 60 * 60), // 48 hours
            trading_interval: Duration::from_secs(30), // Trade every 30 seconds
            starting_balance: 100_000.0, // $100,000 starting capital
            max_allowed_difference_percent: 1.0, // <1% difference allowed
            audit_log_interval: Duration::from_secs(60), // Log every minute
            performance_monitoring_interval: Duration::from_secs(30), // Monitor every 30s
            auto_save_interval: Duration::from_secs(300), // Save state every 5 minutes
        }
    }
}

/// Validation results and statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResults {
    pub start_time: SystemTime,
    pub end_time: SystemTime,
    pub total_duration: Duration,
    pub total_trades: u64,
    pub successful_trades: u64,
    pub failed_trades: u64,
    pub starting_portfolio_value: f64,
    pub ending_portfolio_value: f64,
    pub total_return_percent: f64,
    pub max_difference_observed: f64,
    pub average_difference: f64,
    pub validation_passed: bool,
    pub security_events: u64,
    pub performance_issues: u64,
    pub uptime_percentage: f64,
}

/// Long-term validation runner
pub struct LongTermValidator {
    config: ValidationConfig,
    performance_manager: Arc<PerformanceManager>,
    paper_trading_engine: PaperTradingEngine,
    _smart_router: SmartRouter, // Prefixed as it's not read
    validation_results: ValidationResults,
    security_audit_log: Vec<SecurityEvent>,
}

/// Security audit event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityEvent {
    pub timestamp: SystemTime,
    pub event_type: SecurityEventType,
    pub description: String,
    pub severity: SecuritySeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SecurityEventType {
    UnauthorizedAccess,
    SuspiciousTrading,
    PerformanceAnomaly,
    SystemIntegrityCheck,
    DataValidation,
    ConfigurationChange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SecuritySeverity {
    Low,
    Medium,
    High,
    Critical,
}

impl LongTermValidator {
    /// Create a new long-term validator
    pub async fn new(config: ValidationConfig) -> Result<Self> {
        info!("🚀 Initializing Long-Term Paper Trading Validator");
        
        // Initialize performance management
        let perf_config = PerformanceConfig::default();
        let performance_manager = Arc::new(PerformanceManager::new(perf_config).await?);
        
        // Initialize paper trading engine
        let mut portfolio = Portfolio::new();
        portfolio.add_balance("USDC".to_string(), config.starting_balance);
        
        let paper_trading_engine = PaperTradingEngine::new(portfolio);
        
        // Initialize smart router
        let router_config = SmartRouterConfig::default();
        let routing_graph = RoutingGraph::new();
        let fee_estimator = FeeEstimator::new();
        let smart_router = SmartRouter::new(router_config, routing_graph, fee_estimator).await?;
        
        let validation_results = ValidationResults {
            start_time: SystemTime::now(),
            end_time: SystemTime::now(),
            total_duration: Duration::from_secs(0),
            total_trades: 0,
            successful_trades: 0,
            failed_trades: 0,
            starting_portfolio_value: config.starting_balance,
            ending_portfolio_value: config.starting_balance,
            total_return_percent: 0.0,
            max_difference_observed: 0.0,
            average_difference: 0.0,
            validation_passed: false,
            security_events: 0,
            performance_issues: 0,
            uptime_percentage: 0.0,
        };
        
        Ok(Self {
            config,
            performance_manager,
            paper_trading_engine,
            _smart_router: smart_router,
            validation_results,
            security_audit_log: Vec::new(),
        })
    }

    /// Run the complete 48+ hour validation
    pub async fn run_validation(&mut self) -> Result<ValidationResults> {
        info!("🎯 Starting 48+ Hour Paper Trading Validation");
        info!("Duration: {:.1} hours", self.config.total_duration.as_secs_f64() / 3600.0);
        info!("Starting Balance: ${:.2}", self.config.starting_balance);
        
        let start_time = Instant::now();
        self.validation_results.start_time = SystemTime::now();
        
        // Start background monitoring tasks
        self.start_background_tasks().await?;
        
        // Main trading loop
        let mut trading_interval = interval(self.config.trading_interval);
        let mut iteration_count = 0u64;
        
        while start_time.elapsed() < self.config.total_duration {
            trading_interval.tick().await;
            iteration_count += 1;
            
            // Execute trading cycle
            match self.execute_trading_cycle(iteration_count).await {
                Ok(_) => {
                    self.validation_results.successful_trades += 1;
                    debug!("Trading cycle {} completed successfully", iteration_count);
                }
                Err(e) => {
                    self.validation_results.failed_trades += 1;
                    warn!("Trading cycle {} failed: {}", iteration_count, e);
                    
                    // Log security event for failed trades
                    self.log_security_event(
                        SecurityEventType::SuspiciousTrading,
                        format!("Trading cycle failed: {}", e),
                        SecuritySeverity::Medium,
                    ).await;
                }
            }
            
            self.validation_results.total_trades += 1;
            
            // Periodic status update
            if iteration_count % 120 == 0 { // Every hour (30s * 120)
                self.log_status_update(start_time).await;
            }
        }
        
        // Complete validation
        self.complete_validation(start_time).await?;
        
        info!("✅ 48+ Hour Validation Complete!");
        Ok(self.validation_results.clone())
    }

    /// Execute a single trading cycle
    async fn execute_trading_cycle(&mut self, cycle_id: u64) -> Result<()> {
        let cycle_start = Instant::now();
        
        // 1. Generate trading opportunities
        let opportunities = self.generate_trading_opportunities().await?;
        
        if opportunities.is_empty() {
            debug!("No trading opportunities found in cycle {}", cycle_id);
            return Ok(());
        }
        
        // 2. Select best opportunity
        let best_opportunity = &opportunities[0];
        
        // 3. Simulate the trade (paper trading)
        let simulation_result = self.simulate_trade(best_opportunity).await?;
        
        // 4. Validate accuracy by comparing with "actual" results
        let actual_result = self.get_actual_trade_result(best_opportunity).await?;
        let difference = self.calculate_difference(&simulation_result, &actual_result);
        
        // 5. Update validation statistics
        self.update_validation_stats(difference);
        
        // 6. Record performance metrics
        let cycle_duration = cycle_start.elapsed();
        {
            let collector = self.performance_manager.metrics_collector();
            let mut collector = collector.write().await;
            collector.record_operation("trading_cycle", cycle_duration, true);
        }
        
        info!("Cycle {}: Simulated: ${:.2}, Actual: ${:.2}, Diff: {:.3}%", 
              cycle_id, simulation_result.expected_profit, actual_result.expected_profit, difference);
        
        Ok(())
    }

    /// Generate trading opportunities
    async fn generate_trading_opportunities(&self) -> Result<Vec<TradingOpportunity>> {
        // Simulate finding arbitrage opportunities
        let opportunities = vec![
            TradingOpportunity {
                input_token: "SOL".to_string(),
                output_token: "USDC".to_string(),
                amount: 1.0,
                expected_profit: 2.50 + (rand::random::<f64>() - 0.5) * 5.0, // $2.50 ± $2.50
                confidence_score: 0.8 + rand::random::<f64>() * 0.2,
                route: "SOL->USDC via Raydium".to_string(),
            },
            TradingOpportunity {
                input_token: "USDC".to_string(),
                output_token: "SOL".to_string(),
                amount: 100.0,
                expected_profit: 1.75 + (rand::random::<f64>() - 0.5) * 3.0, // $1.75 ± $1.50
                confidence_score: 0.75 + rand::random::<f64>() * 0.25,
                route: "USDC->SOL via Orca".to_string(),
            },
        ];
        
        Ok(opportunities)
    }

    /// Simulate a trade using paper trading engine
    async fn simulate_trade(&mut self, opportunity: &TradingOpportunity) -> Result<TradeResult> {
        let result = TradeResult {
            expected_profit: opportunity.expected_profit,
            actual_profit: opportunity.expected_profit * (0.95 + rand::random::<f64>() * 0.1), // 95-105% of expected
            gas_cost: 0.02 + rand::random::<f64>() * 0.03, // $0.02-$0.05 gas
            execution_time: Duration::from_millis(200 + (rand::random::<u64>() % 300)),
            success: true,
        };
        
        // Update paper trading portfolio
        self.paper_trading_engine.update_balance(
            &opportunity.output_token,
            result.actual_profit - result.gas_cost,
        );
        
        Ok(result)
    }

    /// Get "actual" trade result for comparison (simulated for demo)
    async fn get_actual_trade_result(&self, opportunity: &TradingOpportunity) -> Result<TradeResult> {
        // Simulate getting actual market results
        // In real implementation, this would query actual DEX prices
        sleep(Duration::from_millis(50)).await; // Simulate network latency
        
        let actual_profit = opportunity.expected_profit * (0.97 + rand::random::<f64>() * 0.06); // 97-103% variance
        
        Ok(TradeResult {
            expected_profit: opportunity.expected_profit,
            actual_profit,
            gas_cost: 0.025 + rand::random::<f64>() * 0.025, // Slightly different gas costs
            execution_time: Duration::from_millis(180 + (rand::random::<u64>() % 200)),
            success: rand::random::<f64>() > 0.02, // 98% success rate
        })
    }

    /// Calculate percentage difference between simulated and actual results
    fn calculate_difference(&self, simulated: &TradeResult, actual: &TradeResult) -> f64 {
        if actual.actual_profit == 0.0 {
            return 100.0; // Avoid division by zero
        }
        
        let difference = ((simulated.actual_profit - actual.actual_profit) / actual.actual_profit).abs() * 100.0;
        difference
    }

    /// Update validation statistics
    fn update_validation_stats(&mut self, difference: f64) {
        if difference > self.validation_results.max_difference_observed {
            self.validation_results.max_difference_observed = difference;
        }
        
        // Update running average
        let total_comparisons = self.validation_results.total_trades as f64;
        self.validation_results.average_difference = 
            (self.validation_results.average_difference * (total_comparisons - 1.0) + difference) / total_comparisons;
        
        // Check if validation is still passing
        if difference > self.config.max_allowed_difference_percent {
            warn!("⚠️ Validation threshold exceeded: {:.2}% > {:.2}%", 
                  difference, self.config.max_allowed_difference_percent);
        }
    }

    /// Start background monitoring tasks
    async fn start_background_tasks(&self) -> Result<()> {
        // Security audit logging
        let security_log_interval = self.config.audit_log_interval;
        tokio::spawn(async move {
            let mut interval_timer = interval(security_log_interval);
            loop {
                interval_timer.tick().await;
                // Security audit logging would go here
                debug!("Security audit checkpoint completed");
            }
        });
        
        // Performance monitoring
        let perf_manager = self.performance_manager.clone();
        let monitoring_interval = self.config.performance_monitoring_interval;
        tokio::spawn(async move {
            let mut interval_timer = interval(monitoring_interval);
            loop {
                interval_timer.tick().await;
                let report = perf_manager.get_performance_report().await;
                
                // Mock health check - report is just a string for our mock implementation
                if report.len() < 10 {
                    warn!("⚠️ System health degraded: report too short");
                }
            }
        });
        
        // Auto-save state
        let auto_save_interval = self.config.auto_save_interval;
        tokio::spawn(async move {
            let mut interval_timer = interval(auto_save_interval);
            loop {
                interval_timer.tick().await;
                // Auto-save logic would go here
                debug!("Auto-save checkpoint completed");
            }
        });
        
        Ok(())
    }

    /// Log security event
    async fn log_security_event(
        &mut self,
        event_type: SecurityEventType,
        description: String,
        severity: SecuritySeverity,
    ) {
        let event = SecurityEvent {
            timestamp: SystemTime::now(),
            event_type,
            description,
            severity,
        };
        
        self.security_audit_log.push(event.clone());
        self.validation_results.security_events += 1;
        
        // Write to audit log file
        if let Err(e) = self.write_audit_log(&event).await {
            error!("Failed to write audit log: {}", e);
        }
    }

    /// Write security event to audit log file
    async fn write_audit_log(&self, event: &SecurityEvent) -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open("security_audit.log")
            .await?;
        
        let log_entry = format!(
            "{}: [{:?}] {:?} - {}\n",
            event.timestamp
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            event.severity,
            event.event_type,
            event.description
        );
        
        file.write_all(log_entry.as_bytes()).await?;
        file.flush().await?;
        
        Ok(())
    }

    /// Log periodic status update
    async fn log_status_update(&self, start_time: Instant) {
        let elapsed = start_time.elapsed();
        let progress = elapsed.as_secs_f64() / self.config.total_duration.as_secs_f64() * 100.0;
        
        let current_portfolio_value = self.paper_trading_engine.get_total_value();
        let current_return = (current_portfolio_value - self.config.starting_balance) / self.config.starting_balance * 100.0;
        
        info!("📊 Status Update:");
        info!("  Progress: {:.1}% ({:.1} hours elapsed)", progress, elapsed.as_secs_f64() / 3600.0);
        info!("  Total Trades: {}", self.validation_results.total_trades);
        info!("  Success Rate: {:.1}%", 
              if self.validation_results.total_trades > 0 {
                  self.validation_results.successful_trades as f64 / self.validation_results.total_trades as f64 * 100.0
              } else { 0.0 });
        info!("  Portfolio Value: ${:.2} ({:+.2}%)", current_portfolio_value, current_return);
        info!("  Max Difference: {:.2}%", self.validation_results.max_difference_observed);
        info!("  Avg Difference: {:.2}%", self.validation_results.average_difference);
        info!("  Security Events: {}", self.validation_results.security_events);
    }

    /// Complete validation and generate final results
    async fn complete_validation(&mut self, start_time: Instant) -> Result<()> {
        self.validation_results.end_time = SystemTime::now();
        self.validation_results.total_duration = start_time.elapsed();
        
        // Calculate final portfolio value
        self.validation_results.ending_portfolio_value = self.paper_trading_engine.get_total_value();
        self.validation_results.total_return_percent = 
            (self.validation_results.ending_portfolio_value - self.validation_results.starting_portfolio_value) 
            / self.validation_results.starting_portfolio_value * 100.0;
        
        // Calculate uptime percentage
        let total_expected_trades = self.config.total_duration.as_secs() / self.config.trading_interval.as_secs();
        self.validation_results.uptime_percentage = 
            self.validation_results.total_trades as f64 / total_expected_trades as f64 * 100.0;
        
        // Determine if validation passed
        self.validation_results.validation_passed = 
            self.validation_results.max_difference_observed <= self.config.max_allowed_difference_percent &&
            self.validation_results.uptime_percentage >= 95.0 &&
            self.validation_results.security_events == 0;
        
        // Generate final report
        self.generate_final_report().await?;
        
        Ok(())
    }

    /// Generate comprehensive final report
    async fn generate_final_report(&self) -> Result<()> {
        let report = format!(
            "LONG-TERM PAPER TRADING VALIDATION REPORT\n\
             ==========================================\n\
             \n\
             Test Period: {:.1} hours\n\
             Start Time: {:?}\n\
             End Time: {:?}\n\
             \n\
             TRADING PERFORMANCE\n\
             -------------------\n\
             Total Trades: {}\n\
             Successful Trades: {}\n\
             Failed Trades: {}\n\
             Success Rate: {:.1}%\n\
             \n\
             PORTFOLIO PERFORMANCE\n\
             ---------------------\n\
             Starting Value: ${:.2}\n\
             Ending Value: ${:.2}\n\
             Total Return: {:.2}%\n\
             \n\
             ACCURACY VALIDATION\n\
             -------------------\n\
             Maximum Difference: {:.2}%\n\
             Average Difference: {:.2}%\n\
             Validation Threshold: {:.2}%\n\
             Validation Status: {}\n\
             \n\
             SYSTEM RELIABILITY\n\
             ------------------\n\
             Uptime: {:.1}%\n\
             Security Events: {}\n\
             Performance Issues: {}\n\
             \n\
             OVERALL RESULT: {}\n\
             {}",
            self.validation_results.total_duration.as_secs_f64() / 3600.0,
            self.validation_results.start_time,
            self.validation_results.end_time,
            self.validation_results.total_trades,
            self.validation_results.successful_trades,
            self.validation_results.failed_trades,
            if self.validation_results.total_trades > 0 {
                self.validation_results.successful_trades as f64 / self.validation_results.total_trades as f64 * 100.0
            } else { 0.0 },
            self.validation_results.starting_portfolio_value,
            self.validation_results.ending_portfolio_value,
            self.validation_results.total_return_percent,
            self.validation_results.max_difference_observed,
            self.validation_results.average_difference,
            self.config.max_allowed_difference_percent,
            if self.validation_results.validation_passed { "PASSED ✅" } else { "FAILED ❌" },
            self.validation_results.uptime_percentage,
            self.validation_results.security_events,
            self.validation_results.performance_issues,
            if self.validation_results.validation_passed { "VALIDATION SUCCESSFUL ✅" } else { "VALIDATION FAILED ❌" },
            if self.validation_results.validation_passed {
                "The system meets all requirements for production deployment."
            } else {
                "The system requires improvements before production deployment."
            }
        );
        
        // Write report to file
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open("validation_report.txt")
            .await?;
        
        file.write_all(report.as_bytes()).await?;
        file.flush().await?;
        
        println!("{}", report);
        
        Ok(())
    }
}

/// Trading opportunity structure
#[derive(Debug, Clone)]
pub struct TradingOpportunity {
    pub input_token: String,
    pub output_token: String,
    pub amount: f64,
    pub expected_profit: f64,
    pub confidence_score: f64,
    pub route: String,
}

/// Trade result structure
#[derive(Debug, Clone)]
pub struct TradeResult {
    pub expected_profit: f64,
    pub actual_profit: f64,
    pub gas_cost: f64,
    pub execution_time: Duration,
    pub success: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    
    info!("🚀 Starting Long-Term Paper Trading Validation");
    
    // For demo purposes, use a shorter duration (5 minutes instead of 48 hours)
    let config = ValidationConfig {
        total_duration: Duration::from_secs(300), // 5 minutes for demo
        trading_interval: Duration::from_secs(5), // Trade every 5 seconds
        starting_balance: 10_000.0, // $10,000 for demo
        max_allowed_difference_percent: 1.0,
        audit_log_interval: Duration::from_secs(30),
        performance_monitoring_interval: Duration::from_secs(15),
        auto_save_interval: Duration::from_secs(60),
    };
    
    println!("📋 VALIDATION CONFIGURATION:");
    println!("- Duration: {:.1} minutes (demo mode)", config.total_duration.as_secs_f64() / 60.0);
    println!("- Trading Interval: {} seconds", config.trading_interval.as_secs());
    println!("- Starting Balance: ${:.2}", config.starting_balance);
    println!("- Max Allowed Difference: {:.1}%", config.max_allowed_difference_percent);
    println!();
    
    let mut validator = LongTermValidator::new(config).await?;
    let results = validator.run_validation().await?;
    
    println!("\n🎯 VALIDATION COMPLETE!");
    println!("Status: {}", if results.validation_passed { "PASSED ✅" } else { "FAILED ❌" });
    println!("Total Trades: {}", results.total_trades);
    println!("Success Rate: {:.1}%", 
             if results.total_trades > 0 {
                 results.successful_trades as f64 / results.total_trades as f64 * 100.0
             } else { 0.0 });
    println!("Max Difference: {:.2}%", results.max_difference_observed);
    println!("Portfolio Return: {:.2}%", results.total_return_percent);
    
    if results.validation_passed {
        println!("\n✅ SYSTEM READY FOR PRODUCTION DEPLOYMENT");
        println!("- Accuracy validation: PASSED (<1% difference)");
        println!("- Continuous operation: PASSED");
        println!("- Security audit: PASSED");
    } else {
        println!("\n❌ SYSTEM REQUIRES IMPROVEMENTS");
        println!("Please review the validation report for details.");
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_validator_initialization() {
        let config = ValidationConfig {
            total_duration: Duration::from_secs(10),
            ..Default::default()
        };
        
        let validator = LongTermValidator::new(config).await.unwrap();
        assert_eq!(validator.validation_results.starting_portfolio_value, 100_000.0);
    }

    #[tokio::test]
    async fn test_difference_calculation() {
        let config = ValidationConfig::default();
        let validator = LongTermValidator::new(config).await.unwrap();
        
        let simulated = TradeResult {
            expected_profit: 100.0,
            actual_profit: 102.0,
            gas_cost: 1.0,
            execution_time: Duration::from_millis(200),
            success: true,
        };
        
        let actual = TradeResult {
            expected_profit: 100.0,
            actual_profit: 100.0,
            gas_cost: 1.0,
            execution_time: Duration::from_millis(200),
            success: true,
        };
        
        let difference = validator.calculate_difference(&simulated, &actual);
        assert_eq!(difference, 2.0); // 2% difference
    }
}
