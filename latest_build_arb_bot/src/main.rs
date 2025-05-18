mod api;
mod arbitrage;
pub mod cache;
mod config;
mod dex;
mod error;
mod metrics;
mod solana;
pub mod utils;
pub mod websocket;

use crate::api::fetch_ai_suggestions;
use crate::cache::Cache; // Use Redis caching module
use crate::utils::{DexType, PoolInfo, TokenAmount};
use arbitrage::dynamic_threshold::{recommended_min_profit_threshold, VolatilityTracker};
use arbitrage::engine::ArbitrageEngine;
use arbitrage::executor::ArbitrageExecutor;
use config::check_and_print_env_vars;
use dex::get_all_clients_arc;
use futures::future::join_all;
use metrics::Metrics;
use solana::rpc::SolanaRpcClient;
use solana_sdk::signature::read_keypair_file;
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::{Mutex, RwLock};
use tokio::time::interval;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    // Initialize Redis cache
    let cache = Cache::new("redis://127.0.0.1/").await;

    // Load & validate config
    let _config = config::env::load_config();
    check_and_print_env_vars();

    let metrics = Arc::new(Mutex::new(Metrics::new()));
    metrics.lock().await.log_launch();
    let dex_clients = get_all_clients_arc().await;

    let primary_endpoint = "https://api.mainnet-beta.solana.com";
    let rpc_url = std::env::var("RPC_URL").unwrap_or_else(|_| primary_endpoint.to_string());

    // Implement caching for market data retrieval
    let market_data_cache_key = "solana_market_data";
    let market_data = match cache.get_cached_data(market_data_cache_key).await {
        Some(cached_data) => {
            println!("✅ Using cached market data.");
            cached_data
        }
        None => {
            println!("🔄 Fetching fresh market data...");
            let fresh_data = api::fetch_ai_suggestions(&rpc_url).await?;
            cache
                .store_in_cache(market_data_cache_key, &fresh_data, 3600)
                .await; // Cache for 1 hour
            fresh_data
        }
    };

    println!("✅ Market Data Loaded: {}", market_data);

    // -- RUST BOT ENTRY POINT --

    let arbitrage_engine = Arc::new(ArbitrageEngine::new(
        Arc::new(RwLock::new(HashMap::new())),
        0.01,
        0.005,
        5000,
        None,
        None,
    ));

    arbitrage_engine.start_services().await;

    let mut volatility_tracker = VolatilityTracker::new(20);
    let engine_handle = arbitrage_engine.clone();

    let threshold_task = tokio::spawn(async move {
        let mut intv = interval(Duration::from_secs(10));
        loop {
            intv.tick().await;
            let simulated_price = 1.0; // TODO: Pull real price from price feed
            volatility_tracker.add_price(simulated_price);
            let vol = volatility_tracker.volatility();
            let new_threshold = recommended_min_profit_threshold(vol);
            engine_handle.set_min_profit_threshold(new_threshold).await;
            println!("📈 Dynamic threshold updated: {:.4}", new_threshold);
        }
    });

    signal::ctrl_c()
        .await
        .expect("Failed to listen for shutdown");

    threshold_task.abort();
    println!("🛑 Arbitrage bot stopping...");
    metrics.lock().await.summary();

    Ok(())
}
