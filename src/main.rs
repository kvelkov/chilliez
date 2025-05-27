// /Users/kiril/Desktop/chilliez/src/main.rs
mod arbitrage;
mod cache;
mod config;
mod dex;
mod error;
mod metrics;
mod solana;
pub mod utils;
pub mod websocket;

use crate::error::ArbResult;
use crate::{
    arbitrage::{
        calculator,
        engine::ArbitrageEngine,
        executor::ArbitrageExecutor,
    },
    cache::Cache, // Keep Cache import
    config::init_and_get_config, // ArbError is used
    dex::get_all_clients_arc,
    error::ArbError, // CircuitBreaker and RetryPolicy are not directly used in main
    metrics::Metrics,
    solana::{
        rpc::SolanaRpcClient,
        websocket::{SolanaWebsocketManager, RawAccountUpdate, WebsocketUpdate}, // Added RawAccountUpdate, WebsocketUpdate
        // Ensure FutureExt is in scope if not already via other imports, or add `use futures::FutureExt;`
    },
    utils::{setup_logging, PoolInfo, ProgramConfig},
    websocket::CryptoDataProvider,
};
use log::{error, info, warn};
use solana_client::nonblocking::rpc_client::RpcClient as NonBlockingRpcClient;
use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file, signer::Signer};
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, Mutex, RwLock}; // Added broadcast
use tokio::time::interval;
use futures::FutureExt; // Add this for .boxed()

