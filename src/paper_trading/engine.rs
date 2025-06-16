// src/paper_trading/engine.rs
//! Simulated execution engine for paper trading

use crate::{
    arbitrage::opportunity::MultiHopArbOpportunity,
    dex::api::DexClient,
    utils::PoolInfo,
};
use super::{config::PaperTradingConfig, portfolio::SafeVirtualPortfolio};
use anyhow::Result;
use log::{debug, error, info, warn};
use rand::Rng;
use solana_sdk::pubkey::Pubkey;
use std::{sync::Arc, time::Instant};
use tokio::time::{sleep, Duration};

/// Result of a simulated trade execution
#[derive(Debug, Clone)]
pub struct SimulatedTradeResult {
    pub success: bool,
    pub input_amount: u64,
    pub output_amount: u64,
    pub expected_output: u64,
    pub slippage_bps: u16,
    pub fee_amount: u64,
    pub execution_time_ms: u64,
    pub error_message: Option<String>,
}

/// Simulated execution engine for paper trading
#[derive(Debug)]
pub struct SimulatedExecutionEngine {
    config: PaperTradingConfig,
    portfolio: Arc<SafeVirtualPortfolio>,
    sol_mint: Pubkey, // Native SOL mint pubkey
}

impl SimulatedExecutionEngine {
    /// Create a new simulated execution engine
    pub fn new(config: PaperTradingConfig, portfolio: Arc<SafeVirtualPortfolio>) -> Self {
        // Use the actual SOL mint from constants
        let sol_mint: Pubkey = "So11111111111111111111111111111111111111112".parse()
            .expect("Invalid SOL mint address");
        
        Self {
            config,
            portfolio,
            sol_mint,
        }
    }
    
    /// Get reference to the virtual portfolio
    pub fn portfolio(&self) -> &SafeVirtualPortfolio {
        &self.portfolio
    }
    
    /// Simulate execution of an arbitrage opportunity
    pub async fn simulate_arbitrage_execution(
        &self,
        opportunity: &MultiHopArbOpportunity,
        _dex_clients: &[Arc<dyn DexClient>],
    ) -> Result<SimulatedTradeResult> {
        let start_time = Instant::now();
        
        info!(
            "Simulating arbitrage execution: {} -> {} (expected profit: {:.2}%)",
            opportunity.input_token, opportunity.output_token, opportunity.profit_pct
        );
        
        // Simulate random execution time (50-200ms)
        let execution_delay = rand::thread_rng().gen_range(50..=200);
        sleep(Duration::from_millis(execution_delay)).await;
        
        // Check if we should simulate a failure
        if self.config.simulate_failures && self.should_simulate_failure() {
            let error_msg = "Simulated transaction failure".to_string();
            warn!("Simulating trade failure: {}", error_msg);
            
            self.portfolio.record_failed_trade();
            
            return Ok(SimulatedTradeResult {
                success: false,
                input_amount: (opportunity.input_amount * 1_000_000.0) as u64, // Convert to lamports
                output_amount: 0,
                expected_output: (opportunity.expected_output * 1_000_000.0) as u64,
                slippage_bps: 0,
                fee_amount: self.config.simulated_tx_fee,
                execution_time_ms: start_time.elapsed().as_millis() as u64,
                error_message: Some(error_msg),
            });
        }
        
        // Calculate simulated output with additional slippage
        let input_amount_lamports = (opportunity.input_amount * 1_000_000.0) as u64;
        let expected_output_lamports = (opportunity.expected_output * 1_000_000.0) as u64;
        let (simulated_output, actual_slippage) = self.calculate_simulated_output(
            expected_output_lamports,
            input_amount_lamports,
        );
        
        // Check if we have sufficient balance for the trade
        if self.portfolio.get_balance(&opportunity.input_token_mint) < input_amount_lamports {
            let error_msg = format!(
                "Insufficient balance for trade: have {}, need {}",
                self.portfolio.get_balance(&opportunity.input_token_mint),
                input_amount_lamports
            );
            
            self.portfolio.record_failed_trade();
            
            return Ok(SimulatedTradeResult {
                success: false,
                input_amount: input_amount_lamports,
                output_amount: 0,
                expected_output: expected_output_lamports,
                slippage_bps: actual_slippage,
                fee_amount: self.config.simulated_tx_fee,
                execution_time_ms: start_time.elapsed().as_millis() as u64,
                error_message: Some(error_msg),
            });
        }
        
        // Execute the simulated trade
        match self.portfolio.execute_trade(
            opportunity.input_token_mint,
            opportunity.output_token_mint,
            input_amount_lamports,
            simulated_output,
            self.config.simulated_tx_fee,
            self.sol_mint,
        ) {
            Ok(()) => {
                info!(
                    "Simulated trade successful: {} {} -> {} {} (slippage: {} bps)",
                    input_amount_lamports,
                    opportunity.input_token_mint,
                    simulated_output,
                    opportunity.output_token_mint,
                    actual_slippage
                );
                
                Ok(SimulatedTradeResult {
                    success: true,
                    input_amount: input_amount_lamports,
                    output_amount: simulated_output,
                    expected_output: expected_output_lamports,
                    slippage_bps: actual_slippage,
                    fee_amount: self.config.simulated_tx_fee,
                    execution_time_ms: start_time.elapsed().as_millis() as u64,
                    error_message: None,
                })
            }
            Err(e) => {
                error!("Simulated trade failed: {}", e);
                self.portfolio.record_failed_trade();
                
                Ok(SimulatedTradeResult {
                    success: false,
                    input_amount: input_amount_lamports,
                    output_amount: 0,
                    expected_output: expected_output_lamports,
                    slippage_bps: actual_slippage,
                    fee_amount: self.config.simulated_tx_fee,
                    execution_time_ms: start_time.elapsed().as_millis() as u64,
                    error_message: Some(e.to_string()),
                })
            }
        }
    }
    
