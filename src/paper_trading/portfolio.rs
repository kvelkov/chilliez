// src/paper_trading/portfolio.rs
//! Virtual portfolio management for paper trading

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use log::info;

/// Virtual portfolio for tracking simulated balances and trades
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualPortfolio {
    /// Current token balances (mint -> balance in base units)
    pub balances: HashMap<Pubkey, u64>,
    
    /// Initial balances for calculating P&L
    pub initial_balances: HashMap<Pubkey, u64>,
    
    /// Total number of trades executed
    pub total_trades: u64,
    
    /// Total number of successful trades
    pub successful_trades: u64,
    
    /// Total fees paid (in SOL lamports)
    pub total_fees_paid: u64,
    
    /// Portfolio creation timestamp
    pub created_at: u64,
    
    /// Last update timestamp
    pub updated_at: u64,
}

impl VirtualPortfolio {
    /// Create a new virtual portfolio with initial balances
    pub fn new(initial_balances: HashMap<Pubkey, u64>) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
            
        Self {
            balances: initial_balances.clone(),
            initial_balances,
            total_trades: 0,
            successful_trades: 0,
            total_fees_paid: 0,
            created_at: now,
            updated_at: now,
        }
    }
    
    /// Get current balance for a specific token
    pub fn get_balance(&self, mint: &Pubkey) -> u64 {
        self.balances.get(mint).copied().unwrap_or(0)
    }
    
    /// Get all current balances
    pub fn get_balances(&self) -> &HashMap<Pubkey, u64> {
        &self.balances
    }
    
    /// Update balance for a specific token
    pub fn set_balance(&mut self, mint: Pubkey, balance: u64) {
        self.balances.insert(mint, balance);
        self.updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
    }
    
    /// Add to balance for a specific token
    pub fn add_balance(&mut self, mint: Pubkey, amount: u64) {
        let current = self.get_balance(&mint);
        self.set_balance(mint, current.saturating_add(amount));
    }
    
    /// Subtract from balance for a specific token
    pub fn subtract_balance(&mut self, mint: Pubkey, amount: u64) -> Result<()> {
        let current = self.get_balance(&mint);
        if current < amount {
            return Err(anyhow!(
                "Insufficient balance for {}: have {}, need {}", 
                mint, current, amount
            ));
        }
        self.set_balance(mint, current - amount);
        Ok(())
    }
    
    /// Execute a simulated trade
    pub fn execute_trade(
        &mut self,
        input_mint: Pubkey,
        output_mint: Pubkey,
        input_amount: u64,
        output_amount: u64,
        fee_amount: u64,
        sol_mint: Pubkey,
    ) -> Result<()> {
        // Check if we have sufficient input balance
        if self.get_balance(&input_mint) < input_amount {
            return Err(anyhow!(
                "Insufficient {} balance for trade: have {}, need {}",
                input_mint,
                self.get_balance(&input_mint),
                input_amount
            ));
        }
        
        // Check if we have sufficient SOL for fees
        if sol_mint != input_mint && self.get_balance(&sol_mint) < fee_amount {
            return Err(anyhow!(
                "Insufficient SOL balance for fees: have {}, need {}",
                self.get_balance(&sol_mint),
                fee_amount
            ));
        }
        
        // Execute the trade
        self.subtract_balance(input_mint, input_amount)?;
        self.add_balance(output_mint, output_amount);
        
        // Pay fees
        if sol_mint != input_mint {
            self.subtract_balance(sol_mint, fee_amount)?;
        }
        
        // Update statistics
        self.total_trades += 1;
        self.successful_trades += 1;
        self.total_fees_paid = self.total_fees_paid.saturating_add(fee_amount);
        
        info!(
            "Paper trade executed: {} {} -> {} {} (fee: {} SOL)",
            input_amount, input_mint, output_amount, output_mint, fee_amount
        );
        
        Ok(())
    }
    
    /// Record a failed trade attempt
    pub fn record_failed_trade(&mut self) {
        self.total_trades += 1;
        self.updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
    }
    
    /// Calculate profit/loss for a specific token compared to initial balance
    pub fn calculate_pnl(&self, mint: &Pubkey) -> i64 {
        let current = self.get_balance(mint) as i64;
        let initial = self.initial_balances.get(mint).copied().unwrap_or(0) as i64;
        current - initial
    }
    
    /// Get success rate as a percentage
    pub fn success_rate(&self) -> f64 {
        if self.total_trades == 0 {
            return 0.0;
        }
        (self.successful_trades as f64 / self.total_trades as f64) * 100.0
    }
    
    /// Get portfolio summary
    pub fn get_summary(&self) -> PortfolioSummary {
        let mut pnl_by_token = HashMap::new();
        for (mint, &initial) in &self.initial_balances {
            let current = self.get_balance(mint);
            let pnl = current as i64 - initial as i64;
            pnl_by_token.insert(*mint, pnl);
        }
        
        PortfolioSummary {
            total_trades: self.total_trades,
            successful_trades: self.successful_trades,
            success_rate: self.success_rate(),
            total_fees_paid: self.total_fees_paid,
            pnl_by_token,
            uptime_seconds: self.updated_at.saturating_sub(self.created_at),
        }
    }
}

