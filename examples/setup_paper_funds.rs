// Example: How to set up paper trading with virtual funds

use solana_arb_bot::simulation::{SimulationConfig, SafeVirtualPortfolio};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;

fn setup_paper_trading_with_funds() -> SimulationConfig {
    let mut initial_balances = HashMap::new();

    // Define token mints (these would be real token addresses in production)
    let sol_mint = Pubkey::new_unique(); // Represents SOL
    let usdc_mint = Pubkey::new_unique(); // Represents USDC
    let usdt_mint = Pubkey::new_unique(); // Represents USDT

    // "Fund" your paper trading wallet with virtual tokens
    initial_balances.insert(sol_mint, 100_000_000_000); // 100 SOL (in lamports)
    initial_balances.insert(usdc_mint, 50_000_000_000); // 50,000 USDC (in micro-USDC)
    initial_balances.insert(usdt_mint, 25_000_000_000); // 25,000 USDT (in micro-USDT)

    // Create simulation config with your virtual funds
    SimulationConfig {
        enabled: true,
        initial_balances,
        default_sol_balance: 100_000_000_000, // 100 SOL fallback
        default_usdc_balance: 50_000_000_000, // 50,000 USDC fallback
        simulated_tx_fee: 5_000,              // 0.000005 SOL per transaction
        simulation_slippage_bps: 50,          // 0.5% additional slippage
        simulate_failures: true,
        failure_rate: 0.03, // 3% failure rate for realism
        log_trades: true,
        reports_dir: "./my_paper_trading_logs".to_string(),
        save_analytics: true,
        max_concurrent_trades: 10,
    }
}

// Usage in your main function
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create your virtual portfolio with paper money
    let config = setup_paper_trading_with_funds();
    let portfolio = SafeVirtualPortfolio::new(config.initial_balances.clone());

    // Check your virtual balances
    println!("📊 Your Virtual Portfolio:");
    for (mint, balance) in portfolio.get_balances() {
        println!("  Token {}: {} units", mint, balance);
    }

    // Now you can simulate trades with this virtual money!
    Ok(())
}
