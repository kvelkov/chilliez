mod arbitrage;
mod config;
mod dex;
mod error;
mod metrics;
mod solana;
pub mod utils;
pub mod websocket;

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

    // --- Load and validate config (typed, env-driven) ---
    let _config = config::env::load_config();
    check_and_print_env_vars();

    let metrics = Arc::new(Mutex::new(Metrics::new()));
    metrics.lock().await.log_launch();
    let dex_clients = get_all_clients_arc().await;

    let pools: Arc<RwLock<HashMap<solana_sdk::pubkey::Pubkey, Arc<PoolInfo>>>> =
        Arc::new(RwLock::new(HashMap::new()));
    metrics
        .lock()
        .await
        .log_pools_fetched(pools.read().await.len());

    let min_profit_threshold = 0.01;
    let max_slippage = 0.005;
    let tx_fee_lamports = 5000;

    let primary_endpoint = "https://api.mainnet-beta.solana.com";
    let fallback_endpoints = vec![
        "https://fallback-rpc.solana.com".to_string(),
        "https://backup-rpc.solana.com".to_string(),
    ];

    let max_retries = 3;
    let retry_delay = Duration::from_secs(5);

    let solana_client = Arc::new(SolanaRpcClient::new(
        primary_endpoint,
        fallback_endpoints,
        max_retries,
        retry_delay,
    ));

    let rpc_url = std::env::var("RPC_URL").unwrap_or_else(|_| primary_endpoint.to_string());
    let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
        rpc_url,
    ));

    let paper_trading = env::var("PAPER_TRADING").unwrap_or_else(|_| "false".to_string());
    let simulation_mode = paper_trading.to_lowercase() == "true";

    let wallet_path =
        env::var("TRADER_WALLET_KEYPAIR_PATH").expect("TRADER_WALLET_KEYPAIR_PATH must be set");
    let wallet = Arc::new(read_keypair_file(wallet_path).expect("Failed to read keypair file"));

    let executor = Arc::new(ArbitrageExecutor::new(
        wallet,
        rpc_client,
        tx_fee_lamports,
        Duration::from_secs(30),
        simulation_mode,
    ));

    let arbitrage_engine = Arc::new(ArbitrageEngine::new(
        pools.clone(),
        min_profit_threshold,
        max_slippage,
        tx_fee_lamports,
        None,
        None,
    ));

    // -- RUST BOT ENTRY POINT --

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

    let multihop_opps = arbitrage_engine.discover_multihop_opportunities().await;
    println!(
        "[ENGINE API] Found {} multi-hop arbitrage opportunities",
        multihop_opps.len()
    );

    let profitable_opps: Vec<_> = multihop_opps
        .into_iter()
        .filter(|opp| opp.is_profitable(min_profit_threshold))
        .collect();

    for opp in &profitable_opps {
        opp.log_summary();
        opp.log_hop();
        let mut m = metrics.lock().await;
        m.log_opportunity(
            &metrics::TradingPair(opp.input_token.clone(), opp.output_token.clone()),
            opp.profit_pct,
        );
        m.record_arbitrage_opportunity(
            opp.profit_pct,
            &opp.input_token,
            &opp.output_token,
            opp.input_amount,
            opp.expected_output,
        );
    }

    let exec_futs = profitable_opps.iter().map(|opp| {
        let executor = executor.clone();
        let engine = arbitrage_engine.clone();
        let metrics = metrics.clone();
        async move {
            let pools_opt = engine.resolve_pools_for_opportunity(opp).await;
            let pools_vec = match pools_opt {
                Some(p) => p,
                None => {
                    println!("[ENGINE] Skipping trade: missing pool info");
                    return;
                }
            };

            let pool_refs: Vec<&PoolInfo> = pools_vec.iter().map(|arc| arc.as_ref()).collect();

            let (total_profit, total_slippage, total_fee) =
                crate::arbitrage::calculator::calculate_multihop_profit_and_slippage(
                    &pool_refs,
                    opp.input_amount,
                    &vec![true; opp.hops.len()],
                    &vec![(None, None, None); opp.hops.len()],
                );

            let fee_lamports = total_fee as u64;
            if !engine.should_execute_trade(total_slippage, fee_lamports) {
                println!(
                    "[ENGINE] Skipping trade: slippage {:.5} or fee {} too high",
                    total_slippage, fee_lamports
                );
                return;
            }

            match executor.execute_multihop(opp).await {
                Ok(sig) => {
                    println!("✅ Executed multi-hop trade: txid={}", sig);
                    let mut m = metrics.lock().await;
                    m.record_execution_result(true, tx_fee_lamports as f64, 0.0);
                    m.log_trade_result(
                        (opp.input_token.as_str(), opp.output_token.as_str()),
                        opp.total_profit,
                        simulation_mode,
                    );
                }
                Err(e) => {
                    eprintln!("❌ Execution failed: {}", e);
                    let mut m = metrics.lock().await;
                    m.record_execution_result(false, tx_fee_lamports as f64, 0.0);
                    m.log_trade_result(
                        (opp.input_token.as_str(), opp.output_token.as_str()),
                        0.0,
                        simulation_mode,
                    );
                }
            }
        }
    });

    join_all(exec_futs).await;

    signal::ctrl_c()
        .await
        .expect("Failed to listen for shutdown");

    threshold_task.abort();
    println!("🛑 Arbitrage bot stopping...");

    metrics.lock().await.summary();

    Ok(())
}