#[tokio::main]
async fn main() -> ArbResult<()> {
    setup_logging().expect("Failed to initialize logging");
    info!("Solana Arbitrage Bot starting...");

    let app_config = init_and_get_config();
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

    // Initialize WebSocket Manager components
    // ws_setup_opt will hold the manager Arc and the initial raw updates receiver
    let ws_setup_opt: Option<(Arc<Mutex<SolanaWebsocketManager>>, broadcast::Receiver<RawAccountUpdate>)> = if !app_config.ws_url.is_empty() {
        let (manager, raw_updates_rx_for_main) = SolanaWebsocketManager::new(
            app_config.ws_url.clone(),
            app_config.rpc_url_backup
                .as_ref()
                .map(|s| s.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
                .unwrap_or_else(Vec::new),
            app_config.ws_update_channel_size.unwrap_or(1024),
        );
        // Start the websocket manager (heartbeat, reconnection)
        let manager_arc = Arc::new(Mutex::new(manager));
        {
            let mut mgr = manager_arc.lock().await;
            let _ = mgr.start(Some(redis_cache.clone())).await;
        }
        Some((manager_arc, raw_updates_rx_for_main))
    } else {
        warn!("WebSocket URL not configured, WebSocket manager not started.");
        None
    };


    // Initialize ArbitrageEngine BEFORE ArbitrageExecutor
    let price_provider_instance: Option<Arc<dyn CryptoDataProvider + Send + Sync>> = None; // Assuming no price provider for now

    let arbitrage_engine: Arc<ArbitrageEngine> = Arc::new(ArbitrageEngine::new(
        pools_map.clone(),
        ws_setup_opt.as_ref().map(|(manager_arc, _rx)| manager_arc.clone()), // Engine gets the manager Arc
        price_provider_instance,
        Some(ha_solana_rpc_client.clone()),
        app_config.clone(),
        metrics.clone(),
        dex_api_clients.clone(),
    ));
    info!("ArbitrageEngine initialized.");

    let simulation_mode_from_env = env::var("SIMULATION_MODE")
        .unwrap_or_else(|_| "false".to_string())
        .parse::<bool>()
        .unwrap_or(false);

    let tx_executor: Arc<ArbitrageExecutor> = Arc::new(ArbitrageExecutor::new(
        wallet.clone(), direct_rpc_client.clone(),
        app_config.default_priority_fee_lamports,
        Duration::from_secs(app_config.max_transaction_timeout_seconds.expect("max_transaction_timeout_seconds must be set")),
        simulation_mode_from_env,
        app_config.paper_trading, // Correctly pass paper_trading_mode
        arbitrage_engine.get_circuit_breaker(), // Pass the engine's circuit breaker
        arbitrage_engine.get_retry_policy().clone(), // Pass a clone of the engine's retry policy
        None, // event_sender: Option<tokio::sync::mpsc::Sender<ExecutorEvent>>
    ).with_solana_rpc(ha_solana_rpc_client.clone())); // Add None for the event_sender
    info!("ArbitrageExecutor initialized.");

    if let Some((ws_manager_arc, raw_updates_rx_consumer)) = ws_setup_opt { // Take ownership of components
        arbitrage_engine.start_services(Some(Arc::clone(&redis_cache))).await;

        // Dummy subscription to make subscribe_to_account live
        let dummy_pubkey_to_sub = solana_sdk::system_program::id();
        info!("[Main] Subscribing to dummy account {} for testing purposes.", dummy_pubkey_to_sub);
        if let Err(e) = ws_manager_arc.lock().await.subscribe_to_account(dummy_pubkey_to_sub, Some(redis_cache.clone())).await {
            warn!("[Main] Failed to subscribe to dummy account: {}", e);
        }

        // Spawn RawAccountUpdate consumer task only if ws_manager_opt is Some
        // We use the raw_updates_rx_consumer obtained when ws_manager was initialized.
        let mut actual_raw_updates_rx = raw_updates_rx_consumer; // Move receiver into the task
        tokio::spawn(async move {
            info!("[RawUpdateLogger] Started");
            loop {
                match actual_raw_updates_rx.recv().await {
                    Ok(update) => {
                        info!(
                            "[RawUpdateLogger] Update: Pubkey={}, DataLen={:?}, Timestamp={:?}",
                            update.pubkey(),
                            update.data().map(|d| d.len()),
                            // Extract timestamp if possible
                            match &update {
                                RawAccountUpdate::Account { timestamp, .. } => Some(timestamp),
                                RawAccountUpdate::Disconnected { timestamp, .. } => Some(timestamp),
                                RawAccountUpdate::Error { timestamp, .. } => Some(timestamp),
                            }
                        );
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        warn!("[RawUpdateLogger] Lagged behind, some raw updates were missed.");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("[RawUpdateLogger] Broadcast channel closed. Logger stopping.");
                        break;
                    }
                }
            }
            info!("[RawUpdateLogger] Stopped");
        });
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

    info!("Starting main arbitrage detection cycle (interval: {}s).", app_config.cycle_interval_seconds.expect("cycle_interval_seconds must be set"));
    let mut main_cycle_interval = interval(Duration::from_secs(app_config.cycle_interval_seconds.expect("cycle_interval_seconds must be set")));
    main_cycle_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    // Create a separate interval for the fixed input/multi-hop detection cycle part
    let mut fixed_input_cycle_interval = interval(Duration::from_secs(app_config.cycle_interval_seconds.expect("cycle_interval_seconds must be set"))); // Or a different interval if needed
    fixed_input_cycle_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    // Setup shutdown signal for the new WebSocket processing task
       let (shutdown_tx_ws_processor, _shutdown_rx_ws_processor) = broadcast::channel(1); // Prefixed for now
    let mut ws_processor_task_handle = None;

    // Check if the engine has a WS manager configured for its processing loop
    if arbitrage_engine.ws_manager.is_some() {
        let engine_for_ws_loop = Arc::clone(&arbitrage_engine);
        info!("[Main] Spawning ArbitrageEngine's WebSocket update processing loop.");
        ws_processor_task_handle = Some(tokio::spawn(async move {
            // The loop inside process_websocket_updates_loop will handle repeated calls to try_recv_update.
            engine_for_ws_loop.process_websocket_updates_loop().await;
        }));
    } else {
        warn!("[Main] WebSocket manager not configured, ArbitrageEngine's WebSocket update processing loop will not be started.");
    }
    
    loop {
        tokio::select! {
            _ = main_cycle_interval.tick() => {
                let current_time_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                info!("Main arbitrage cycle tick at {}.", current_time_str);
                metrics.lock().await.increment_main_cycles();
                let cycle_start_time = std::time::Instant::now();

                calculator::clear_caches_if_needed();

                match arbitrage_engine.detect_arbitrage().await {
                    Ok(mut all_opportunities) => {
                        if !all_opportunities.is_empty() {
                            info!("Detected {} total opportunities in this cycle.", all_opportunities.len());
                            all_opportunities.sort_by(|a, b| b.profit_pct.partial_cmp(&a.profit_pct).unwrap_or(std::cmp::Ordering::Equal));

                            if let Some(best_opp) = all_opportunities.first() {
                                info!("Best opportunity ID: {} with profit: {:.4}%", best_opp.id, best_opp.profit_pct);
                                if app_config.paper_trading || simulation_mode_from_env {
                                    info!("Paper/Simulation Mode: Logging execution for opportunity {} (using execute_opportunity_with_retries)", best_opp.id);
                                    // For paper/simulation, validity check can be simple or always true. Use .boxed()
                                    let is_valid_sim = || async { true }.boxed();
                                    match tx_executor.execute_opportunity_with_retries(best_opp, is_valid_sim).await {
                                        Ok(_signature) => { // signature might be unused here
                                            // execute_opportunity_with_retries returns Result<(), String>, map to Signature
                                            // In simulation/paper mode, the signature is often dummy or not needed.
                                            // We can return a dummy signature here or adjust the return type if needed.
                                            // For now, let's just log success.
                                            info!("(Paper/Simulated) Successfully processed opportunity {}", best_opp.id);
                                            // Note: Actual signature is not returned by execute_opportunity_with_retries
                                        }
                                        Err(e) => {
                                            error!("(Paper/Simulated) Failed to process opportunity {}: {}", best_opp.id, e);
                                        }
                                    }
                                } else {
                                    info!("Real Trading Mode: Attempting to execute opportunity {}", best_opp.id);
                                    // Define the async closure for opportunity validity check
                                    let engine_clone = Arc::clone(&arbitrage_engine);
                                    let opp_clone = best_opp.clone();
                                    let is_valid_real = move || {
                                        let engine = Arc::clone(&engine_clone);
                                        let opp = opp_clone.clone();
                                        async move { engine.is_opportunity_still_valid(&opp).await }.boxed()
                                    };
                                     match tx_executor.execute_opportunity_with_retries(best_opp, is_valid_real).await {
                                        Ok(_signature) => { // signature might be unused here
                                            info!("Successfully EXECUTED opportunity {}", best_opp.id);
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
                        error!("[CAT: {:?}] Error during arbitrage detection cycle: {}", e.categorize(), e);
                    }
                }
                metrics.lock().await.record_main_cycle_duration(cycle_start_time.elapsed().as_millis() as u64);
            },
            // Separate tick for fixed input opportunities if desired, or combine with main cycle
            // For simplicity, adding them sequentially in the main cycle tick here.
            // _ = async {} => { // Placeholder if you want a separate select arm
            // }
            _ = fixed_input_cycle_interval.tick() => { // Use the new interval
                info!("Fixed input and multi-hop detection cycle part.");
                let fixed_input_amount_example = app_config.fixed_input_arb_amount.unwrap_or(100.0); // Ensure fixed_input_arb_amount is in your Config

                match arbitrage_engine.discover_fixed_input_opportunities(fixed_input_amount_example).await {
                    Ok(mut fixed_input_opps) => {
                        if !fixed_input_opps.is_empty() {
                            info!("Detected {} fixed input (2-hop) opportunities.", fixed_input_opps.len());
                            fixed_input_opps.sort_by(|a, b| b.profit_pct.partial_cmp(&a.profit_pct).unwrap_or(std::cmp::Ordering::Equal));
                            if let Some(best_opp) = fixed_input_opps.first() {
                                info!("Best fixed input opportunity ID: {} with profit: {:.4}%", best_opp.id, best_opp.profit_pct);
                                // Potentially execute similar to other opportunities
                            }
                        } else {
                            info!("No fixed input (2-hop) opportunities found in this cycle part.");
                        }
                    }
                    Err(e) => {
                        error!("[CAT: {:?}] Error during fixed input opportunity detection: {}", e.categorize(), e);
                    }
                }

                match arbitrage_engine.discover_multihop_opportunities().await {
                    Ok(mut multihop_opps) => {
                        if !multihop_opps.is_empty() {
                            info!("Detected {} multi-hop opportunities.", multihop_opps.len());
                            multihop_opps.sort_by(|a, b| b.profit_pct.partial_cmp(&a.profit_pct).unwrap_or(std::cmp::Ordering::Equal));
                            if let Some(best_opp) = multihop_opps.first() {
                                info!("Best multi-hop opportunity ID: {} with profit: {:.4}%", best_opp.id, best_opp.profit_pct);
                                // Potentially execute similar to other opportunities
                            }
                        } else {
                            info!("No multi-hop opportunities found in this cycle part.");
                        }
                    }
                    Err(e) => {
                        error!("[CAT: {:?}] Error during multi-hop opportunity detection: {}", e.categorize(), e);
                    }
                }
            },
            // The WebSocket update checking logic has been moved to the dedicated arbitrage_engine.process_websocket_updates_loop task
            _ = tokio::signal::ctrl_c() => {
                info!("CTRL-C received, shutting down tasks...");
                threshold_task_handle.abort();
                congestion_task_handle.abort();
                health_check_task_handle.abort();

                // Use the manager from the engine for shutdown, if it was initialized
                if let Some(manager_arc) = &arbitrage_engine.ws_manager {
                    // Send shutdown signal to the WebSocket processing loop
                    if shutdown_tx_ws_processor.send(()).is_err() {
                        error!("[Main] Failed to send shutdown signal to WebSocket processor task.");
                    }

                    let dummy_pubkey_to_unsub = solana_sdk::system_program::id();
                    info!("[Main] Unsubscribing from dummy account {} before shutdown.", dummy_pubkey_to_unsub);
                    if let Err(e) = manager_arc.lock().await.unsubscribe(&dummy_pubkey_to_unsub).await {
                         warn!("[Main] Failed to unsubscribe from dummy account: {}", e);
                    }
                    // Call stop
                    manager_arc.lock().await.stop().await;
                    info!("[Main] WebSocket manager stop called.");
                }

                // Await the WebSocket processor task if it was started
                if let Some(handle) = ws_processor_task_handle.take() {
                    info!("[Main] Awaiting WebSocket processor task to complete...");
                    if let Err(e) = handle.await {
                        error!("[Main] WebSocket processor task panicked or encountered an error: {:?}", e);
                    }
                }
                info!("Background tasks aborted. Exiting.");
                // Call engine shutdown
                info!("[Main] Shutting down ArbitrageEngine...");
                if let Err(e) = arbitrage_engine.shutdown().await {
                    error!("[Main] Error during ArbitrageEngine shutdown: {}", e);
                }
                info!("[Main] ArbitrageEngine shutdown complete.");
                break;
            }
        }
    }
    metrics.lock().await.summary();
    Ok(())
}
