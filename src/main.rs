// src/main.rs
mod arbitrage; 
mod cache;
mod config;
mod dex;
mod error;
mod metrics;
mod solana;
pub mod utils;
pub mod websocket; // Ensuring this is present

use crate::{
    arbitrage::{
        calculator, 
        // detector::ArbitrageDetector, // No longer need to instantiate detector here
        engine::ArbitrageEngine, 
        executor::ArbitrageExecutor,
    },
    cache::Cache,
    config::load_config,
    dex::get_all_clients_arc,
    error::ArbError,
    metrics::Metrics,
    solana::{
        rpc::SolanaRpcClient,
        websocket::SolanaWebsocketManager,
    },
    utils::{setup_logging, PoolInfo, ProgramConfig},
    websocket::CryptoDataProvider, // Added for price_provider in Engine
};
use log::{error, info, warn}; 
use solana_client::nonblocking::rpc_client::RpcClient as NonBlockingRpcClient;
use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file, signer::Signer};
use std::collections::HashMap;
use std::env; 
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};
use tokio::time::interval; 

#[tokio::main]
async fn main() -> Result<(), ArbError> {
    setup_logging().expect("Failed to initialize logging");
    info!("Solana Arbitrage Bot starting...");

    let app_config = load_config()?;
    info!("Application configuration loaded and validated successfully.");

    let program_config = ProgramConfig::new("ChillarezBot".to_string(), env!("CARGO_PKG_VERSION").to_string());
    program_config.log_details();

    let metrics = Arc::new(Mutex::new(Metrics::new(
        app_config.sol_price_usd.unwrap_or(100.0), 
        app_config.metrics_log_path.clone(),
    )));
    metrics.lock().await.log_launch();

    info!("Initializing Solana RPC clients...");
    let primary_rpc_endpoint = app_config.rpc_url.clone();
    let fallback_rpc_endpoints_str = app_config.rpc_url_backup.clone().unwrap_or_default();

    let fallback_rpc_endpoints: Vec<String> = app_config.rpc_url_backup
        .as_ref()
        .map(|s| s.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_else(Vec::new);
    let ha_solana_rpc_client = Arc::new(SolanaRpcClient::new(
        &primary_rpc_endpoint, fallback_rpc_endpoints,
        app_config.rpc_max_retries.unwrap_or(3) as usize,
        Duration::from_millis(app_config.rpc_retry_delay_ms.unwrap_or(500)),
    ));
    info!("High-availability Solana RPC client initialized.");

    let direct_rpc_client = Arc::new(NonBlockingRpcClient::new_with_commitment(
        primary_rpc_endpoint.clone(),
        solana_sdk::commitment_config::CommitmentConfig::confirmed(),
    ));
    info!("Direct non-blocking Solana RPC client initialized.");

    let redis_cache = Arc::new(
        Cache::new(&app_config.redis_url, app_config.redis_default_ttl_secs).await
            .map_err(|e| ArbError::ConfigError(format!("Redis cache init failed: {}", e)))?,
    );
    info!("Redis cache initialized successfully.");

    let dex_api_clients = get_all_clients_arc(Arc::clone(&redis_cache), Arc::clone(&app_config)).await;
    info!("DEX API clients initialized.");

    let pools_map: Arc<RwLock<HashMap<Pubkey, Arc<PoolInfo>>>> = Arc::new(RwLock::new(HashMap::new()));
    metrics.lock().await.log_pools_fetched(pools_map.read().await.len());
    info!("Initial pool data map initialized (current size: {}).", pools_map.read().await.len());

    let wallet_path = app_config.trader_wallet_keypair_path.clone().unwrap_or(app_config.wallet_path.clone());
    let wallet = match read_keypair_file(&wallet_path) {
        Ok(kp) => Arc::new(kp),
        Err(e) => return Err(ArbError::ConfigError(format!("Failed to read wallet keypair '{}': {}", wallet_path, e))),
    };
    info!("Trader wallet loaded: {}", wallet.pubkey());

    let simulation_mode_from_env = env::var("SIMULATION_MODE")
        .unwrap_or_else(|_| "false".to_string()) 
        .parse::<bool>()
        .unwrap_or(false); 

    let tx_executor: Arc<ArbitrageExecutor> = Arc::new(ArbitrageExecutor::new(
        wallet.clone(), direct_rpc_client.clone(),
        app_config.default_priority_fee_lamports,
        Duration::from_secs(app_config.max_transaction_timeout_seconds.unwrap_or(60)),
        simulation_mode_from_env, 
        app_config.paper_trading,
    ).with_solana_rpc(ha_solana_rpc_client.clone()));
    info!("ArbitrageExecutor initialized.");

    let ws_fallback_urls: Vec<String> = app_config.rpc_url_backup
        .as_ref()
        .map(|s| s.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_else(Vec::new);
    let ws_manager_instance = if !app_config.ws_url.is_empty() {
        let (manager, _raw_update_receiver) = SolanaWebsocketManager::new(
            app_config.ws_url.clone(),
            ws_fallback_urls,
            app_config.ws_update_channel_size.unwrap_or(1024),
        );
        Some(Arc::new(Mutex::new(manager)))
    } else { warn!("WebSocket URL not configured, WebSocket manager not started."); None };

    // price_provider can be initialized here if a concrete implementation is chosen
    let price_provider_instance: Option<Arc<dyn CryptoDataProvider + Send + Sync>> = None; // Placeholder
    // Example: if you had a CoinGeckoProvider:
    // let price_provider_instance = Some(Arc::new(CoinGeckoProvider::new()));

    let arbitrage_engine: Arc<ArbitrageEngine> = Arc::new(ArbitrageEngine::new(
        pools_map.clone(),
        ws_manager_instance.clone(),
        price_provider_instance,
        Some(ha_solana_rpc_client.clone()),
        app_config.clone(),
        metrics.clone(),
        dex_api_clients.clone(),
    ));
    info!("ArbitrageEngine initialized.");
    
    if ws_manager_instance.is_some() {
        arbitrage_engine.start_services().await; 
    }

    let engine_for_threshold_task: Arc<ArbitrageEngine> = Arc::clone(&arbitrage_engine);
    let threshold_task_handle = tokio::spawn(async move {
        engine_for_threshold_task.run_dynamic_threshold_updates().await;
        info!("Dynamic threshold update task finished (normally runs indefinitely).");
    });

    let executor_for_congestion_task: Arc<ArbitrageExecutor> = Arc::clone(&tx_executor); 
    let congestion_update_interval_secs = app_config.congestion_update_interval_secs.unwrap_or(30);
    let congestion_task_handle = tokio::spawn(async move {
        info!("Starting congestion update task (interval: {}s).", congestion_update_interval_secs);
        let mut interval_timer = interval(Duration::from_secs(congestion_update_interval_secs));
        loop {
            interval_timer.tick().await;
            if let Err(e) = executor_for_congestion_task.update_network_congestion().await {
                warn!("Failed to update network congestion: {}", e);
            }
        }
    });
    
    let engine_for_health_task: Arc<ArbitrageEngine> = Arc::clone(&arbitrage_engine);
    let health_check_interval_from_config = Duration::from_secs(app_config.health_check_interval_secs.unwrap_or(60));
    let health_check_task_handle = tokio::spawn(async move {
        info!("Starting health check task (interval: {:?}).", health_check_interval_from_config);
        let mut interval_timer = interval(health_check_interval_from_config);
        loop {
            interval_timer.tick().await;
            engine_for_health_task.run_health_checks().await;
            let status_string = engine_for_health_task.get_current_status_string().await;
            info!("Engine Status: {}", status_string);
        }
    });

    info!("Starting main arbitrage detection cycle (interval: {:?}s).", app_config.cycle_interval_seconds);
    let mut main_cycle_interval = interval(Duration::from_secs(app_config.cycle_interval_seconds.unwrap_or(10)));
    main_cycle_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = main_cycle_interval.tick() => {
                let current_time_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                info!("Main arbitrage cycle tick at {}.", current_time_str);
                metrics.lock().await.increment_main_cycles();
                let cycle_start_time = std::time::Instant::now();

                calculator::clear_caches_if_needed();

                // Use the primary detection method from the engine
                match arbitrage_engine.detect_arbitrage().await {
                    Ok(mut all_opportunities) => { // Made mutable for sorting
                        if !all_opportunities.is_empty() {
                            info!("Detected {} total opportunities in this cycle.", all_opportunities.len());
                            all_opportunities.sort_by(|a, b| b.profit_pct.partial_cmp(&a.profit_pct).unwrap_or(std::cmp::Ordering::Equal));
                            
                            if let Some(best_opp) = all_opportunities.first() {
                                info!("Best opportunity ID: {} with profit: {:.4}%", best_opp.id, best_opp.profit_pct);
                                if app_config.paper_trading || simulation_mode_from_env {
                                    info!("Paper/Simulation Mode: Logging execution for opportunity {}", best_opp.id);
                                    match tx_executor.execute_opportunity(best_opp).await {
                                        Ok(signature) => {
                                            info!("(Paper/Simulated) Successfully processed opportunity {} with signature: {}", best_opp.id, signature);
                                        }
                                        Err(e) => {
                                            error!("(Paper/Simulated) Failed to process opportunity {}: {}", best_opp.id, e);
                                        }
                                    }
                                } else {
                                    // Real trading logic would go here
                                    info!("Real Trading Mode: Attempting to execute opportunity {}", best_opp.id);
                                     match tx_executor.execute_opportunity(best_opp).await {
                                        Ok(signature) => {
                                            info!("Successfully EXECUTED opportunity {} with signature: {}", best_opp.id, signature);
                                        }
                                        Err(e) => {
                                            error!("Failed to EXECUTE opportunity {}: {}", best_opp.id, e);
                                        }
                                    }
                                }
                            }
                        } else {
                            info!("No arbitrage opportunities found in this cycle.");
                        }
                    }
                    Err(e) => {
                        error!("Error during arbitrage detection cycle: {}", e);
                    }
                }
                metrics.lock().await.record_main_cycle_duration(cycle_start_time.elapsed().as_millis() as u64);
            },
            _ = tokio::signal::ctrl_c() => {
                info!("CTRL-C received, shutting down tasks...");
                threshold_task_handle.abort();
                congestion_task_handle.abort();
                health_check_task_handle.abort();
                info!("Background tasks aborted. Exiting.");
                break;
            }
        }
    }
    metrics.lock().await.summary();
    Ok(())
}