pub mod websocket;
mod arbitrage;
mod config;
mod dex;
mod error;
mod metrics;
mod solana;

use arbitrage::dynamic_threshold::{VolatilityTracker, recommended_min_profit_threshold};

use arbitrage::engine::ArbitrageEngine;
use config::check_and_print_env_vars;
use dex::get_all_clients_arc;
use metrics::Metrics;
use solana::rpc::SolanaRpcClient;
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::RwLock;
use tokio::time::interval;
use arbitrage::executor::ArbitrageExecutor;
use solana_sdk::signature::read_keypair_file;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    check_and_print_env_vars();

    let mut metrics = Metrics::new();
    let dex_clients = get_all_clients_arc().await;

    // ✅ Ensure `pools` matches expected type: `Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>>`
    let pools = Arc::new(RwLock::new(HashMap::new()));

    let min_profit_threshold = 0.01; // Example threshold
    let max_slippage = 0.005; // Example slippage value
    let tx_fee_lamports = 5000; // Example transaction fee

    // ✅ Correct SolanaRpcClient initialization with expected argument types
    let primary_endpoint = "https://api.mainnet-beta.solana.com"; // ✅ Use &str directly
    let fallback_endpoints = vec![
        "https://fallback-rpc.solana.com".to_string(),
        "https://backup-rpc.solana.com".to_string(),
    ]; // ✅ Uses Vec<String>

    let max_retries = 3;
    let retry_delay = Duration::from_secs(5);

    let solana_client = Arc::new(SolanaRpcClient::new(
        primary_endpoint,
        fallback_endpoints,
        max_retries,
        retry_delay,
    )); // ✅ Fixes type mismatch

    // Create the raw Solana RpcClient for ArbitrageExecutor
    let rpc_url = std::env::var("RPC_URL").unwrap_or_else(|_| primary_endpoint.to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(rpc_url));

    let paper_trading = env::var("PAPER_TRADING").unwrap_or_else(|_| "false".to_string());
    let simulation_mode = paper_trading.to_lowercase() == "true";

    let wallet_path = env::var("TRADER_WALLET_KEYPAIR_PATH").expect("TRADER_WALLET_KEYPAIR_PATH must be set");
    let wallet = Arc::new(read_keypair_file(wallet_path).expect("Failed to read keypair file"));

    // Create ArbitrageExecutor with simulation_mode and correct RpcClient type
    let executor = ArbitrageExecutor::new(
        wallet,
        rpc_client,
        tx_fee_lamports,
        Duration::from_secs(30), // Example timeout
        simulation_mode,
    );

    let arbitrage_engine = Arc::new(ArbitrageEngine::new(
        pools,
        min_profit_threshold,
        max_slippage,
        tx_fee_lamports,
        None, // No websocket manager for now
        None, // No price provider for now
    ));

    // Initialize volatility tracker with a window of 20 samples
    let mut volatility_tracker = VolatilityTracker::new(20);
    
    // Spawn a background task to update dynamic threshold periodically
    let engine_handle = arbitrage_engine.clone();
    let threshold_task = tokio::spawn(async move {
        let mut intv = interval(Duration::from_secs(10));
        loop {
            intv.tick().await;
            // Example: Sample most recent price (stub: 1.0, replace with real price)
            let simulated_price = 1.0;  // TODO: Pull from actual price feed!
            volatility_tracker.add_price(simulated_price);
            let vol = volatility_tracker.volatility();
            let new_threshold = recommended_min_profit_threshold(vol);
            engine_handle.set_min_profit_threshold(new_threshold).await;
            println!("Dynamic threshold updated: {:.4}", new_threshold);
        }
    });

    // Wait for shutdown signal
    signal::ctrl_c()
        .await
        .expect("Failed to listen for shutdown");
    
    // Cancel the threshold update task
    threshold_task.abort();

    println!("✅ Arbitrage bot stopping...");

    Ok(())
}
