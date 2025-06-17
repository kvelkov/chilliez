// src/paper_trading/config.rs
//! Configuration for paper trading mode

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;

/// Configuration for paper trading mode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperTradingConfig {
    /// Whether paper trading mode is enabled
    pub enabled: bool,

    /// Initial virtual balances for different tokens (in base units)
    pub initial_balances: HashMap<Pubkey, u64>,

    /// Default SOL balance if not specified (in lamports)
    pub default_sol_balance: u64,

    /// Default USDC balance if not specified (in micro-USDC)
    pub default_usdc_balance: u64,

    /// Simulated transaction fees (in lamports)
    pub simulated_tx_fee: u64,

    /// Additional slippage factor for simulation (basis points)
    pub simulation_slippage_bps: u16,

    /// Whether to simulate failed transactions
    pub simulate_failures: bool,

    /// Failure rate for simulated transactions (0.0 - 1.0)
    pub failure_rate: f64,

    /// Whether to log all simulated trades
    pub log_trades: bool,

    /// Output directory for paper trading reports
    pub reports_dir: String,

    /// Whether to save detailed analytics
    pub save_analytics: bool,

    /// Maximum number of concurrent simulated trades
    pub max_concurrent_trades: usize,
}

impl Default for PaperTradingConfig {
    fn default() -> Self {
        let mut initial_balances = HashMap::new();

        // SOL mint (native SOL)
        let sol_mint = Pubkey::default(); // Placeholder - should be actual SOL mint
        initial_balances.insert(sol_mint, 10_000_000_000); // 10 SOL in lamports

        Self {
            enabled: false,
            initial_balances,
            default_sol_balance: 10_000_000_000,   // 10 SOL
            default_usdc_balance: 100_000_000_000, // 100,000 USDC
            simulated_tx_fee: 5_000,               // 0.000005 SOL
            simulation_slippage_bps: 10,           // 0.1% additional slippage
            simulate_failures: true,
            failure_rate: 0.02, // 2% failure rate
            log_trades: true,
            reports_dir: "./paper_trading_reports".to_string(),
            save_analytics: true,
            max_concurrent_trades: 10,
        }
    }
}

impl PaperTradingConfig {
    /// Create a new paper trading config with reasonable defaults
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable paper trading mode with custom initial balances
    pub fn with_balances(mut self, balances: HashMap<Pubkey, u64>) -> Self {
        self.enabled = true;
        self.initial_balances = balances;
        self
    }

    /// Set the failure simulation parameters
    pub fn with_failure_simulation(mut self, enabled: bool, rate: f64) -> Self {
        self.simulate_failures = enabled;
        self.failure_rate = rate.clamp(0.0, 1.0);
        self
    }

    /// Enable paper trading with default settings
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }

    /// Get initial balance for a specific token mint
    pub fn get_initial_balance(&self, mint: &Pubkey) -> Option<u64> {
        self.initial_balances.get(mint).copied()
    }
}
