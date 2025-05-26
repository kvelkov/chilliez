// src/main.rs
mod arbitrage; // Ensure all submodules are declared if not in lib.rs
mod cache;
mod config;
mod dex;
mod error;
mod metrics;
mod solana;
pub mod utils;
pub mod websocket;

use crate::{
    arbitrage::{engine::ArbitrageEngine, executor::ArbitrageExecutor}, // Removed unused VolatilityTracker, MultiHopArbOpportunity direct imports
    cache::Cache,
    config::load_config,
    dex::get_all_clients_arc,
    error::ArbError,
    metrics::Metrics, // Removed TradeOutcome
    solana::{
        rpc::SolanaRpcClient,
        websocket::{SolanaWebsocketManager, RawAccountUpdate}, // Removed WebsocketUpdate
    },
    utils::{setup_logging, PoolInfo}, // Removed load_keypair (used via read_keypair_file)
    websocket::CryptoDataProvider,
};
use log::{info, warn}; // Removed debug, error
use solana_client::nonblocking::rpc_client::RpcClient as NonBlockingRpcClient;
use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file, signer::Signer};
use std::collections::HashMap;
use std::env; // Added for reading SIMULATION_MODE directly
use std::sync::Arc;
use std::time::Duration;
// use tokio::signal; // Removed unused import
use tokio::sync::{Mutex, RwLock};
use tokio::time::interval; // Removed Instant