    /// Simulate execution of a single swap
    pub async fn simulate_swap_execution(
        &self,
        pool: &PoolInfo,
        input_mint: Pubkey,
        output_mint: Pubkey,
        input_amount: u64,
        expected_output: u64,
    ) -> Result<SimulatedTradeResult> {
        let start_time = Instant::now();
        
        debug!(
            "Simulating swap: {} {} -> {} {} on {:?}",
            input_amount, input_mint, expected_output, output_mint, pool.dex_type
        );
        
        // Simulate execution delay
        let execution_delay = rand::thread_rng().gen_range(30..=100);
        sleep(Duration::from_millis(execution_delay)).await;
        
        // Check for simulated failure
        if self.config.simulate_failures && self.should_simulate_failure() {
            self.portfolio.record_failed_trade();
            return Ok(SimulatedTradeResult {
                success: false,
                input_amount,
                output_amount: 0,
                expected_output,
                slippage_bps: 0,
                fee_amount: self.config.simulated_tx_fee,
                execution_time_ms: start_time.elapsed().as_millis() as u64,
                error_message: Some("Simulated swap failure".to_string()),
            });
        }
        
        // Calculate simulated output
        let (simulated_output, actual_slippage) = self.calculate_simulated_output(
            expected_output,
            input_amount,
        );
        
        // Execute the trade
        match self.portfolio.execute_trade(
            input_mint,
            output_mint,
            input_amount,
            simulated_output,
            self.config.simulated_tx_fee,
            self.sol_mint,
        ) {
            Ok(()) => Ok(SimulatedTradeResult {
                success: true,
                input_amount,
                output_amount: simulated_output,
                expected_output,
                slippage_bps: actual_slippage,
                fee_amount: self.config.simulated_tx_fee,
                execution_time_ms: start_time.elapsed().as_millis() as u64,
                error_message: None,
            }),
            Err(e) => {
                self.portfolio.record_failed_trade();
                Ok(SimulatedTradeResult {
                    success: false,
                    input_amount,
                    output_amount: 0,
                    expected_output,
                    slippage_bps: actual_slippage,
                    fee_amount: self.config.simulated_tx_fee,
                    execution_time_ms: start_time.elapsed().as_millis() as u64,
                    error_message: Some(e.to_string()),
                })
            }
        }
    }
    
    /// Calculate simulated output with additional slippage
    fn calculate_simulated_output(&self, expected_output: u64, _input_amount: u64) -> (u64, u16) {
        // Add some random slippage on top of the configured base slippage
        let mut rng = rand::thread_rng();
        let random_slippage_bps = rng.gen_range(0..=self.config.simulation_slippage_bps);
        let total_slippage_bps = self.config.simulation_slippage_bps + random_slippage_bps;
        
        // Calculate output after slippage
        let slippage_factor = 1.0 - (total_slippage_bps as f64 / 10_000.0);
        let simulated_output = (expected_output as f64 * slippage_factor) as u64;
        
        (simulated_output, total_slippage_bps)
    }
    
    /// Determine if we should simulate a failure based on configured failure rate
    fn should_simulate_failure(&self) -> bool {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let random_value = rng.gen_range(0.0..1.0);
        random_value < self.config.failure_rate
    }
    
    /// Get current portfolio summary
    pub fn get_portfolio_summary(&self) -> super::portfolio::PortfolioSummary {
        self.portfolio.get_summary()
    }
    
    /// Check if the engine can execute a trade with given parameters
    pub fn can_execute_trade(&self, input_mint: &Pubkey, input_amount: u64) -> bool {
        let current_balance = self.portfolio.get_balance(input_mint);
        let sol_balance = self.portfolio.get_balance(&self.sol_mint);
        
        current_balance >= input_amount && sol_balance >= self.config.simulated_tx_fee
    }
    
    /// Get the configuration
    pub fn config(&self) -> &PaperTradingConfig {
        &self.config
    }
}
