// examples/paper_trading_demo.rs
//! Simple demonstration of the paper trading functionality

use solana_arb_bot::{
    paper_trading::{
        PaperTradingConfig, SafeVirtualPortfolio, PaperTradingAnalytics, 
        SimulatedExecutionEngine, PaperTradingReporter
    },
    arbitrage::opportunity::MultiHopArbOpportunity,
    arbitrage::opportunity::ArbHop,
    utils::DexType,
};
use solana_sdk::pubkey::Pubkey;
use std::{sync::Arc, collections::HashMap};
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Paper Trading Demo");
    println!("====================");
    
    // Create initial virtual balances
    let mut initial_balances = HashMap::new();
    let sol_mint = Pubkey::new_unique();
    let usdc_mint = Pubkey::new_unique();
    initial_balances.insert(sol_mint, 5_000_000_000); // 5 SOL
    initial_balances.insert(usdc_mint, 10_000_000_000); // 10,000 USDC
    
    // Configure paper trading
    let config = PaperTradingConfig {
        enabled: true,
        initial_balances: initial_balances.clone(),
        default_sol_balance: 5_000_000_000, // 5 SOL
        default_usdc_balance: 10_000_000_000, // 10,000 USDC
        simulated_tx_fee: 5000,
        simulation_slippage_bps: 100, // 1%
        simulate_failures: true,
        failure_rate: 0.05, // 5%
        log_trades: true,
        reports_dir: "./paper_trading_reports".to_string(),
        save_analytics: true,
        max_concurrent_trades: 5,
    };
    
    // Initialize components
    let portfolio = Arc::new(SafeVirtualPortfolio::new(initial_balances));
    let analytics = Arc::new(Mutex::new(PaperTradingAnalytics::new()));
    let engine = SimulatedExecutionEngine::new(config);
    let reporter = PaperTradingReporter::new("./demo_paper_logs")?;
    
    println!("💰 Initial Portfolio:");
    println!("  SOL: {} lamports", portfolio.get_balance(&sol_mint));
    println!("  USDC: {} micro-units", portfolio.get_balance(&usdc_mint));
    
    // Create a sample arbitrage opportunity
    let opportunity = MultiHopArbOpportunity {
        id: "demo_arb_001".to_string(),
        hops: vec![
            ArbHop {
                dex: DexType::Orca,
                pool: Pubkey::new_unique(),
                input_token: "SOL".to_string(),
                output_token: "USDC".to_string(),
                input_amount: 0.1, // 0.1 SOL
                expected_output: 15.0, // ~15 USDC
            }
        ],
        total_profit: 0.5, // 0.5 USDC expected profit
        profit_pct: 3.33, // 3.33% profit
        input_token: "SOL".to_string(),
        output_token: "USDC".to_string(),
        input_amount: 0.1,
        expected_output: 15.0,
        dex_path: vec![DexType::Orca],
        pool_path: vec![Pubkey::new_unique()],
        risk_score: Some(0.2),
        notes: Some("Demo arbitrage opportunity".to_string()),
        estimated_profit_usd: Some(0.5),
        input_amount_usd: Some(15.0),
        output_amount_usd: Some(15.5),
        intermediate_tokens: vec![],
        source_pool: Arc::new(create_sample_pool_info(sol_mint, usdc_mint)),
        target_pool: Arc::new(create_sample_pool_info(usdc_mint, sol_mint)),
        input_token_mint: sol_mint,
        output_token_mint: usdc_mint,
        intermediate_token_mint: None,
        estimated_gas_cost: Some(5000),
        detected_at: Some(std::time::Instant::now()),
    };
    
    println!("\n📊 Simulating arbitrage opportunity:");
    println!("  Route: {} -> {}", opportunity.input_token, opportunity.output_token);
    println!("  Input: {} {}", opportunity.input_amount, opportunity.input_token);
    println!("  Expected Output: {} {}", opportunity.expected_output, opportunity.output_token);
    println!("  Expected Profit: {:.2}%", opportunity.profit_pct);
    
    // Simulate multiple trades
    for i in 1..=5 {
        println!("\n🔄 Executing trade {} of 5...", i);
        
        match engine.simulate_arbitrage_execution(&opportunity, &[]).await {
            Ok(result) => {
                println!("  ✅ Trade {}: {}", i, if result.success { "SUCCESS" } else { "FAILED" });
                
                // Clone error message early to avoid moved value issues
                let error_message_for_display = result.error_message.clone();
                let error_message_for_analytics = result.error_message.clone();
                let error_message_for_log = result.error_message.clone();
                
                if result.success {
                    println!("    Input: {} lamports", result.input_amount);
                    println!("    Output: {} lamports", result.output_amount);
                    println!("    Profit: {} lamports", result.output_amount as i64 - result.input_amount as i64);
                    println!("    Slippage: {:.2}%", result.slippage_bps as f64 / 100.0);
                    println!("    Fee: {} lamports", result.fee_amount);
                } else {
                    println!("    Error: {}", error_message_for_display.unwrap_or_default());
                }
                
                // Record analytics
                let mut analytics_guard = analytics.lock().await;
                
                if result.success {
                    analytics_guard.record_successful_execution(
                        result.input_amount,
                        result.output_amount,
                        result.output_amount as i64 - result.input_amount as i64,
                        result.fee_amount,
                    );
                } else {
                    analytics_guard.record_failed_execution(
                        result.input_amount,
                        result.fee_amount,
                        error_message_for_analytics.unwrap_or_default(),
                    );
                }
                
                // Log trade
                let trade_entry = PaperTradingReporter::create_trade_log_entry(
                    &opportunity,
                    result.input_amount,
                    result.output_amount,
                    result.output_amount as i64 - result.input_amount as i64,
                    result.slippage_bps as f64 / 10000.0,
                    result.fee_amount,
                    result.success,
                    error_message_for_log,
                    0,
                );
                
                if let Err(e) = reporter.log_trade(trade_entry) {
                    eprintln!("Failed to log trade: {}", e);
                }
            }
            Err(e) => {
                println!("  ❌ Trade {} failed: {}", i, e);
                
                let mut analytics_guard = analytics.lock().await;
                analytics_guard.record_failed_execution(
                    (opportunity.input_amount * 1_000_000.0) as u64,
                    0,
                    format!("Simulation error: {}", e),
                );
            }
        }
        
        // Small delay between trades
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
    
    // Generate final report
    println!("\n📊 Final Performance Report:");
    let analytics_guard = analytics.lock().await;
    let portfolio_snapshot = portfolio.snapshot();
    
    println!("  Total Trades: {}", analytics_guard.opportunities_executed);
    println!("  Successful: {}", analytics_guard.successful_executions);
    println!("  Failed: {}", analytics_guard.failed_executions);
    println!("  Success Rate: {:.1}%", analytics_guard.get_success_rate());
    println!("  Total P&L: {} lamports", analytics_guard.total_pnl);
    println!("  Average Profit/Trade: {:.0} lamports", analytics_guard.get_average_profit_per_trade());
    println!("  Total Fees: {} lamports", analytics_guard.total_fees_paid);
    
    // Export analytics
    if let Err(e) = reporter.export_analytics(&analytics_guard, &portfolio_snapshot) {
        eprintln!("Failed to export analytics: {}", e);
    } else {
        println!("\n📁 Reports saved to: ./demo_paper_logs/");
        let (trade_path, analytics_path) = reporter.get_log_paths();
        println!("  Trade logs: {}", trade_path);
        println!("  Analytics: {}", analytics_path);
    }
    
    // Print performance summary
    reporter.print_performance_summary(&analytics_guard, &portfolio_snapshot);
    
    println!("\n🎉 Paper trading demo completed!");
    
    Ok(())
}

fn create_sample_pool_info(token_a: Pubkey, token_b: Pubkey) -> solana_arb_bot::utils::PoolInfo {
    use solana_arb_bot::utils::{PoolToken, DexType};
    
    solana_arb_bot::utils::PoolInfo {
        address: Pubkey::new_unique(),
        name: "Sample Pool".to_string(),
        token_a: PoolToken {
            mint: token_a,
            symbol: "TokenA".to_string(),
            decimals: 9,
            reserve: 1_000_000_000,
        },
        token_b: PoolToken {
            mint: token_b,
            symbol: "TokenB".to_string(),
            decimals: 6,
            reserve: 150_000_000_000,
        },
        token_a_vault: Pubkey::new_unique(),
        token_b_vault: Pubkey::new_unique(),
        fee_numerator: Some(3),
        fee_denominator: Some(1000),
        fee_rate_bips: Some(30),
        last_update_timestamp: 0,
        dex_type: DexType::Orca,
        liquidity: Some(1_000_000_000_000),
        sqrt_price: None,
        tick_current_index: None,
        tick_spacing: None,
        tick_array_0: None,
        tick_array_1: None,
        tick_array_2: None,
        oracle: None,
    }
}