#[tokio::main]
async fn main() -> Result<(), ArbError> {
    setup_logging().expect("Failed to initialize logging");
    info!("Solana Arbitrage Bot starting...");

    let app_config = load_config()?;
    info!("Configuration loaded and validated successfully.");

    let metrics = Arc::new(Mutex::new(Metrics::new(
        app_config.sol_price_usd.unwrap_or(100.0),
        app_config.metrics_log_path.clone(),
    )));
    metrics.lock().await.log_launch();

    info!("Initializing Solana RPC clients...");
    let primary_rpc_endpoint = app_config.rpc_url.clone();
    let fallback_rpc_endpoints_str = app_config.rpc_url_backup.clone().unwrap_or_default();

    let ha_solana_rpc_client = Arc::new(SolanaRpcClient::new(
        &primary_rpc_endpoint, fallback_rpc_endpoints_str,
        app_config.rpc_max_retries.unwrap_or(3),
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
    metrics.lock().await.log_pools_fetched(0);
    info!("Initial pool data map initialized.");

    let wallet_path = app_config.trader_wallet_keypair_path.clone();
    let wallet = match read_keypair_file(&wallet_path) {
        Ok(kp) => Arc::new(kp),
        Err(e) => return Err(ArbError::ConfigError(format!("Failed to read wallet keypair '{}': {}", wallet_path, e))),
    };
    info!("Trader wallet loaded: {}", wallet.pubkey());

    // Read SIMULATION_MODE directly from environment as a workaround
    // Ideally, simulation_mode should be part of the Config struct in settings.rs
    let simulation_mode_from_env = env::var("SIMULATION_MODE")
        .unwrap_or_else(|_| "false".to_string()) // Default to "false" if not set
        .parse::<bool>()
        .unwrap_or(false); // Default to false if parsing fails

    let tx_executor = Arc::new(ArbitrageExecutor::new(
        wallet.clone(), direct_rpc_client.clone(),
        app_config.default_priority_fee_lamports,
        Duration::from_secs(app_config.max_transaction_timeout_seconds),
        simulation_mode_from_env, // Using value read directly from env
        app_config.paper_trading,
    ).with_solana_rpc(ha_solana_rpc_client.clone()));
    info!("ArbitrageExecutor initialized.");

    let ws_manager_instance = if !app_config.ws_url.is_empty() {
        let (manager, _raw_receiver) = SolanaWebsocketManager::new(
            app_config.ws_url.clone(),
            app_config.rpc_url_backup.clone().unwrap_or_default(),
            app_config.ws_update_channel_size.unwrap_or(1024),
        );
        Some(Arc::new(Mutex::new(manager)))
    } else { warn!("WebSocket URL not configured."); None };

    let arbitrage_engine = Arc::new(ArbitrageEngine::new(
        pools_map.clone(), Some(ha_solana_rpc_client.clone()),
        app_config.clone(), metrics.clone(), None,
        ws_manager_instance.clone(), dex_api_clients.clone(),
    ));
    info!("ArbitrageEngine initialized.");
    arbitrage_engine.start_services().await;

    let engine_for_threshold_task = Arc::clone(&arbitrage_engine);
    let mut threshold_task_handle = tokio::spawn(async move {
        engine_for_threshold_task.run_dynamic_threshold_updates().await;
        info!("Dynamic threshold update task finished.");
    });

    let executor_for_congestion_task = Arc::clone(&tx_executor);
    let congestion_update_interval_secs = app_config.congestion_update_interval_secs.unwrap_or(30);
    let mut congestion_task_handle = tokio::spawn(async move {
        info!("Starting congestion update task (interval: {}s).", congestion_update_interval_secs);
        let mut interval_timer = interval(Duration::from_secs(congestion_update_interval_secs));
        loop {
            interval_timer.tick().await;
            if let Err(e) = executor_for_congestion_task.update_network_congestion().await {
                warn!("Failed to update network congestion: {}", e);
            }
        }
    });
    
    let engine_for_health_task = Arc::clone(&arbitrage_engine);
    // The health check task in main.rs uses the interval from app_config
    let health_check_interval_from_config = Duration::from_secs(app_config.health_check_interval_secs.unwrap_or(60));
    let mut health_check_task_handle = tokio::spawn(async move {
        info!("Starting health check task (interval: {:?}).", health_check_interval_from_config);
        let mut interval_timer = interval(health_check_interval_from_config);
        loop {
            interval_timer.tick().await;
            engine_for_health_task.run_health_checks().await;
        }
    });

    info!("Starting main arbitrage detection cycle (interval: {}s).", app_config.cycle_interval_seconds);
    let mut main_cycle_interval = interval(Duration::from_secs(app_config.cycle_interval_seconds));
    main_cycle_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    // Main select loop (ensure this part is complete and correct in your actual file)
    // The provided snippet for the main loop was:
    // loop { /* ... main select loop remains largely the same as previous corrected version ... */ }
    // This loop needs to be implemented for the bot to function.
    // For now, I will add a placeholder for the loop structure.
    loop {
        tokio::select! {
            _ = main_cycle_interval.tick() => {
                info!("Main arbitrage cycle tick.");
                // Implement your arbitrage detection and execution logic here
                // For example:
                // if let Ok(opportunities) = arbitrage_engine.detect_arbitrage().await {
                //    info!("Detected {} opportunities.", opportunities.len());
                //    for opp in opportunities {
                //        // Potentially execute opportunity
                //        // tx_executor.execute_opportunity(&opp).await;
                //    }
                // }
            },
            // Example for WebSocket updates if ws_manager_instance is Some
            // Ensure this part correctly uses `WebsocketUpdate` if it's re-added to imports
            // and the ws_manager_instance logic is active.
            // update_result = async {
            //     if let Some(manager) = &ws_manager_instance {
            //         manager.lock().await.try_recv_update().await // Assuming try_recv_update is async
            //     } else {
            //         futures::future::pending().await // Keeps the arm pending if no ws_manager
            //     }
            // }, if ws_manager_instance.is_some() => {
            //     match update_result {
            //         Ok(Some(ws_update)) => {
            //             // Process ws_update (this is where crate::solana::websocket::WebsocketUpdate would be used)
            //             info!("Received WS update: {:?}", ws_update);
            //             // Example: if let crate::solana::websocket::WebsocketUpdate::PoolUpdate(pool_info) = ws_update {
            //             //     arbitrage_engine.pools.write().await.insert(pool_info.address, Arc::new(pool_info));
            //             // }
            //         },
            //         Ok(None) => { /* Channel was empty */ }
            //         Err(_e) => { /* Channel closed or error */ }
            //         _ => { /* Should not happen with Option<WebsocketUpdate> */ }
            //     }
            // }
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
    Ok(())
}