/// Summary of portfolio performance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioSummary {
    pub total_trades: u64,
    pub successful_trades: u64,
    pub success_rate: f64,
    pub total_fees_paid: u64,
    pub pnl_by_token: HashMap<Pubkey, i64>,
    pub uptime_seconds: u64,
}

/// Thread-safe wrapper for virtual portfolio
#[derive(Debug, Clone)]
pub struct SafeVirtualPortfolio {
    inner: Arc<RwLock<VirtualPortfolio>>,
}

impl SafeVirtualPortfolio {
    /// Create a new thread-safe virtual portfolio
    pub fn new(initial_balances: HashMap<Pubkey, u64>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(VirtualPortfolio::new(initial_balances))),
        }
    }

    /// Create a new thread-safe virtual portfolio from config
    pub async fn from_config(config: &crate::paper_trading::config::PaperTradingConfig) -> Result<Arc<Self>> {
        let mut initial_balances = config.initial_balances.clone();
        
        // If no initial balances specified, use defaults
        if initial_balances.is_empty() {
            let sol_mint: Pubkey = "So11111111111111111111111111111111111111112".parse()
                .expect("Invalid SOL mint");
            initial_balances.insert(sol_mint, config.default_sol_balance);
        }
        
        Ok(Arc::new(Self {
            inner: Arc::new(RwLock::new(VirtualPortfolio::new(initial_balances))),
        }))
    }
    
    /// Get current balance for a token
    pub fn get_balance(&self, mint: &Pubkey) -> u64 {
        self.inner.read().unwrap().get_balance(mint)
    }
    
    /// Execute a simulated trade
    pub fn execute_trade(
        &self,
        input_mint: Pubkey,
        output_mint: Pubkey,
        input_amount: u64,
        output_amount: u64,
        fee_amount: u64,
        sol_mint: Pubkey,
    ) -> Result<()> {
        self.inner.write().unwrap().execute_trade(
            input_mint, output_mint, input_amount, 
            output_amount, fee_amount, sol_mint
        )
    }
    
    /// Record a failed trade
    pub fn record_failed_trade(&self) {
        self.inner.write().unwrap().record_failed_trade();
    }
    
    /// Get portfolio summary
    pub fn get_summary(&self) -> PortfolioSummary {
        self.inner.read().unwrap().get_summary()
    }
    
    /// Get a snapshot of the current portfolio
    pub fn snapshot(&self) -> VirtualPortfolio {
        self.inner.read().unwrap().clone()
    }
    
    /// Get total portfolio value in lamports (simplified calculation)
    pub fn get_total_value(&self) -> u64 {
        self.inner.read().unwrap().balances.values().sum()
    }
    
    /// Get all current balances
    pub fn get_balances(&self) -> HashMap<Pubkey, u64> {
        self.inner.read().unwrap().balances.clone()
    }
}
