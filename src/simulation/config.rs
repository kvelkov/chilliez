//! Configuration for simulation mode

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;

/// Configuration for simulation mode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationConfig {
    pub enabled: bool,
    pub initial_balances: HashMap<Pubkey, u64>,
    pub default_sol_balance: u64,
    pub default_usdc_balance: u64,
    pub simulated_tx_fee: u64,
    pub simulation_slippage_bps: u16,
    pub simulate_failures: bool,
    pub failure_rate: f64,
    pub log_trades: bool,
    pub reports_dir: String,
    pub save_analytics: bool,
    pub max_concurrent_trades: usize,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        let mut initial_balances = HashMap::new();
        let sol_mint = Pubkey::default();
        initial_balances.insert(sol_mint, 10_000_000_000);
        Self {
            enabled: false,
            initial_balances,
            default_sol_balance: 10_000_000_000,
            default_usdc_balance: 100_000_000_000,
            simulated_tx_fee: 5000,
            simulation_slippage_bps: 10,
            simulate_failures: false,
            failure_rate: 0.0,
            log_trades: true,
            reports_dir: "./simulation_reports".to_string(),
            save_analytics: true,
            max_concurrent_trades: 8,
        }
    }
}

impl SimulationConfig {
    /// TODO: Replace stub with real configuration initialization logic
    /// Create a new `SimulationConfig` with default settings
    pub fn new() -> Self {
        Self::default()
    }
}
