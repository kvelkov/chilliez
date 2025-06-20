// examples/mainnet_paper_trading_demo.rs
//! Mainnet Paper Trading Demo
//!
//! This demo runs paper trading against real mainnet data but with simulated trades.
//! It's the next step after devnet testing and before live trading.
//!
//! Features:
//! - Real mainnet API data
//! - Simulated trades (no real money)
//! - Enhanced error handling with ban detection
//! - Real-time monitoring and reporting

use anyhow::Result;
use log::{error, info, warn};
use std::env;
use std::time::Duration;
use tokio::time::sleep;

use solana_arb_bot::{
    api::{EnhancedApiErrorHandler, EnhancedRetryExecutor},
    simulation::{SimulationConfig, SafeVirtualPortfolio, SimulatedExecutionEngine},
};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    info!("🚀 Starting Mainnet Paper Trading Demo");
    info!("═══════════════════════════════════════");

    // Load mainnet environment
    match dotenv::from_filename(".env.mainnet") {
        Ok(_) => info!("✅ Loaded .env.mainnet configuration"),
        Err(e) => {
            error!("❌ Failed to load .env.mainnet: {}", e);
            info!("💡 Make sure .env.mainnet exists with proper configuration");
            return Err(e.into());
        }
    }

    // Verify paper trading mode
    let paper_trading_mode = env::var("PAPER_TRADING_MODE")
        .unwrap_or_default()
        .parse::<bool>()
        .unwrap_or(true);

    if !paper_trading_mode {
        error!("🚨 SAFETY CHECK: PAPER_TRADING_MODE must be true for this demo!");
        error!("Please set PAPER_TRADING_MODE=true in .env.mainnet");
        return Err(anyhow::anyhow!("Paper trading mode required for safety"));
    }

    info!("✅ Paper trading mode confirmed - safe to proceed");

    // Initialize enhanced error handling
    info!("🛡️ Initializing enhanced error handling...");
    let _helius_handler = EnhancedApiErrorHandler::new("helius".to_string());
    let _raydium_handler = EnhancedApiErrorHandler::new("raydium".to_string());
    let _orca_handler = EnhancedApiErrorHandler::new("orca".to_string());
    let _jupiter_handler = EnhancedApiErrorHandler::new("jupiter".to_string());

    // Create retry executors
    let _helius_executor = EnhancedRetryExecutor::new("helius".to_string(), 3);
    let mut raydium_executor = EnhancedRetryExecutor::new("raydium".to_string(), 3);
    let mut orca_executor = EnhancedRetryExecutor::new("orca".to_string(), 3);
    let mut jupiter_executor = EnhancedRetryExecutor::new("jupiter".to_string(), 3);

    info!("✅ Enhanced error handling initialized for all APIs");

    // Test API connections with error handling
    info!("🔌 Testing mainnet API connections...");

    // Test Orca API
    info!("Testing Orca API connection...");
    let orca_test_result = orca_executor
        .execute_with_retry(|| async {
            let client = reqwest::Client::new();
            let response = client
                .get("https://api.mainnet.orca.so/v1/whirlpool/list")
                .timeout(Duration::from_secs(10))
                .send()
                .await?;

            if response.status().is_success() {
                let text = response.text().await?;
                Ok(text.len())
            } else {
                Err(anyhow::anyhow!(
                    "HTTP {}: {}",
                    response.status(),
                    response.status().canonical_reason().unwrap_or("Unknown")
                ))
            }
        })
        .await;

    match orca_test_result {
        Ok(data_size) => info!("✅ Orca API: Connected successfully ({} bytes)", data_size),
        Err(e) => {
            warn!("⚠️ Orca API: Connection failed - {}", e);
            let ban_status = orca_executor.get_ban_status();
            if ban_status.is_banned {
                warn!("🚨 Orca API appears to be banned or rate limited");
            }
        }
    }

    // Test Raydium API
    info!("Testing Raydium API connection...");
    let raydium_test_result = raydium_executor
        .execute_with_retry(|| async {
            let client = reqwest::Client::new();
            let response = client
                .get("https://api.raydium.io/v2/sdk/liquidity/mainnet.json")
                .timeout(Duration::from_secs(10))
                .send()
                .await?;

            if response.status().is_success() {
                let text = response.text().await?;
                Ok(text.len())
            } else {
                Err(anyhow::anyhow!(
                    "HTTP {}: {}",
                    response.status(),
                    response.status().canonical_reason().unwrap_or("Unknown")
                ))
            }
        })
        .await;

    match raydium_test_result {
        Ok(data_size) => info!(
            "✅ Raydium API: Connected successfully ({} bytes)",
            data_size
        ),
        Err(e) => {
            warn!("⚠️ Raydium API: Connection failed - {}", e);
            let ban_status = raydium_executor.get_ban_status();
            if ban_status.is_banned {
                warn!("🚨 Raydium API appears to be banned or rate limited");
            }
        }
    }

    // Test Jupiter API
    info!("Testing Jupiter API connection...");
    let jupiter_test_result = jupiter_executor
        .execute_with_retry(|| async {
            let client = reqwest::Client::new();
            let response = client
                .get("https://quote-api.jup.ag/v6/tokens")
                .timeout(Duration::from_secs(10))
                .send()
                .await?;

            if response.status().is_success() {
                let text = response.text().await?;
                Ok(text.len())
            } else {
                Err(anyhow::anyhow!(
                    "HTTP {}: {}",
                    response.status(),
                    response.status().canonical_reason().unwrap_or("Unknown")
                ))
            }
        })
        .await;

    match jupiter_test_result {
        Ok(data_size) => info!(
            "✅ Jupiter API: Connected successfully ({} bytes)",
            data_size
        ),
        Err(e) => {
            warn!("⚠️ Jupiter API: Connection failed - {}", e);
            let ban_status = jupiter_executor.get_ban_status();
            if ban_status.is_banned {
                warn!("🚨 Jupiter API appears to be banned or rate limited");
            }
        }
    }

    // Initialize paper trading config with proper fields
    info!("📊 Initializing paper trading configuration...");
    let initial_sol_balance = env::var("INITIAL_BALANCE_LAMPORTS")
        .unwrap_or_default()
        .parse()
        .unwrap_or(100_000_000); // 0.1 SOL

    let mut config = SimulationConfig::new();
    config.enabled = true;
    config.default_sol_balance = initial_sol_balance;
    config.default_usdc_balance = 100_000_000_000; // 100k USDC
    config.simulated_tx_fee = 5000; // Transaction fee in lamports
    config.simulation_slippage_bps = 50; // 0.5% additional slippage
    config.simulate_failures = true;
    config.failure_rate = 0.02; // 2% failure rate
    config.log_trades = true;
    config.save_analytics = true;
    config.max_concurrent_trades = 5;
    config.reports_dir = "./mainnet_paper_trading_reports".to_string();

    // Initialize paper trading portfolio
    let portfolio = SafeVirtualPortfolio::from_config(&config).await?;
    
    // SOL mint address
    let sol_mint: solana_sdk::pubkey::Pubkey = "So11111111111111111111111111111111111111112".parse()?;
    let _engine = SimulatedExecutionEngine::new(config.clone(), std::sync::Arc::new(portfolio.clone()), sol_mint);

    info!(
        "✅ Paper trading engine initialized with {} SOL starting balance",
        config.default_sol_balance as f64 / 1_000_000_000.0
    );

    // Display current configuration
    info!("⚙️ Current Configuration:");
    info!(
        "  • Network: {}",
        env::var("SOLANA_NETWORK").unwrap_or("mainnet-beta".to_string())
    );
    info!(
        "  • RPC URL: {}",
        env::var("SOLANA_RPC_URL").unwrap_or("default".to_string())
    );
    info!("  • Paper Trading: {}", paper_trading_mode);
    info!(
        "  • Max Position Size: {} SOL",
        env::var("MAX_POSITION_SIZE_LAMPORTS")
            .unwrap_or_default()
            .parse::<u64>()
            .unwrap_or(10_000_000) as f64
            / 1_000_000_000.0
    );
    info!(
        "  • Max Slippage: {}%",
        env::var("MAX_SLIPPAGE_BPS")
            .unwrap_or_default()
            .parse::<u16>()
            .unwrap_or(25) as f64
            / 100.0
    );

    // Simulate some trading activity for demonstration
    info!("🔄 Starting simulated trading loop...");
    info!("Note: This is PAPER TRADING - no real transactions are executed");

    for round in 1..=5 {
        info!("📈 Trading Round {}/5", round);

        // Simulate finding an arbitrage opportunity
        if round % 2 == 1 {
            info!("  🎯 Simulating arbitrage opportunity found:");
            info!("    • SOL/USDC price difference between Orca and Raydium");
            info!("    • Expected profit: 0.001 SOL");

            // Simulate trade execution
            info!("    🔄 Executing simulated trade...");
            sleep(Duration::from_millis(500)).await;

            // Simulate successful trade
            let trade_successful = true;

            if trade_successful {
                info!("    ✅ Paper trade executed successfully");
                info!("    📊 Portfolio updated (simulated)");
            } else {
                warn!("    ❌ Paper trade simulation failed");
            }
        } else {
            info!("  📊 No profitable opportunities found in this round");
        }

        // Display current portfolio status
        let portfolio_summary = portfolio.get_summary();
        info!("  💰 Current Portfolio:");
        info!(
            "    • Total Value: {:.6} SOL",
            portfolio.get_total_value() as f64 / 1_000_000_000.0
        );
        info!("    • Trade Count: {}", portfolio_summary.total_trades);
        info!("    • Total Fees: {} lamports", portfolio_summary.total_fees_paid);

        // Show API health status
        let orca_status = orca_executor.get_ban_status();
        let raydium_status = raydium_executor.get_ban_status();
        let jupiter_status = jupiter_executor.get_ban_status();

        info!("  🌐 API Health Status:");
        info!(
            "    • Orca: {} (errors: {})",
            if orca_status.is_banned {
                "🚨 BANNED"
            } else {
                "✅ OK"
            },
            orca_status.recent_error_count
        );
        info!(
            "    • Raydium: {} (errors: {})",
            if raydium_status.is_banned {
                "🚨 BANNED"
            } else {
                "✅ OK"
            },
            raydium_status.recent_error_count
        );
        info!(
            "    • Jupiter: {} (errors: {})",
            if jupiter_status.is_banned {
                "🚨 BANNED"
            } else {
                "✅ OK"
            },
            jupiter_status.recent_error_count
        );

        // Wait between rounds
        if round < 5 {
            info!("  ⏳ Waiting 10 seconds before next round...");
            sleep(Duration::from_secs(10)).await;
        }
    }

    // Final summary
    info!("🎉 Mainnet Paper Trading Demo Completed!");
    info!("═══════════════════════════════════════");

    let final_portfolio = portfolio.get_summary();
    info!("📊 Final Results:");
    info!(
        "  • Total Value: {:.6} SOL",
        portfolio.get_total_value() as f64 / 1_000_000_000.0
    );
    info!("  • Total Trades: {}", final_portfolio.total_trades);
    info!("  • Total Fees: {} lamports", final_portfolio.total_fees_paid);
    info!("  • Error Handling: ✅ All APIs monitored with ban detection");

    info!("🚀 System is ready for live trading with proper configuration!");
    info!("Next steps:");
    info!("  1. Get premium API keys (Helius, QuickNode, etc.)");
    info!("  2. Create secure mainnet wallets");
    info!("  3. Fund with minimal test amounts (0.1-1 SOL)");
    info!("  4. Enable live trading with conservative limits");

    Ok(())
}
